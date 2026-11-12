"""Portable validation and real, offline macOS universal-app assembly fixtures.

Native fixtures are linked and signed, but never launched. No browser build,
Finder session, accounts, network access, or signing credentials are needed.
"""

import contextlib
import hashlib
import io
import os
import plistlib
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from pathlib import Path
from unittest.mock import patch

import assemble_universal_macos as universal


def icon_bytes(color):
    """A tiny valid ICNS containing a 16x16 PNG, independent of AppKit."""

    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data))
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", 16, 16, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress((b"\0" + bytes(color) * 16) * 16))
    png += chunk(b"IEND", b"")
    element = b"icp4" + struct.pack(">I", len(png) + 8) + png
    return b"icns" + struct.pack(">I", len(element) + 8) + element


def make_bundle(app, binary=b"fixture executable", color=(255, 0, 0)):
    for directory in universal.DIRECTORIES:
        (app / directory).mkdir(parents=True, exist_ok=True)
    (app / universal.EXECUTABLE).write_bytes(binary)
    (app / universal.EXECUTABLE).chmod(0o755)
    (app / "Contents/Info.plist").write_bytes(plistlib.dumps(universal.expected_info()))
    (app / "Contents/PkgInfo").write_bytes(b"APPL????")
    (app / "Contents/Resources/Felinaut.icns").write_bytes(icon_bytes(color))
    (app / "Contents/Resources/LICENSE").write_bytes(
        (universal.ROOT / "LICENSE").read_bytes()
    )
    (app / "Contents/_CodeSignature/CodeResources").write_bytes(b"fixture signature")
    return app


def snapshot(app):
    result = {}
    for path in [app, *sorted(app.rglob("*"))]:
        info = path.lstat()
        if path.is_symlink():
            content = os.readlink(path)
        elif path.is_file():
            content = hashlib.sha256(path.read_bytes()).hexdigest()
        else:
            content = None
        result[str(path.relative_to(app))] = (info.st_mode, info.st_mtime_ns, content)
    return result


class ValidationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="universal validation ")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.arm64 = make_bundle(self.directory / "arm64/Felinaut.app")
        self.intel = make_bundle(
            self.directory / "intel/Felinaut.app", color=(0, 0, 255)
        )
        self.output = self.directory / "Felinaut-version-hash-macos-universal.zip"
        self.expected = universal.expected_info()

    def test_current_metadata_and_different_icons_are_accepted(self):
        self.assertEqual(
            universal.validate_bundle(self.arm64, self.expected), self.expected
        )
        self.assertEqual(
            universal.validate_bundle(self.intel, self.expected), self.expected
        )
        self.assertNotEqual(
            (self.arm64 / "Contents/Resources/Felinaut.icns").read_bytes(),
            (self.intel / "Contents/Resources/Felinaut.icns").read_bytes(),
        )
        universal.validate_paths(self.arm64, self.intel, self.output)

    def test_missing_entries_are_rejected(self):
        for relative in sorted(universal.FILES | universal.DIRECTORIES):
            with self.subTest(relative=relative):
                app = (
                    self.directory
                    / "missing"
                    / relative.replace("/", "_")
                    / "Felinaut.app"
                )
                make_bundle(app)
                path = app / relative
                if path.is_dir():
                    shutil.rmtree(path)
                else:
                    path.unlink()
                with self.assertRaisesRegex(
                    universal.AssemblyError, "Missing bundle entries"
                ):
                    universal.validate_bundle(app, self.expected)

    def test_unexpected_helpers_frameworks_and_resources_are_rejected(self):
        for relative in (
            "Contents/Frameworks",
            "Contents/Helpers",
            "Contents/MacOS/helper",
            "Contents/Resources/helper.app",
            "Contents/embedded.provisionprofile",
            "Contents/Resources/extra.dylib",
        ):
            with self.subTest(relative=relative):
                path = self.arm64 / relative
                path.write_bytes(b"unexpected")
                try:
                    with self.assertRaisesRegex(
                        universal.AssemblyError, "Unexpected bundle entry"
                    ):
                        universal.validate_bundle(self.arm64, self.expected)
                finally:
                    path.unlink()

    def test_nonregular_file_is_rejected(self):
        if not hasattr(os, "mkfifo"):
            self.skipTest("requires FIFO support")
        path = self.arm64 / "Contents/Resources/LICENSE"
        path.unlink()
        os.mkfifo(path)
        with self.assertRaisesRegex(universal.AssemblyError, "file type"):
            universal.validate_bundle(self.arm64, self.expected)

    def test_symlink_files_and_directories_are_rejected(self):
        for relative in (
            "Contents/Resources/LICENSE",
            "Contents/MacOS",
            "Contents/_CodeSignature",
        ):
            with self.subTest(relative=relative):
                app = (
                    self.directory
                    / "symlinks"
                    / relative.replace("/", "_")
                    / "Felinaut.app"
                )
                make_bundle(app)
                path = app / relative
                target = path.with_name(path.name + "-outside")
                path.rename(target)
                # Keep the symlink target outside the bundle so layout validation
                # cannot reject the target as an extra entry first.
                outside = self.directory / (relative.replace("/", "_") + "-target")
                target.rename(outside)
                path.symlink_to(outside, target_is_directory=outside.is_dir())
                with self.assertRaisesRegex(universal.AssemblyError, "Symlinks"):
                    universal.validate_bundle(app, self.expected)

    def test_symlink_app_root_is_rejected(self):
        app = self.directory / "linked/Felinaut.app"
        app.parent.mkdir()
        app.symlink_to(self.arm64, target_is_directory=True)
        with self.assertRaisesRegex(universal.AssemblyError, "non-symlink"):
            universal.validate_bundle(app, self.expected)

    def test_macho_cannot_hide_under_an_allowed_resource_name(self):
        for magic in universal.MACHO_MAGICS:
            with self.subTest(magic=magic):
                (self.arm64 / "Contents/Resources/LICENSE").write_bytes(
                    magic + b"payload"
                )
                with self.assertRaisesRegex(
                    universal.AssemblyError, "Unexpected bundled Mach-O"
                ):
                    universal.validate_bundle(self.arm64, self.expected)

    def test_wrong_bundle_name_and_nondirectory_are_rejected(self):
        for path in (
            self.directory / "Other.app",
            self.directory / "file/Felinaut.app",
        ):
            with self.subTest(path=path):
                path.parent.mkdir(exist_ok=True)
                path.write_bytes(b"not a bundle")
                with self.assertRaisesRegex(
                    universal.AssemblyError, "Felinaut.app directory"
                ):
                    universal.validate_bundle(path, self.expected)

    def test_malformed_and_non_dictionary_plists_are_rejected(self):
        for data in (
            b"not a plist",
            b"bplist00broken",
            b"<?xml version='1.0'?><plist><dict>",
            plistlib.dumps(["not a dictionary"]),
        ):
            with self.subTest(data=data):
                (self.arm64 / "Contents/Info.plist").write_bytes(data)
                with self.assertRaisesRegex(universal.AssemblyError, "Info.plist"):
                    universal.validate_bundle(self.arm64, self.expected)

    def test_matching_but_stale_metadata_is_rejected(self):
        info = dict(self.expected, CFBundleVersion="99.0.0")
        for app in (self.arm64, self.intel):
            (app / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
            with self.assertRaisesRegex(
                universal.AssemblyError, "current checkout metadata"
            ):
                universal.validate_bundle(app, self.expected)

    def test_invalid_pkginfo_is_rejected(self):
        (self.arm64 / "Contents/PkgInfo").write_bytes(b"WRONG???")
        with self.assertRaisesRegex(universal.AssemblyError, "PkgInfo"):
            universal.validate_bundle(self.arm64, self.expected)

    def test_existing_output_and_dangling_symlink_are_not_overwritten(self):
        self.output.write_bytes(b"keep me")
        with self.assertRaisesRegex(universal.AssemblyError, "already exists"):
            universal.assemble(self.arm64, self.intel, self.output)
        self.assertEqual(self.output.read_bytes(), b"keep me")
        self.output.unlink()
        self.output.symlink_to(self.directory / "absent")
        with self.assertRaisesRegex(universal.AssemblyError, "already exists"):
            universal.assemble(self.arm64, self.intel, self.output)
        self.assertTrue(self.output.is_symlink())

    def test_output_must_be_zip_with_existing_parent_and_outside_inputs(self):
        for output in (
            self.directory / "out.tar",
            self.directory / "missing/out.zip",
            self.arm64 / "out.zip",
            self.intel / "Contents/out.zip",
        ):
            with self.subTest(output=output):
                with self.assertRaises(universal.AssemblyError):
                    universal.validate_paths(self.arm64, self.intel, output)
                self.assertFalse(output.exists())

    def test_missing_or_repeated_inputs_are_rejected(self):
        with self.assertRaisesRegex(universal.AssemblyError, "not found"):
            universal.validate_paths(self.directory / "absent", self.intel, self.output)
        with self.assertRaisesRegex(universal.AssemblyError, "different app bundles"):
            universal.validate_paths(self.arm64, self.arm64, self.output)
        self.assertFalse(self.output.exists())

    def test_exact_architecture_check_rejects_empty_extra_and_duplicate_archs(self):
        for actual in ("", "arm64 x86_64", "arm64 arm64", "x86_64"):
            with (
                self.subTest(actual=actual),
                patch.object(universal, "run", side_effect=["", actual]),
            ):
                with self.assertRaisesRegex(
                    universal.AssemblyError, "Expected exactly"
                ):
                    universal.verify_architectures(
                        self.arm64 / universal.EXECUTABLE, ("arm64",)
                    )

    def test_architecture_verification_keeps_variadic_option_last(self):
        binary = self.arm64 / universal.EXECUTABLE
        with patch.object(universal, "run", side_effect=["", "arm64"]) as run:
            universal.verify_architectures(binary, ("arm64",))
        self.assertEqual(
            run.call_args_list[0].args, ("lipo", binary, "-verify_arch", "arm64")
        )

    def test_subprocess_failures_and_timeouts_are_actionable_and_bounded(self):
        failures = (
            (subprocess.TimeoutExpired(["lipo"], 7), "timed out after 7s"),
            (
                subprocess.CalledProcessError(1, ["lipo"], stderr="bad binary"),
                "bad binary",
            ),
            (FileNotFoundError("missing tool"), "Cannot run lipo"),
        )
        for failure, message in failures:
            with (
                self.subTest(failure=failure),
                patch.object(universal.subprocess, "run", side_effect=failure) as run,
            ):
                with self.assertRaisesRegex(universal.AssemblyError, message):
                    universal.run("lipo", "fixture", timeout=7)
                self.assertEqual(run.call_args.kwargs["timeout"], 7)
                self.assertTrue(run.call_args.kwargs["check"])

    def test_nonmac_assembly_reports_platform_requirement(self):
        with patch.object(universal.sys, "platform", "linux"):
            with self.assertRaisesRegex(universal.AssemblyError, "requires macOS"):
                universal.assemble(self.arm64, self.intel, self.output)
        self.assertFalse(self.output.exists())

    def test_cli_reports_errors_without_tracebacks(self):
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            result = universal.main(
                [
                    "--arm64",
                    str(self.directory / "absent"),
                    "--x86-64",
                    str(self.intel),
                    "--output",
                    str(self.output),
                ]
            )
        self.assertEqual(result, 1)
        self.assertIn("Input app not found", stderr.getvalue())
        self.assertNotIn("Traceback", stderr.getvalue())
        self.assertFalse(self.output.exists())


def native_run(*args, source=None):
    try:
        return subprocess.run(
            [str(arg) for arg in args],
            input=source,
            text=True,
            check=True,
            capture_output=True,
            timeout=60,
        ).stdout.strip()
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f"{error.cmd}: {error.stderr}") from error


@unittest.skipUnless(
    sys.platform == "darwin", "requires real macOS clang/lipo/codesign/ditto fixtures"
)
class NativeAssemblyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        temporary = tempfile.TemporaryDirectory(prefix="universal linked fixtures ")
        cls.addClassCleanup(temporary.cleanup)
        cls.binaries = {}
        for arch in ("arm64", "x86_64"):
            binary = Path(temporary.name) / arch
            native_run(
                "xcrun",
                "clang",
                "-target",
                f"{arch}-apple-macos12.0",
                "-x",
                "c",
                "-o",
                binary,
                "-",
                source="int main(void) { return 0; }\n",
            )
            cls.binaries[arch] = binary.read_bytes()

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="universal native test ")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.arm64 = make_bundle(
            self.directory / "arm64/Felinaut.app", self.binaries["arm64"]
        )
        self.intel = make_bundle(
            self.directory / "x86_64/Felinaut.app", self.binaries["x86_64"], (0, 0, 255)
        )
        self.output = self.directory / "Felinaut-0.1.0-fixture-macos-universal.zip"
        for app in (self.arm64, self.intel):
            self.sign(app)
            universal.verify_signature(app)

    def sign(self, app):
        native_run("codesign", "--force", "--sign", "-", app)

    def assert_rejected(self, message=None):
        before = [snapshot(self.arm64), snapshot(self.intel)]
        with self.assertRaises(universal.AssemblyError) as caught:
            universal.assemble(self.arm64, self.intel, self.output)
        if message:
            self.assertIn(message, str(caught.exception))
        self.assertFalse(self.output.exists())
        self.assertEqual(before, [snapshot(self.arm64), snapshot(self.intel)])
        self.assertEqual(list(self.directory.glob(".felinaut-universal-*")), [])

    def test_end_to_end_zip_is_universal_signed_and_inputs_unchanged(self):
        before = [snapshot(self.arm64), snapshot(self.intel)]
        # Apple credentials/settings must not influence this ad-hoc-only path.
        with patch.dict(
            os.environ,
            {
                "FELINAUT_SIGN_IDENTITY": "must not be used",
                "FELINAUT_NOTARY_PROFILE": "must not be used",
                "FELINAUT_PROVISIONING_PROFILE": "/must/not/be/read",
            },
        ):
            result = universal.assemble(self.arm64, self.intel, self.output)
        self.assertEqual(result, self.output)
        self.assertTrue(self.output.is_file())
        extracted = self.directory / "extracted"
        native_run("ditto", "-x", "-k", self.output, extracted)
        app = extracted / "Felinaut.app"
        binary = app / universal.EXECUTABLE
        self.assertEqual(
            set(native_run("lipo", "-archs", binary).split()), {"arm64", "x86_64"}
        )
        for arch in ("arm64", "x86_64"):
            native_run("lipo", binary, "-verify_arch", arch)
        native_run(
            "codesign", "--verify", "--all-architectures", "--deep", "--strict", app
        )
        details = subprocess.run(
            ["codesign", "--display", "--verbose=4", str(app)],
            text=True,
            check=True,
            capture_output=True,
            timeout=30,
        )
        self.assertIn("Signature=adhoc", details.stderr)
        self.assertEqual(stat.S_IMODE(binary.stat().st_mode), 0o755)
        universal.validate_bundle(app, universal.expected_info())
        for resource in ("Felinaut.icns", "LICENSE"):
            relative = "Contents/Resources/" + resource
            self.assertEqual(
                (app / relative).read_bytes(), (self.arm64 / relative).read_bytes()
            )
        self.assertEqual(before, [snapshot(self.arm64), snapshot(self.intel)])
        self.assertEqual(list(self.directory.glob(".felinaut-universal-*")), [])

    def test_missing_executable_is_rejected(self):
        (self.arm64 / universal.EXECUTABLE).unlink()
        self.assert_rejected("Missing bundle entries")

    def test_wrong_architecture_is_rejected(self):
        (self.arm64 / universal.EXECUTABLE).write_bytes(self.binaries["x86_64"])
        self.sign(self.arm64)
        self.assert_rejected()

    def test_two_inputs_of_same_architecture_are_rejected(self):
        (self.intel / universal.EXECUTABLE).write_bytes(self.binaries["arm64"])
        self.sign(self.intel)
        self.assert_rejected()

    def test_already_universal_input_is_rejected(self):
        binary = self.directory / "fat"
        native_run(
            "lipo",
            "-create",
            self.arm64 / universal.EXECUTABLE,
            self.intel / universal.EXECUTABLE,
            "-output",
            binary,
        )
        shutil.copy2(binary, self.arm64 / universal.EXECUTABLE)
        (self.arm64 / universal.EXECUTABLE).chmod(0o755)
        self.sign(self.arm64)
        self.assert_rejected("Expected exactly arm64")

    def test_object_file_is_not_an_app_executable(self):
        binary = self.arm64 / universal.EXECUTABLE
        native_run(
            "xcrun",
            "clang",
            "-target",
            "arm64-apple-macos12.0",
            "-x",
            "c",
            "-c",
            "-o",
            binary,
            "-",
            source="int fixture(void) { return 0; }\n",
        )
        self.assert_rejected("Mach-O executable")

    def test_invalid_binary_is_rejected(self):
        (self.arm64 / universal.EXECUTABLE).write_bytes(b"not Mach-O")
        self.assert_rejected()

    def test_mismatched_metadata_is_rejected_even_when_signed(self):
        info = universal.expected_info()
        info["CFBundleGetInfoString"] = "different build"
        (self.intel / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
        self.sign(self.intel)
        self.assert_rejected("current checkout metadata")

    def test_tampered_resource_signature_is_rejected(self):
        (self.intel / "Contents/Resources/LICENSE").write_bytes(b"tampered resource")
        self.assert_rejected("codesign")

    def test_tampered_executable_signature_is_rejected(self):
        with (self.arm64 / universal.EXECUTABLE).open("r+b") as file:
            file.seek(512)
            byte = file.read(1)
            file.seek(512)
            file.write(bytes([byte[0] ^ 1]))
        self.assert_rejected("codesign")

    def test_unsigned_input_is_rejected(self):
        native_run("codesign", "--remove-signature", self.arm64 / universal.EXECUTABLE)
        self.assert_rejected()

    def test_extra_native_helper_is_rejected(self):
        (self.arm64 / "Contents/MacOS/helper").write_bytes(self.binaries["arm64"])
        self.assert_rejected("Unexpected bundle entry")

    def test_resource_symlink_is_rejected(self):
        resource = self.arm64 / "Contents/Resources/LICENSE"
        resource.unlink()
        resource.symlink_to(self.intel / "Contents/Resources/LICENSE")
        self.assert_rejected("Symlinks")

    def test_existing_output_is_preserved(self):
        self.output.write_bytes(b"previous release")
        with self.assertRaisesRegex(universal.AssemblyError, "already exists"):
            universal.assemble(self.arm64, self.intel, self.output)
        self.assertEqual(self.output.read_bytes(), b"previous release")

    def test_failed_resigning_or_archiving_never_publishes_output(self):
        original_run = universal.run
        for failing_tool in ("codesign", "ditto"):

            def fail(*args, **kwargs):
                if args[0] == failing_tool and (
                    failing_tool != "codesign" or "--sign" in args
                ):
                    if failing_tool == "ditto":
                        Path(args[-1]).write_bytes(b"partial ZIP")
                    raise universal.AssemblyError(f"injected {failing_tool} failure")
                return original_run(*args, **kwargs)

            with (
                self.subTest(tool=failing_tool),
                patch.object(universal, "run", side_effect=fail),
            ):
                self.assert_rejected(f"injected {failing_tool} failure")

    def test_cli_entry_point_produces_the_requested_zip(self):
        before = [snapshot(self.arm64), snapshot(self.intel)]
        result = native_run(
            sys.executable,
            universal.ROOT / "scripts/assemble_universal_macos.py",
            "--arm64",
            self.arm64,
            "--x86-64",
            self.intel,
            "--output",
            self.output,
        )
        self.assertIn(f"Universal app ZIP: {self.output}", result)
        self.assertIn("not Developer ID signed or Apple-notarized", result)
        self.assertTrue(self.output.is_file())
        self.assertEqual(before, [snapshot(self.arm64), snapshot(self.intel)])

    def test_nonexecutable_input_is_rejected(self):
        (self.arm64 / universal.EXECUTABLE).chmod(0o644)
        self.assert_rejected("no execute permission")

    def test_failed_staged_signature_validation_never_publishes_output(self):
        original_run = universal.run

        def tamper_after_signing(*args, **kwargs):
            result = original_run(*args, **kwargs)
            if args[0] == "codesign" and "--sign" in args:
                (Path(args[-1]) / "Contents/Resources/LICENSE").write_bytes(
                    b"tampered stage"
                )
            return result

        with patch.object(universal, "run", side_effect=tamper_after_signing):
            self.assert_rejected("codesign")

    def test_missing_or_empty_zip_never_publishes_output(self):
        original_run = universal.run
        for empty in (False, True):

            def omit_zip(*args, **kwargs):
                if args[0] == "ditto":
                    if empty:
                        Path(args[-1]).touch()
                    return ""
                return original_run(*args, **kwargs)

            with (
                self.subTest(empty=empty),
                patch.object(universal, "run", side_effect=omit_zip),
            ):
                self.assert_rejected("nonempty ZIP")

    def test_destination_created_during_assembly_is_not_overwritten(self):
        original_run = universal.run

        def race(*args, **kwargs):
            result = original_run(*args, **kwargs)
            if args[0] == "ditto":
                self.output.write_bytes(b"concurrent release")
            return result

        with patch.object(universal, "run", side_effect=race):
            with self.assertRaises(FileExistsError):
                universal.assemble(self.arm64, self.intel, self.output)
        self.assertEqual(self.output.read_bytes(), b"concurrent release")
        self.assertEqual(list(self.directory.glob(".felinaut-universal-*")), [])


if __name__ == "__main__":
    unittest.main()
