"""Exercise release workflow Python and real, offline Mach-O architecture checks."""

import hashlib
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from contextlib import chdir, contextmanager
from pathlib import Path
from unittest.mock import call, patch

ROOT = Path(__file__).resolve().parent.parent
BINARY = Path("dist/Felinaut.app/Contents/MacOS/felinaut")
ARCHITECTURES = ("arm64", "x86_64")


@unittest.skipUnless(sys.platform == "darwin", "requires macOS Xcode tools")
class ReleaseArchitectureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixtures = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.fixtures.cleanup)
        cls.lipo = subprocess.check_output(
            ["xcrun", "--find", "lipo"],
            text=True,
            timeout=10,
        ).strip()
        cls.objects = {}
        for arch in ARCHITECTURES:
            output = Path(cls.fixtures.name) / f"{arch}.o"
            subprocess.run(
                [
                    "xcrun",
                    "clang",
                    "-target",
                    f"{arch}-apple-macos12.0",
                    "-x",
                    "c",
                    "-c",
                    "-o",
                    str(output),
                    "-",
                ],
                input="int fixture(void) { return 0; }\n",
                text=True,
                check=True,
                capture_output=True,
                timeout=30,
            )
            cls.objects[arch] = output

    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="felinaut workflow ")
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.binary = self.directory / BINARY
        self.binary.parent.mkdir(parents=True)

    def verify(self, arch):
        lines = (ROOT / ".github/workflows/release.yml").read_text().splitlines()
        commands = [line.strip() for line in lines if line.strip().startswith("lipo ")]
        self.assertEqual(len(commands), 1, "expected one workflow architecture check")
        command = shlex.split(commands[0].replace("${{ matrix.arch }}", arch))
        command[0] = self.lipo
        # Xcode 16 consumes every operand after this option as an architecture;
        # newer lipo versions also accept the broken ordering, masking CI failures.
        self.assertEqual(
            command[command.index("-verify_arch") + 1 :],
            [arch],
            "the binary must precede the variadic -verify_arch option",
        )
        return subprocess.run(
            command,
            cwd=self.directory,
            text=True,
            capture_output=True,
            timeout=10,
        )

    def test_workflow_accepts_each_matching_architecture(self):
        for arch in ARCHITECTURES:
            with self.subTest(arch=arch):
                shutil.copy2(self.objects[arch], self.binary)
                result = self.verify(arch)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_workflow_rejects_the_wrong_architecture(self):
        for actual, requested in [("arm64", "x86_64"), ("x86_64", "arm64")]:
            with self.subTest(actual=actual, requested=requested):
                shutil.copy2(self.objects[actual], self.binary)
                self.assertNotEqual(self.verify(requested).returncode, 0)

    def test_workflow_accepts_both_slices_of_a_universal_fixture(self):
        subprocess.run(
            [
                self.lipo,
                "-create",
                *map(str, self.objects.values()),
                "-output",
                str(self.binary),
            ],
            check=True,
            capture_output=True,
            timeout=10,
        )
        for arch in ARCHITECTURES:
            with self.subTest(arch=arch):
                result = self.verify(arch)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_workflow_rejects_invalid_binary(self):
        self.binary.write_bytes(b"not a Mach-O file")
        self.assertNotEqual(self.verify("arm64").returncode, 0)

    def test_workflow_rejects_missing_binary(self):
        self.assertNotEqual(self.verify("arm64").returncode, 0)


def workflow_python(job):
    """Extract a job's Python heredoc without interpreting YAML or shell code."""
    lines = (ROOT / ".github/workflows/release.yml").read_text().splitlines()
    start = lines.index(f"  {job}:") + 1
    end = next(
        (
            i
            for i in range(start, len(lines))
            if lines[i].startswith("  ") and not lines[i].startswith("    ")
        ),
        len(lines),
    )
    blocks = []
    for i in range(start, end):
        if lines[i].strip() != "python3 - <<'PY'":
            continue
        indent = lines[i][: len(lines[i]) - len(lines[i].lstrip())]
        stop = lines.index(f"{indent}PY", i + 1, end)
        body = lines[i + 1 : stop]
        if any(line and not line.startswith(indent) for line in body):
            raise ValueError("Python heredoc escaped its run block")
        blocks.append("\n".join(line.removeprefix(indent) for line in body))
    if len(blocks) != 1:
        raise ValueError(f"Expected one Python heredoc in {job}, found {len(blocks)}")
    return compile(blocks[0], f"release.yml:{job}", "exec")


class ReleasePythonTests(unittest.TestCase):
    VERSION = "8.7.6"
    SHA = "0123456789abcdef0123456789abcdef01234567"
    SHORT = SHA[:12]
    PAYLOAD = b"PK\x03\x04universal fixture\x00\xff\n"

    def asset(self, arch, *, version=None, short=None):
        return (
            f"Felinaut-{version or self.VERSION}-{short or self.SHORT}-macos-{arch}.zip"
        )

    @contextmanager
    def workspace(self):
        outputs = {
            ("git", "rev-parse", "HEAD"): self.SHA + "\n",
            ("git", "rev-parse", "--short=12", "HEAD"): self.SHORT + "\n",
        }
        with (
            tempfile.TemporaryDirectory(prefix="felinaut workflow ") as directory,
            chdir(directory),
            patch.dict(os.environ, {"GITHUB_ENV": str(Path(directory) / "github-env")}),
            patch(
                "subprocess.check_output",
                side_effect=lambda args, **kw: outputs[tuple(args)],
            ) as git,
            patch("subprocess.run", side_effect=self.native_command) as native,
        ):
            self.git, self.native = git, native
            Path("Cargo.toml").write_text(f'[package]\nversion = "{self.VERSION}"\n')
            yield

    def native_command(self, args, **kwargs):
        if args[0] == "ditto":
            (Path(args[-1]) / "Felinaut.app").mkdir()
        elif args[0] == "python3":
            for arch in ARCHITECTURES:
                self.assertTrue(Path(f"native-apps/{arch}/Felinaut.app").is_dir())
            Path(args[-1]).write_bytes(self.PAYLOAD)
        else:
            self.fail(f"Unexpected native command: {args}")
        return subprocess.CompletedProcess(args, 0)

    def native_inputs(self, overrides=None):
        for arch in ARCHITECTURES:
            directory = Path("native-zips") / arch
            directory.mkdir(parents=True)
            for name in (overrides or {}).get(arch, [self.asset(arch)]):
                (directory / name).write_bytes(b"native ZIP fixture")

    def assembly_calls(self):
        return [
            call(
                [
                    "ditto",
                    "-x",
                    "-k",
                    f"native-zips/{arch}/{self.asset(arch)}",
                    f"native-apps/{arch}",
                ],
                check=True,
                timeout=120,
            )
            for arch in ARCHITECTURES
        ] + [
            call(
                [
                    "python3",
                    "scripts/assemble_universal_macos.py",
                    "--arm64",
                    "native-apps/arm64/Felinaut.app",
                    "--x86-64",
                    "native-apps/x86_64/Felinaut.app",
                    "--output",
                    f"release-assets/{self.asset('universal')}",
                ],
                check=True,
                timeout=600,
            )
        ]

    def execute(self, job):
        exec(workflow_python(job), {"__name__": "__main__"})

    def test_universal_extracts_current_native_zips_and_calls_assembler(self):
        with self.workspace():
            self.native_inputs()
            self.execute("universal")
            self.assertEqual(self.native.call_args_list, self.assembly_calls())
            self.git.assert_called_once_with(
                ["git", "rev-parse", "--short=12", "HEAD"],
                text=True,
            )
            assets = Path("release-assets")
            self.assertEqual(list(assets.iterdir()), [assets / self.asset("universal")])
            self.assertEqual(
                (assets / self.asset("universal")).read_bytes(), self.PAYLOAD
            )

    def test_universal_rejects_missing_stale_misnamed_or_extra_native_artifacts(self):
        for arch in ARCHITECTURES:
            cases = {
                "missing": [],
                "wrong commit": [self.asset(arch, short="deadbeef0000")],
                "wrong version": [self.asset(arch, version="1.2.3")],
                "wrong architecture": [
                    self.asset("x86_64" if arch == "arm64" else "arm64")
                ],
                "extra": [self.asset(arch), "unexpected.txt"],
            }
            for reason, names in cases.items():
                with self.subTest(arch=arch, reason=reason), self.workspace():
                    self.native_inputs({arch: names})
                    with self.assertRaisesRegex(
                        RuntimeError, f"unexpected native {arch}"
                    ):
                        self.execute("universal")
                    for actual in self.native.call_args_list:
                        self.assertIn(actual, self.assembly_calls()[:2])
                    self.assertFalse(Path("release-assets").exists())

    def test_universal_stops_on_native_command_failure_without_a_zip(self):
        for index in range(3):
            with self.subTest(command=index), self.workspace():
                self.native_inputs()
                calls = self.assembly_calls()

                def fail_command(args, **kwargs):
                    if args == calls[index].args[0]:
                        raise subprocess.CalledProcessError(1, args)
                    return self.native_command(args, **kwargs)

                self.native.side_effect = fail_command
                with self.assertRaises(subprocess.CalledProcessError):
                    self.execute("universal")
                self.assertEqual(self.native.call_args_list, calls[: index + 1])
                self.assertEqual(list(Path("release-assets").glob("*")), [])

    def test_publish_checksums_only_universal_zip_and_exports_exact_metadata(self):
        with self.workspace():
            assets = Path("release-assets")
            assets.mkdir()
            name = self.asset("universal")
            (assets / name).write_bytes(self.PAYLOAD)
            env = Path(os.environ["GITHUB_ENV"])
            env.write_text("EXISTING=value\n")
            self.execute("publish")
            self.assertEqual(
                (assets / "SHA256SUMS").read_text(),
                f"{hashlib.sha256(self.PAYLOAD).hexdigest()}  {name}\n",
            )
            self.assertEqual({p.name for p in assets.iterdir()}, {name, "SHA256SUMS"})
            self.assertEqual(
                env.read_text(),
                f"EXISTING=value\nRELEASE_TAG=main-{self.SHA}\n"
                f"RELEASE_TITLE=Felinaut {self.VERSION} ({self.SHORT})\n",
            )
            self.assertEqual(
                self.git.call_args_list,
                [
                    call(["git", "rev-parse", "HEAD"], text=True),
                    call(["git", "rev-parse", "--short=12", "HEAD"], text=True),
                ],
            )
            self.native.assert_not_called()

    def test_publish_rejects_missing_thin_stale_or_extra_assets_without_outputs(self):
        universal = self.asset("universal")
        cases = {
            "missing": [],
            "old thin pair": [self.asset(arch) for arch in ARCHITECTURES],
            "wrong commit": [self.asset("universal", short="deadbeef0000")],
            "wrong version": [self.asset("universal", version="1.2.3")],
            "extra thin ZIP": [universal, self.asset("arm64")],
            "extra non-ZIP": [universal, "unexpected.txt"],
            "stale checksum": [universal, "SHA256SUMS"],
        }
        for reason, names in cases.items():
            with self.subTest(reason=reason), self.workspace():
                assets = Path("release-assets")
                assets.mkdir()
                for name in names:
                    (assets / name).write_bytes(self.PAYLOAD)
                env = Path(os.environ["GITHUB_ENV"])
                env.write_text("EXISTING=value\n")
                # Accept the existing assert and its planned explicit RuntimeError guard.
                with self.assertRaisesRegex(
                    (RuntimeError, AssertionError),
                    "^Missing or unexpected release assets$",
                ):
                    self.execute("publish")
                self.assertEqual(
                    {p.name: p.read_bytes() for p in assets.iterdir()},
                    {name: self.PAYLOAD for name in names},
                )
                self.assertEqual(env.read_text(), "EXISTING=value\n")
                self.native.assert_not_called()


if __name__ == "__main__":
    unittest.main()
