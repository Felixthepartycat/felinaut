"""Offline version-contract tests; all Git writes stay in temporary repositories."""

import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import release_version as release


def manifest(version="0.1.0", extra=""):
    return f'[package]\nname = "fixture"\nversion = "{version}"\n{extra}'


class VersionTests(unittest.TestCase):
    def test_canonical_versions_and_u64_boundaries(self):
        for text, expected in (
            ("0.0.0", (0, 0, 0)),
            ("12.34.56", (12, 34, 56)),
            (
                f"{release.U64_MAX}.{release.U64_MAX}.{release.U64_MAX}",
                (release.U64_MAX,) * 3,
            ),
        ):
            with self.subTest(text=text):
                self.assertEqual(release.parse_version(text), expected)

    def test_noncanonical_versions_are_rejected(self):
        for text in (
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.3-beta",
            "1.2.3+build",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "00.0.0",
            "+1.2.3",
            "-1.2.3",
            " 1.2.3",
            "1.2.3 ",
            "1.2.3\n",
            "\t1.2.3",
            "1.2.3\r\ntag=v9.9.9",
            "１.2.3",
            "١.2.3",
            "1.2.\x003",
            "1.2.3\x00",
            None,
            123,
            True,
            "9" * 5000 + ".0.0",
        ):
            with self.subTest(text=repr(text)[:100]):
                with self.assertRaises(ValueError):
                    release.parse_version(text)

    def test_each_component_is_bounded(self):
        for index in range(3):
            parts = ["0", "0", "0"]
            parts[index] = str(release.U64_MAX + 1)
            with self.subTest(index=index):
                with self.assertRaisesRegex(ValueError, "u64"):
                    release.parse_version(".".join(parts))

    def test_override_accepts_same_or_larger_patch(self):
        for value in ("1.2.3", "1.2.4", f"1.2.{release.U64_MAX}"):
            with self.subTest(value=value):
                self.assertEqual(release.validate_override("1.2.3", value), value)

    def test_override_rejects_wrong_series_lower_patch_or_invalid_text(self):
        for value in (
            "1.2.2",
            "1.3.3",
            "2.2.3",
            "0.2.3",
            "1.1.99",
            "",
            "v1.2.3",
            "1.2.3\nversion=9.9.9",
            f"1.2.{release.U64_MAX + 1}",
        ):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    release.validate_override("1.2.3", value)
        with self.assertRaises(ValueError):
            release.validate_override("01.2.3", "1.2.3")


class BuildVersionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="release source archive ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "Cargo.toml").write_text(manifest("1.2.3"), encoding="utf-8")
        environment = patch.dict(os.environ, {}, clear=True)
        environment.start()
        self.addCleanup(environment.stop)

    def test_source_archive_base_without_git(self):
        with patch.object(
            release.subprocess, "run", side_effect=AssertionError("Git used")
        ) as run:
            self.assertEqual(release.build_version(self.root), "1.2.3")
        run.assert_not_called()

    def test_environment_override_without_git(self):
        with patch.dict(os.environ, {"FELINAUT_RELEASE_VERSION": "1.2.90"}):
            with patch.object(
                release.subprocess, "run", side_effect=AssertionError("Git used")
            ):
                self.assertEqual(release.build_version(self.root), "1.2.90")

    def test_present_empty_and_invalid_environment_values_are_errors(self):
        for value in ("", " ", "1.2.2", "1.3.3", "2.2.3", "1.2.4\ntag=evil"):
            with self.subTest(value=value):
                with patch.dict(os.environ, {"FELINAUT_RELEASE_VERSION": value}):
                    with self.assertRaises(ValueError):
                        release.build_version(self.root)

    def test_invalid_missing_or_noncanonical_manifest_is_rejected(self):
        for text in (
            "",
            "[package]\n",
            "[package]\nversion = 123\n",
            "[package]\nversion = [1, 2, 3]\n",
            "[package]\nversion = true\n",
            "[package]\nversion = {workspace = true}\n",
            "not TOML!",
            manifest("01.2.3"),
            manifest("1.2.3-beta"),
            manifest("1.2.3 "),
            manifest(f"1.2.{release.U64_MAX + 1}"),
        ):
            with self.subTest(text=text):
                (self.root / "Cargo.toml").write_text(text, encoding="utf-8")
                with self.assertRaisesRegex(ValueError, "Cargo"):
                    release.build_version(self.root)
        (self.root / "Cargo.toml").write_bytes(b"\xff")
        with self.assertRaisesRegex(ValueError, "Cannot read"):
            release.build_version(self.root)
        (self.root / "Cargo.toml").unlink()
        with self.assertRaisesRegex(ValueError, "Cannot read"):
            release.build_version(self.root)

    def test_valid_override_cannot_hide_invalid_base(self):
        (self.root / "Cargo.toml").write_text(manifest("1.2.3-beta"), encoding="utf-8")
        with patch.dict(os.environ, {"FELINAUT_RELEASE_VERSION": "1.2.4"}):
            with self.assertRaises(ValueError):
                release.build_version(self.root)


@unittest.skipUnless(
    shutil.which("git"), "Git is required for offline history fixtures"
)
class GitVersionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="release git fixtures ")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.root = self.directory / "repository"
        self.root.mkdir()
        # Ignore user/system Git settings, injected Git paths, signing, and hooks.
        environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith("GIT_") and key != "FELINAUT_RELEASE_VERSION"
        }
        environment.update(
            {
                "HOME": str(self.directory),
                "XDG_CONFIG_HOME": str(self.directory / "xdg"),
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_CONFIG_GLOBAL": os.devnull,
                "GIT_TERMINAL_PROMPT": "0",
                "GIT_AUTHOR_NAME": "Release Fixture",
                "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
                "GIT_COMMITTER_NAME": "Release Fixture",
                "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
            }
        )
        isolated = patch.dict(os.environ, environment, clear=True)
        isolated.start()
        self.addCleanup(isolated.stop)
        self.git("init", "-b", "main")

    def git(self, *args, root=None):
        return subprocess.run(
            [
                "git",
                "--no-pager",
                "-c",
                f"core.hooksPath={os.devnull}",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "tag.gpgSign=false",
                "-C",
                str(root or self.root),
                *map(str, args),
            ],
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=15,
        ).stdout.strip()

    def commit(self, version=None, text=None, message="fixture commit"):
        if version is not None or text is not None:
            (self.root / "Cargo.toml").write_text(
                text if text is not None else manifest(version), encoding="utf-8"
            )
        self.git("add", "-A")
        self.git("commit", "--allow-empty", "-m", message)
        return self.git("rev-parse", "HEAD")

    def test_initial_commit_uses_base(self):
        self.commit("0.1.0")
        self.assertEqual(release.version_from_git(self.root), "0.1.0")

    def test_repeated_commits_and_retry_same_head_are_deterministic(self):
        self.commit("0.1.0")
        for patch_number in range(1, 5):
            self.commit()
            with self.subTest(patch=patch_number):
                self.assertEqual(
                    release.version_from_git(self.root), f"0.1.{patch_number}"
                )
                self.assertEqual(
                    release.version_from_git(self.root), f"0.1.{patch_number}"
                )
        before = self.git("status", "--porcelain")
        head = self.git("rev-parse", "HEAD")
        release.version_from_git(self.root)
        self.assertEqual(self.git("status", "--porcelain"), before)
        self.assertEqual(self.git("rev-parse", "HEAD"), head)

    def test_metadata_comments_formatting_and_rename_do_not_reset(self):
        self.commit("0.1.0")
        self.commit(text=manifest(extra='description = "metadata only"\n'))
        self.commit(text="# comment\n[package]\nversion='0.1.0'\nname=\"renamed\"\n")
        self.assertEqual(release.version_from_git(self.root), "0.1.2")

    def test_each_manual_version_change_resets_the_floor(self):
        self.commit("0.1.0")
        self.commit()
        for base, next_version in (
            ("0.2.0", "0.2.1"),
            ("1.0.0", "1.0.1"),
            ("1.0.42", "1.0.43"),
            ("0.1.0", "0.1.1"),
        ):
            with self.subTest(base=base):
                self.commit(base)
                self.assertEqual(release.version_from_git(self.root), base)
                self.commit()
                self.assertEqual(release.version_from_git(self.root), next_version)

    def test_custom_initial_patch_floor(self):
        self.commit("3.4.57")
        self.assertEqual(release.version_from_git(self.root), "3.4.57")
        self.commit()
        self.assertEqual(release.version_from_git(self.root), "3.4.58")

    def test_initial_application_introduction_after_nonapp_history(self):
        self.commit(text="invalid historical Cargo")
        (self.root / "Cargo.toml").unlink()
        self.commit(message="no application here")
        self.commit("0.1.0")
        self.commit()
        self.assertEqual(release.version_from_git(self.root), "0.1.1")

    def test_merge_counts_once_and_does_not_walk_side_branch(self):
        self.commit("0.1.0")
        self.git("checkout", "-b", "side")
        self.commit(text="malformed side-branch Cargo")
        self.commit("0.1.0")
        for _ in range(3):
            self.commit()
        self.git("checkout", "main")
        self.commit()
        self.git("merge", "--no-ff", "--no-edit", "side")
        self.assertEqual(release.version_from_git(self.root), "0.1.2")

    def test_version_bump_introduced_by_merge_uses_base(self):
        self.commit("0.1.0")
        self.git("checkout", "-b", "side")
        self.commit("1.0.0")
        self.commit()
        self.git("checkout", "main")
        self.git("merge", "--no-ff", "--no-edit", "side")
        self.assertEqual(release.version_from_git(self.root), "1.0.0")
        self.commit()
        self.assertEqual(release.version_from_git(self.root), "1.0.1")

    def test_shallow_checkout_rejected_even_if_head_is_version_bump(self):
        self.commit("0.1.0")
        self.commit("1.0.0")
        shallow = self.directory / "shallow"
        self.git("clone", "--depth=1", "--no-local", self.root.as_uri(), shallow)
        self.assertEqual(
            self.git("rev-parse", "--is-shallow-repository", root=shallow), "true"
        )
        with self.assertRaisesRegex(ValueError, r"shallow.*fetch-depth.*0"):
            release.version_from_git(shallow)
        self.assertEqual(release.build_version(shallow), "1.0.0")

    def test_linked_worktree_and_detached_head(self):
        self.commit("0.1.0")
        self.commit()
        worktree = self.directory / "linked worktree"
        self.git("worktree", "add", "--detach", worktree, "HEAD")
        self.assertTrue((worktree / ".git").is_file())
        self.assertEqual(release.version_from_git(worktree), "0.1.1")
        self.git("checkout", "--detach", "HEAD")
        self.assertEqual(release.version_from_git(self.root), "0.1.1")

    def test_working_tree_version_mismatch_rejected_but_cosmetics_allowed(self):
        self.commit("0.1.0")
        self.commit()
        (self.root / "Cargo.toml").write_text(manifest("0.2.0"), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "does not match HEAD"):
            release.version_from_git(self.root)
        self.git("add", "Cargo.toml")
        with self.assertRaisesRegex(ValueError, "does not match HEAD"):
            release.version_from_git(self.root)
        (self.root / "Cargo.toml").write_text(
            manifest(extra="# dirty comment\n"), encoding="utf-8"
        )
        self.assertEqual(release.version_from_git(self.root), "0.1.1")

    def test_invalid_historical_manifest_is_not_silently_a_boundary(self):
        for text in (
            "invalid TOML",
            "[package]\nname='missing-version'\n",
            manifest("v0.1.0"),
            manifest("0.1.0-beta"),
        ):
            with self.subTest(text=text):
                bad_commit = self.commit(text=text)
                self.commit("0.1.0")
                with self.assertRaisesRegex(ValueError, f"Git commit {bad_commit}"):
                    release.version_from_git(self.root)
                # Insert a real absence boundary before the next case.
                (self.root / "Cargo.toml").unlink()
                self.commit()

    def test_git_show_failure_is_not_treated_as_missing_manifest(self):
        self.commit("0.1.0")
        real_git = release._git

        def fail_show(root, *args):
            if args[0] == "show":
                raise ValueError("unreadable Git object")
            return real_git(root, *args)

        with patch.object(release, "_git", side_effect=fail_show):
            with self.assertRaisesRegex(ValueError, "unreadable Git object"):
                release.version_from_git(self.root)

    def test_nonregular_historical_manifest_is_not_an_absence_boundary(self):
        (self.root / "Cargo.toml").mkdir()
        (self.root / "Cargo.toml/entry").write_text("fixture", encoding="utf-8")
        bad_commit = self.commit()
        shutil.rmtree(self.root / "Cargo.toml")
        self.commit("0.1.0")
        with self.assertRaisesRegex(ValueError, f"{bad_commit}.*not a regular file"):
            release.version_from_git(self.root)

    def test_patch_overflow_checked_without_wrapping_and_max_itself_allowed(self):
        self.commit(f"1.2.{release.U64_MAX - 1}")
        self.commit()
        self.assertEqual(release.version_from_git(self.root), f"1.2.{release.U64_MAX}")
        self.commit()
        with self.assertRaisesRegex(ValueError, "overflow"):
            release.version_from_git(self.root)
        self.commit(f"1.2.{release.U64_MAX}")
        self.assertEqual(release.version_from_git(self.root), f"1.2.{release.U64_MAX}")
        self.commit()
        with self.assertRaisesRegex(ValueError, "overflow"):
            release.version_from_git(self.root)

    def test_source_archive_build_works_but_git_derivation_fails_clearly(self):
        archive = self.directory / "source archive"
        archive.mkdir()
        (archive / "Cargo.toml").write_text(manifest(), encoding="utf-8")
        self.assertEqual(release.build_version(archive), "0.1.0")
        with self.assertRaisesRegex(ValueError, "Git release history"):
            release.version_from_git(archive)
        # An extracted archive nested inside a checkout must not borrow its Git.
        self.commit("0.1.0")
        nested = self.root / "archive"
        nested.mkdir()
        (nested / "Cargo.toml").write_text(manifest(), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "worktree root"):
            release.version_from_git(nested)

    def test_unborn_repository_and_missing_head_manifest_are_errors(self):
        (self.root / "Cargo.toml").write_text(manifest(), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Git release history"):
            release.version_from_git(self.root)
        (self.root / "Cargo.toml").unlink()
        self.commit()
        (self.root / "Cargo.toml").write_text(manifest(), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "does not match HEAD"):
            release.version_from_git(self.root)

    def test_git_derivation_ignores_environment_override(self):
        self.commit("0.1.0")
        self.commit()
        with patch.dict(os.environ, {"FELINAUT_RELEASE_VERSION": "garbage"}):
            self.assertEqual(release.version_from_git(self.root), "0.1.1")
            with self.assertRaises(ValueError):
                release.build_version(self.root)

    def cli(self, *args):
        return subprocess.run(
            [
                sys.executable,
                str(Path(release.__file__).resolve()),
                "--root",
                str(self.root),
                *map(str, args),
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=30,
        )

    def test_cli_prints_only_canonical_version_and_appends_safe_labels(self):
        self.commit("0.1.0")
        self.commit()
        result = self.cli()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "0.1.1\n")
        self.assertEqual(result.stderr, "")
        output = self.directory / "github output"
        output.write_text("existing=value\n", encoding="utf-8")
        for _ in range(2):
            result = self.cli("--github-output", output)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            output.read_text(encoding="utf-8"),
            "existing=value\n" + "version=0.1.1\ntag=v0.1.1\n" * 2,
        )
        output.unlink()
        result = self.cli("--github-output", output)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            output.read_text(encoding="utf-8"), "version=0.1.1\ntag=v0.1.1\n"
        )

    def test_cli_failure_has_no_version_stdout_or_output_writes(self):
        self.commit("0.1.0")
        (self.root / "Cargo.toml").write_text(manifest("0.2.0"), encoding="utf-8")
        output = self.directory / "github output"
        output.write_text("existing=value\n", encoding="utf-8")
        result = self.cli("--github-output", output)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("does not match HEAD", result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertEqual(output.read_text(encoding="utf-8"), "existing=value\n")

    def test_cli_injection_is_rejected_before_output(self):
        self.commit(text='[package]\nversion = "0.1.0\\ntag=evil"\n')
        output = self.directory / "github output"
        result = self.cli("--github-output", output)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertFalse(output.exists())

    def test_cli_reports_output_io_failure_without_success_stdout(self):
        self.commit("0.1.0")
        result = self.cli("--github-output", self.directory / "missing/output")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("Release version error", result.stderr)


class ReleaseTagTests(unittest.TestCase):
    repository = "fixture-owner/fixture.repo"
    tag = "v1.2.3"
    commit = "a" * 40
    tag_sha = "b" * 40

    def setUp(self):
        mocked = patch.object(release.subprocess, "run")
        self.run = mocked.start()
        self.addCleanup(mocked.stop)
        self.run.return_value = self.response([])

    def tearDown(self):
        for call in self.run.call_args_list:
            # All requests, including error paths and nested tags, stay read-only.
            self.assertEqual(call.args[0][:4], ["gh", "api", "--method", "GET"])
            self.assertEqual(len(call.args[0]), 5)
            self.assertTrue(call.kwargs["check"])
            self.assertTrue(call.kwargs["capture_output"])
            self.assertTrue(call.kwargs["text"])
            self.assertEqual(call.kwargs["encoding"], "utf-8")
            self.assertGreater(call.kwargs["timeout"], 0)
            self.assertLessEqual(call.kwargs["timeout"], 60)
            self.assertNotIn("shell", call.kwargs)

    def response(self, value):
        return subprocess.CompletedProcess([], 0, stdout=json.dumps(value))

    def ref(self, tag=None, kind="commit", sha=None):
        return {
            "ref": f"refs/tags/{tag or self.tag}",
            "object": {"type": kind, "sha": sha or self.commit},
        }

    def verify(self):
        return release.verify_release_tag(self.repository, self.tag, self.commit)

    def assert_endpoint(self, index, suffix):
        self.assertEqual(
            self.run.call_args_list[index].args[0][-1],
            f"repos/{self.repository}/git/{suffix}",
        )

    def test_absent_exact_tag_is_allowed(self):
        self.assertIsNone(self.verify())
        self.assertEqual(self.run.call_count, 1)
        self.assert_endpoint(0, "matching-refs/tags/v1.2.3")

    def test_prefix_matches_do_not_prove_exact_tag_exists(self):
        self.run.return_value = self.response(
            [
                self.ref("v1.2.30", sha="c" * 40),
                self.ref("v1.2.3-extra", kind="tag", sha=self.tag_sha),
            ]
        )
        self.assertIsNone(self.verify())
        self.assertEqual(self.run.call_count, 1)

    def test_prefix_tags_pointing_to_other_git_object_types_are_irrelevant(self):
        self.run.return_value = self.response(
            [
                self.ref("v1.2.30", kind="blob"),
                self.ref("v1.2.3-tree", kind="tree"),
            ]
        )
        self.assertIsNone(self.verify())
        self.assertEqual(self.run.call_count, 1)

    def test_exact_lightweight_tag_between_prefixes_matches_commit(self):
        self.run.return_value = self.response(
            [
                self.ref("v1.2.30", sha="c" * 40),
                self.ref(),
                self.ref("v1.2.3-extra", sha="d" * 40),
            ]
        )
        self.assertIsNone(self.verify())
        self.assertEqual(self.run.call_count, 1)

    def test_lightweight_tag_mismatch_is_an_error(self):
        self.run.return_value = self.response([self.ref(sha="c" * 40)])
        with self.assertRaisesRegex(ValueError, "Existing release tag.*not commit"):
            self.verify()
        self.assertEqual(self.run.call_count, 1)

    def test_full_64_character_commits_are_supported_not_prefix_matched(self):
        commit = "a" * 64
        self.run.return_value = self.response([self.ref(sha=commit)])
        self.assertIsNone(release.verify_release_tag(self.repository, self.tag, commit))
        with self.assertRaisesRegex(ValueError, "not commit"):
            self.verify()

    def test_safe_repository_names_and_version_boundaries(self):
        for repository in (
            "a/b",
            "Owner-Name/.github",
            "owner/repo_name-1.2",
            "a" * 39 + "/" + "b" * 100,
        ):
            with self.subTest(repository=repository):
                self.assertIsNone(
                    release.verify_release_tag(
                        repository, f"v0.{release.U64_MAX}.0", self.commit
                    )
                )

    def test_invalid_inputs_fail_before_any_api_request(self):
        cases = []
        for repository in (
            "",
            "owner",
            "owner/repo/extra",
            "/repo",
            "owner/",
            "../repo",
            "owner/.",
            "owner/..",
            "owner/repo?query",
            "owner/repo#fragment",
            "owner/repo%2fextra",
            "owner/repo\n",
            " owner/repo",
            "owner/re po",
            "https://github.com/owner/repo",
            "--hostname/attacker",
            "ow_ner/repo",
            "ow\\ner/repo",
            "owner/仓库",
            "a" * 40 + "/repo",
            "a/" + "b" * 101,
            None,
            123,
        ):
            cases.append((repository, self.tag, self.commit))
        for tag in (
            "",
            "1.2.3",
            "vv1.2.3",
            "v01.2.3",
            "v1.02.3",
            "v1.2.03",
            "v1.2.3-beta",
            "v1.2.3+build",
            "v1.2.3\n",
            "v1.2.3/extra",
            "v1.2.3?query",
            f"v1.2.{release.U64_MAX + 1}",
            None,
            123,
        ):
            cases.append((self.repository, tag, self.commit))
        for commit in (
            "",
            "a" * 39,
            "a" * 41,
            "a" * 63,
            "a" * 65,
            "A" * 40,
            "g" * 40,
            "a" * 40 + "\n",
            " " + "a" * 40,
            "HEAD",
            "--help",
            "a" * 39 + "/",
            None,
            123,
        ):
            cases.append((self.repository, self.tag, commit))
        for repository, tag, commit in cases:
            with self.subTest(repository=repository, tag=tag, commit=commit):
                with self.assertRaises(ValueError):
                    release.verify_release_tag(repository, tag, commit)
        self.run.assert_not_called()

    def test_annotated_tag_resolves_to_commit(self):
        self.run.side_effect = [
            self.response([self.ref(kind="tag", sha=self.tag_sha)]),
            self.response(
                {
                    "sha": self.tag_sha,
                    "object": {
                        "type": "commit",
                        "sha": self.commit,
                    },
                }
            ),
        ]
        self.assertIsNone(self.verify())
        self.assertEqual(self.run.call_count, 2)
        self.assert_endpoint(1, f"tags/{self.tag_sha}")

    def test_annotated_tag_commit_mismatch_is_an_error(self):
        self.run.side_effect = [
            self.response([self.ref(kind="tag", sha=self.tag_sha)]),
            self.response({"object": {"type": "commit", "sha": "c" * 40}}),
        ]
        with self.assertRaisesRegex(ValueError, "not commit"):
            self.verify()

    def test_five_nested_annotations_can_resolve_to_commit(self):
        tags = [f"{number:064x}" for number in range(1, 6)]
        self.run.side_effect = [
            self.response([self.ref(kind="tag", sha=tags[0])]),
            *[
                self.response(
                    {
                        "sha": sha,
                        "object": {
                            "type": "tag" if index < 4 else "commit",
                            "sha": tags[index + 1] if index < 4 else self.commit,
                        },
                    }
                )
                for index, sha in enumerate(tags)
            ],
        ]
        self.assertIsNone(self.verify())
        self.assertEqual(self.run.call_count, 6)
        for index, sha in enumerate(tags, 1):
            self.assert_endpoint(index, f"tags/{sha}")

    def test_more_than_five_annotations_fail_before_sixth_lookup(self):
        tags = [f"{number:040x}" for number in range(1, 7)]
        self.run.side_effect = [
            self.response([self.ref(kind="tag", sha=tags[0])]),
            *[
                self.response({"object": {"type": "tag", "sha": tags[index + 1]}})
                for index in range(5)
            ],
        ]
        with self.assertRaisesRegex(ValueError, "maximum depth of 5"):
            self.verify()
        self.assertEqual(self.run.call_count, 6)

    def test_self_cycle_fails_without_repeating_lookup(self):
        self.run.side_effect = [
            self.response([self.ref(kind="tag", sha=self.tag_sha)]),
            self.response({"object": {"type": "tag", "sha": self.tag_sha}}),
        ]
        with self.assertRaisesRegex(ValueError, "Cycle"):
            self.verify()
        self.assertEqual(self.run.call_count, 2)

    def test_multi_tag_cycle_fails_without_repeating_lookup(self):
        second = "c" * 40
        self.run.side_effect = [
            self.response([self.ref(kind="tag", sha=self.tag_sha)]),
            self.response({"object": {"type": "tag", "sha": second}}),
            self.response({"object": {"type": "tag", "sha": self.tag_sha}}),
        ]
        with self.assertRaisesRegex(ValueError, "Cycle"):
            self.verify()
        self.assertEqual(self.run.call_count, 3)

    def test_malformed_json_fails_instead_of_implying_absence(self):
        for text in (
            "",
            "not JSON",
            "[",
            "[] trailing garbage",
        ):
            with self.subTest(text=text):
                self.run.return_value = subprocess.CompletedProcess([], 0, stdout=text)
                with self.assertRaisesRegex(ValueError, "Malformed GitHub API JSON"):
                    self.verify()

    def test_json_parser_depth_errors_fail_clearly(self):
        with patch.object(
            release.json, "loads", side_effect=RecursionError("JSON nesting limit")
        ):
            with self.assertRaisesRegex(ValueError, "Malformed GitHub API JSON"):
                self.verify()

    def test_malformed_matching_refs_schema_fails(self):
        for value in (
            None,
            {},
            {"message": "Not Found"},
            "[]",
            [None],
            [1],
            [{}],
            [{"ref": 123}],
            [{"ref": "refs/heads/v1.2.3", "object": {}}],
            [{"ref": f"refs/tags/{self.tag}"}],
        ):
            with self.subTest(value=value):
                self.run.return_value = self.response(value)
                with self.assertRaisesRegex(ValueError, "Malformed GitHub"):
                    self.verify()

    def test_duplicate_exact_refs_fail(self):
        self.run.return_value = self.response([self.ref(), self.ref()])
        with self.assertRaisesRegex(ValueError, "duplicate exact tag"):
            self.verify()

    def test_unexpected_types_and_malformed_shas_fail_at_any_depth(self):
        for target in (
            None,
            [],
            {},
            {"type": "blob", "sha": self.commit},
            {"type": "tree", "sha": self.commit},
            {"type": ["commit"], "sha": self.commit},
            {"type": "commit"},
            {"type": "commit", "sha": "a" * 39},
            {"type": "tag", "sha": "../../malicious"},
            {"type": "commit", "sha": "A" * 40},
            {"type": "commit", "sha": 123},
        ):
            for annotated in (False, True):
                with self.subTest(target=target, annotated=annotated):
                    invalid = {"ref": f"refs/tags/{self.tag}", "object": target}
                    self.run.side_effect = (
                        [
                            self.response([self.ref(kind="tag", sha=self.tag_sha)]),
                            self.response({"object": target}),
                        ]
                        if annotated
                        else [self.response([invalid])]
                    )
                    with self.assertRaisesRegex(
                        ValueError, "GitHub release tag object"
                    ):
                        self.verify()

    def test_malformed_annotated_response_cannot_use_target_commitish(self):
        for value in (None, [], "commit", {}, {"targetCommitish": self.commit}):
            with self.subTest(value=value):
                self.run.side_effect = [
                    self.response([self.ref(kind="tag", sha=self.tag_sha)]),
                    self.response(value),
                ]
                with self.assertRaisesRegex(ValueError, "Malformed GitHub"):
                    self.verify()

    def test_annotated_response_cannot_substitute_another_object(self):
        self.run.side_effect = [
            self.response([self.ref(kind="tag", sha=self.tag_sha)]),
            self.response(
                {
                    "sha": "c" * 40,
                    "object": {
                        "type": "commit",
                        "sha": self.commit,
                    },
                }
            ),
        ]
        with self.assertRaisesRegex(ValueError, "SHA does not match requested object"):
            self.verify()

    def test_nested_malformed_json_is_not_ignored(self):
        self.run.side_effect = [
            self.response([self.ref(kind="tag", sha=self.tag_sha)]),
            subprocess.CompletedProcess([], 0, stdout="not JSON"),
        ]
        with self.assertRaisesRegex(ValueError, "Malformed GitHub API JSON"):
            self.verify()

    def test_api_errors_including_404_fail_at_each_lookup(self):
        for annotated in (False, True):
            for status in (401, 403, 404, 429, 500):
                with self.subTest(annotated=annotated, status=status):
                    error = subprocess.CalledProcessError(
                        1, ["gh", "api"], stderr=f"HTTP {status}"
                    )
                    self.run.side_effect = (
                        [self.response([self.ref(kind="tag", sha=self.tag_sha)]), error]
                        if annotated
                        else [error]
                    )
                    with self.assertRaisesRegex(
                        ValueError, f"GitHub API failed.*HTTP {status}"
                    ):
                        self.verify()

    def test_timeouts_fail_at_each_lookup(self):
        for annotated in (False, True):
            with self.subTest(annotated=annotated):
                error = subprocess.TimeoutExpired(["gh", "api"], 30)
                self.run.side_effect = (
                    [self.response([self.ref(kind="tag", sha=self.tag_sha)]), error]
                    if annotated
                    else [error]
                )
                with self.assertRaisesRegex(ValueError, "GitHub API timed out"):
                    self.verify()

    def test_missing_gh_and_invalid_response_encoding_fail_clearly(self):
        for error in (
            FileNotFoundError("gh missing"),
            UnicodeDecodeError("utf-8", b"\xff", 0, 1, "invalid byte"),
        ):
            with self.subTest(error=type(error).__name__):
                self.run.side_effect = error
                with self.assertRaisesRegex(ValueError, "Cannot read GitHub API"):
                    self.verify()


class GitFailureTests(unittest.TestCase):
    def test_git_processes_have_bounded_timeouts_and_read_only_commands(self):
        completed = subprocess.CompletedProcess([], 0, stdout="false\n")
        with patch.object(release.subprocess, "run", return_value=completed) as run:
            self.assertEqual(
                release._git(Path("fixture"), "rev-parse", "--is-shallow-repository"),
                "false\n",
            )
        command = run.call_args.args[0]
        self.assertEqual(
            command,
            [
                "git",
                "--no-pager",
                "-C",
                "fixture",
                "rev-parse",
                "--is-shallow-repository",
            ],
        )
        self.assertGreater(run.call_args.kwargs["timeout"], 0)
        self.assertLessEqual(run.call_args.kwargs["timeout"], 60)
        self.assertNotIn("shell", run.call_args.kwargs)

    def test_git_timeout_missing_executable_and_command_failure_are_clear(self):
        cases = (
            (subprocess.TimeoutExpired(["git"], 30), "timed out"),
            (FileNotFoundError("git missing"), "Cannot read Git release history"),
            (
                subprocess.CalledProcessError(
                    128, ["git"], stderr="not a git repository"
                ),
                "not a git repository",
            ),
            (
                UnicodeDecodeError("utf-8", b"\xff", 0, 1, "invalid byte"),
                "Git release history",
            ),
        )
        for error, message in cases:
            with self.subTest(error=type(error).__name__):
                with patch.object(release.subprocess, "run", side_effect=error):
                    with self.assertRaisesRegex(ValueError, message):
                        release._git(Path("fixture"), "rev-parse", "HEAD")

    def test_main_defaults_to_script_root_and_prints_safe_labels(self):
        stdout = io.StringIO()
        with patch.object(release, "version_from_git", return_value="1.2.3") as derive:
            with contextlib.redirect_stdout(stdout):
                self.assertEqual(release.main([]), 0)
        derive.assert_called_once_with(release.ROOT)
        self.assertEqual(stdout.getvalue(), "1.2.3\n")


if __name__ == "__main__":
    unittest.main()
