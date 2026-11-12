#!/usr/bin/env python3
"""Combine two freshly built CI apps into one ad-hoc-signed universal macOS ZIP.

Requires macOS, Python 3.11+, lipo, codesign and ditto. This does not build apps,
use Apple credentials, or notarize. Both inputs must come from the same validated
CI run; metadata checks cannot establish build provenance on their own.
"""

from __future__ import annotations

import argparse
import os
import plistlib
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
from pathlib import Path
from xml.parsers.expat import ExpatError

import package_macos as package
from release_version import build_version

ROOT = Path(__file__).resolve().parent.parent
EXECUTABLE = Path("Contents/MacOS/felinaut")
DIRECTORIES = {
    "Contents",
    "Contents/MacOS",
    "Contents/Resources",
    "Contents/_CodeSignature",
}
FILES = {
    EXECUTABLE.as_posix(),
    "Contents/Info.plist",
    "Contents/PkgInfo",
    "Contents/Resources/Felinaut.icns",
    "Contents/Resources/LICENSE",
    "Contents/_CodeSignature/CodeResources",
}
MACHO_MAGICS = {
    b"\xfe\xed\xfa\xce",
    b"\xce\xfa\xed\xfe",
    b"\xfe\xed\xfa\xcf",
    b"\xcf\xfa\xed\xfe",
    b"\xca\xfe\xba\xbe",
    b"\xbe\xba\xfe\xca",
    b"\xca\xfe\xba\xbf",
    b"\xbf\xba\xfe\xca",
}


class AssemblyError(RuntimeError):
    """Invalid input or a failed native packaging operation."""


def run(*args: str | Path, timeout: int = 120) -> str:
    command = [str(arg) for arg in args]
    try:
        result = subprocess.run(
            command, check=True, capture_output=True, text=True, timeout=timeout
        )
    except subprocess.TimeoutExpired as error:
        raise AssemblyError(f"{command[0]} timed out after {timeout}s") from error
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or "no diagnostic output").strip()
        raise AssemblyError(f"{' '.join(command)} failed: {detail}") from error
    except OSError as error:
        raise AssemblyError(f"Cannot run {command[0]}: {error}") from error
    return result.stdout.strip()


def expected_info() -> dict:
    version = build_version(ROOT)
    display = package.display_version((ROOT / "src/main.rs").read_text())
    return package.info_plist(version, display)


def validate_bundle(app: Path, expected: dict) -> dict:
    """Accept only the current flat bundle, never nested code or symlinks."""
    if app.is_symlink() or not app.is_dir() or app.name != "Felinaut.app":
        raise AssemblyError(f"Expected a non-symlink Felinaut.app directory: {app}")
    found = set()
    for parent, directories, files in os.walk(app, followlinks=False):
        for name in directories + files:
            path = Path(parent) / name
            relative = path.relative_to(app).as_posix()
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode):
                raise AssemblyError(f"Symlinks are not allowed in app bundles: {path}")
            if stat.S_ISDIR(mode) and relative in DIRECTORIES:
                pass
            elif stat.S_ISREG(mode) and relative in FILES:
                if relative != EXECUTABLE.as_posix():
                    with path.open("rb") as file:
                        if file.read(4) in MACHO_MAGICS:
                            raise AssemblyError(f"Unexpected bundled Mach-O: {path}")
            else:
                raise AssemblyError(f"Unexpected bundle entry or file type: {path}")
            found.add(relative)
    missing = (DIRECTORIES | FILES) - found
    if missing:
        raise AssemblyError(
            f"Missing bundle entries in {app}: {', '.join(sorted(missing))}"
        )
    if (app / "Contents/PkgInfo").read_bytes() != b"APPL????":
        raise AssemblyError(f"Invalid PkgInfo in {app}")
    try:
        info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    except (
        ValueError,
        plistlib.InvalidFileException,
        OverflowError,
        ExpatError,
    ) as error:
        raise AssemblyError(f"Invalid Info.plist in {app}: {error}") from error
    if info != expected:
        raise AssemblyError(
            f"Info.plist in {app} does not match current checkout metadata"
        )
    return info


def verify_architectures(binary: Path, expected: tuple[str, ...]) -> None:
    # -verify_arch is variadic: the file must precede it, even on older Xcode.
    for arch in expected:
        run("lipo", binary, "-verify_arch", arch)
    actual = run("lipo", "-archs", binary).split()
    if sorted(actual) != sorted(expected):
        raise AssemblyError(
            f"Expected exactly {'+'.join(expected)} in {binary}; "
            f"found {'+'.join(actual) or 'no architectures'}"
        )


def verify_thin_executable(binary: Path, arch: str) -> None:
    verify_architectures(binary, (arch,))
    with binary.open("rb") as file:
        header = file.read(16)
    endian = {b"\xcf\xfa\xed\xfe": "<", b"\xfe\xed\xfa\xcf": ">"}.get(header[:4])
    # lipo also accepts object files and static libraries; neither is an app.
    if (
        endian is None
        or len(header) != 16
        or struct.unpack(endian + "I", header[12:])[0] != 2
    ):
        raise AssemblyError(f"Expected a thin 64-bit Mach-O executable: {binary}")
    if not binary.stat().st_mode & 0o111:
        raise AssemblyError(f"Input executable has no execute permission: {binary}")


def verify_signature(app: Path) -> None:
    run("codesign", "--verify", "--all-architectures", "--deep", "--strict", app)


def validate_paths(arm64: Path, x86_64: Path, output: Path) -> None:
    if output.exists() or output.is_symlink():
        raise AssemblyError(f"Output already exists (will not overwrite): {output}")
    if output.suffix != ".zip" or not output.parent.is_dir():
        raise AssemblyError(f"Output must be a .zip in an existing directory: {output}")
    for app in (arm64, x86_64):
        if not app.exists():
            raise AssemblyError(f"Input app not found: {app}")
        if output.parent.resolve().is_relative_to(app.resolve()):
            raise AssemblyError(f"Output must be outside input app: {app}")
    if arm64.samefile(x86_64):
        raise AssemblyError("arm64 and x86_64 inputs must be different app bundles")


def assemble(arm64: Path, x86_64: Path, output: Path) -> Path:
    """Validate native CI apps, merge in private staging, and publish without clobbering."""
    arm64, x86_64, output = (Path(path).absolute() for path in (arm64, x86_64, output))
    validate_paths(arm64, x86_64, output)
    if sys.platform != "darwin":
        raise AssemblyError(
            "Universal app assembly requires macOS (lipo, codesign, ditto)"
        )
    expected = expected_info()
    for app, arch in ((arm64, "arm64"), (x86_64, "x86_64")):
        validate_bundle(app, expected)
        verify_thin_executable(app / EXECUTABLE, arch)
        verify_signature(app)

    # Staging beside the destination keeps publication on the same filesystem.
    with tempfile.TemporaryDirectory(
        prefix=".felinaut-universal-", dir=output.parent
    ) as temporary:
        stage = Path(temporary)
        app = stage / "Felinaut.app"
        shutil.copytree(arm64, app, symlinks=True)
        shutil.rmtree(app / "Contents/_CodeSignature")
        binary = app / EXECUTABLE
        binary.unlink()
        run(
            "lipo",
            "-create",
            arm64 / EXECUTABLE,
            x86_64 / EXECUTABLE,
            "-output",
            binary,
        )
        binary.chmod(0o755)
        verify_architectures(binary, ("arm64", "x86_64"))
        # Never inherit Developer ID, provisioning, or notarization configuration.
        run("codesign", "--force", "--sign", "-", app)
        validate_bundle(app, expected)
        verify_signature(app)
        archive = stage / "universal.zip"
        run(
            "ditto",
            "-c",
            "-k",
            "--sequesterRsrc",
            "--keepParent",
            app,
            archive,
            timeout=300,
        )
        if not archive.is_file() or not archive.stat().st_size:
            raise AssemblyError("ditto did not produce a nonempty ZIP")
        # An atomic, exclusive link also refuses a destination created mid-assembly.
        os.link(archive, output)
    return output


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--arm64", type=Path, required=True, help="fresh arm64 Felinaut.app"
    )
    parser.add_argument(
        "--x86-64", type=Path, required=True, help="fresh x86_64 Felinaut.app"
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="new universal .zip in an existing directory",
    )
    args = parser.parse_args(argv)
    try:
        output = assemble(args.arm64, args.x86_64, args.output)
    except (AssemblyError, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"Universal app ZIP: {output}")
    print("Ad-hoc signature only; not Developer ID signed or Apple-notarized.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
