#!/usr/bin/env python3
"""Dependency-free release versions for Python 3.11+.

Cargo's canonical M.N.P is a patch floor. On the first-parent chain, the commit
introducing that version uses the floor; every subsequent commit adds one.
Only complete Git history may derive a release version. Packaging/source-archive
consumers instead use build_version(), which never invokes Git.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
U64_MAX = (1 << 64) - 1
GIT_TIMEOUT_SECONDS = 30
GH_API_TIMEOUT_SECONDS = 30
_MAX_TAG_DEPTH = 5
_OBJECT_SHA = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})")
_REPOSITORY = re.compile(
    r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?/[A-Za-z0-9_.-]{1,100}"
)
_VERSION = re.compile(
    r"(0|[1-9][0-9]{0,19})\.(0|[1-9][0-9]{0,19})\.(0|[1-9][0-9]{0,19})"
)


def parse_version(text: str) -> tuple[int, int, int]:
    """Parse only canonical ASCII M.N.P, with each component in 0..u64::MAX."""
    match = _VERSION.fullmatch(text) if isinstance(text, str) else None
    if match is None:
        raise ValueError(
            "Expected a canonical M.N.P version (no prefixes or whitespace)"
        )
    major, minor, patch = (int(part) for part in match.groups())
    if any(part > U64_MAX for part in (major, minor, patch)):
        raise ValueError("Version components must be in 0..u64::MAX")
    return major, minor, patch


def validate_override(base: str, value: str) -> str:
    """Accept a canonical release version on the base's major/minor patch floor."""
    base_major, base_minor, base_patch = parse_version(base)
    major, minor, patch = parse_version(value)
    if (major, minor) != (base_major, base_minor) or patch < base_patch:
        raise ValueError(
            "FELINAUT_RELEASE_VERSION must have the same major/minor as Cargo "
            "package.version and a patch at least as large as its base"
        )
    return value


def _manifest_version(text: str, source: str) -> str:
    try:
        version = tomllib.loads(text)["package"]["version"]
        parse_version(version)
    except (ValueError, KeyError, TypeError) as error:
        raise ValueError(
            f"Invalid Cargo package.version in {source}: {error}"
        ) from error
    return version


def _base_version(root: Path) -> str:
    manifest = root / "Cargo.toml"
    try:
        text = manifest.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise ValueError(f"Cannot read {manifest}: {error}") from error
    return _manifest_version(text, str(manifest))


def build_version(root: Path) -> str:
    """Use the validated env override, if present, otherwise Cargo; never use Git."""
    base = _base_version(root)
    if "FELINAUT_RELEASE_VERSION" in os.environ:
        return validate_override(base, os.environ["FELINAUT_RELEASE_VERSION"])
    return base


def _git(root: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", "--no-pager", "-C", str(root), *args],
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=GIT_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise ValueError(
            f"Git timed out after {GIT_TIMEOUT_SECONDS}s while reading release history"
        ) from error
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or "no diagnostic output").strip()
        raise ValueError(f"Cannot read Git release history: {detail}") from error
    except (OSError, UnicodeError) as error:
        raise ValueError(f"Cannot read Git release history: {error}") from error
    return result.stdout


def _historical_version(root: Path, commit: str) -> str | None:
    # Check the tree separately: a failed show is not evidence of a missing file.
    entry = _git(root, "ls-tree", "-z", commit, "--", "Cargo.toml")
    if not entry:
        return None
    if not entry.startswith(("100644 blob ", "100755 blob ")):
        raise ValueError(
            f"Invalid Cargo.toml at Git commit {commit}: not a regular file"
        )
    text = _git(root, "show", f"{commit}:Cargo.toml")
    return _manifest_version(text, f"Git commit {commit}:Cargo.toml")


def version_from_git(root: Path) -> str:
    """Derive BASE.patch + consecutive first-parent commits - 1, without writes.

    The root must be the Git worktree root (linked worktrees are supported).
    Uncommitted manifest cosmetics are allowed, but its version must match HEAD.
    The environment override is deliberately irrelevant to history derivation.
    """
    root = root.resolve()
    base = _base_version(root)
    top = Path(_git(root, "rev-parse", "--show-toplevel").strip()).resolve()
    if top != root:
        raise ValueError(
            "Release root must be the Git worktree root, not a source archive"
        )
    shallow = _git(root, "rev-parse", "--is-shallow-repository").strip()
    if shallow == "true":
        raise ValueError(
            "Cannot derive a release version from shallow Git history; "
            "checkout with fetch-depth: 0 (fetch-depth 0) or fetch the full history"
        )
    if shallow != "false":
        raise ValueError("Cannot determine whether Git release history is shallow")

    # Pin HEAD once so concurrent branch movement cannot mix commit histories.
    head = _git(root, "rev-parse", "--verify", "HEAD").strip()
    commits = _git(root, "rev-list", "--first-parent", head).splitlines()
    count = 0
    for commit in commits:
        historical = _historical_version(root, commit)
        if count == 0 and historical != base:
            raise ValueError(
                "Working-tree Cargo package.version does not match HEAD; "
                "commit the version change before deriving release metadata"
            )
        if historical is None or historical != base:
            break
        count += 1
    if count == 0:
        raise ValueError("Git HEAD has no Cargo package.version history")
    major, minor, patch = parse_version(base)
    increment = count - 1
    if increment > U64_MAX - patch:
        raise ValueError("Release patch overflow: first-parent count exceeds u64::MAX")
    return f"{major}.{minor}.{patch + increment}"


def _github_api(endpoint: str) -> object:
    try:
        result = subprocess.run(
            ["gh", "api", "--method", "GET", endpoint],
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=GH_API_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise ValueError(
            f"GitHub API timed out after {GH_API_TIMEOUT_SECONDS}s at {endpoint}"
        ) from error
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or "no diagnostic output").strip()
        raise ValueError(f"GitHub API failed at {endpoint}: {detail}") from error
    except (OSError, UnicodeError) as error:
        raise ValueError(f"Cannot read GitHub API at {endpoint}: {error}") from error
    try:
        return json.loads(result.stdout)
    except (ValueError, RecursionError) as error:
        raise ValueError(f"Malformed GitHub API JSON at {endpoint}") from error


def _release_tag_object(value: object) -> tuple[str, str]:
    if not isinstance(value, dict):
        raise ValueError("Malformed GitHub release tag object: expected an object")
    kind, sha = value.get("type"), value.get("sha")
    if kind not in ("commit", "tag"):
        raise ValueError(
            "Unexpected GitHub release tag object type; expected commit or tag"
        )
    if not isinstance(sha, str) or _OBJECT_SHA.fullmatch(sha) is None:
        raise ValueError("Malformed GitHub release tag object SHA")
    return kind, sha


def verify_release_tag(repository: str, tag: str, commit: str) -> None:
    """Allow an absent semantic tag, or prove its Git object resolves to commit.

    Uses read-only GitHub API requests via gh (and its caller-provided GH_TOKEN).
    Prefix matches are not tag existence proofs, nor is release.targetCommitish.
    Resolve at most five annotated tag objects; fail closed on API/schema errors.
    This preflight does not lock the remote ref against concurrent changes.
    """
    if (
        not isinstance(repository, str)
        or _REPOSITORY.fullmatch(repository) is None
        or repository.split("/")[1] in (".", "..")
    ):
        raise ValueError("Expected a safe GitHub repository owner/name")
    if not isinstance(tag, str) or not tag.startswith("v"):
        raise ValueError("Release tag must be canonical vM.N.P")
    try:
        parse_version(tag[1:])
    except ValueError as error:
        raise ValueError(f"Release tag must be canonical vM.N.P: {error}") from error
    if not isinstance(commit, str) or _OBJECT_SHA.fullmatch(commit) is None:
        raise ValueError(
            "Release commit must be 40 or 64 lowercase hexadecimal characters"
        )

    endpoint = f"repos/{repository}/git/matching-refs/tags/{tag}"
    refs = _github_api(endpoint)
    if not isinstance(refs, list):
        raise ValueError("Malformed GitHub matching-refs response: expected a list")
    exact = []
    for ref in refs:
        if (
            not isinstance(ref, dict)
            or not isinstance(ref.get("ref"), str)
            or not ref["ref"].startswith("refs/tags/")
        ):
            raise ValueError("Malformed GitHub matching-refs entry")
        if ref["ref"] == f"refs/tags/{tag}":
            exact.append(_release_tag_object(ref.get("object")))
    if not exact:
        return
    if len(exact) != 1:
        raise ValueError("Malformed GitHub matching-refs response: duplicate exact tag")

    kind, sha = exact[0]
    seen = set()
    while kind == "tag":
        if sha in seen:
            raise ValueError("Cycle in annotated GitHub release tag objects")
        if len(seen) >= _MAX_TAG_DEPTH:
            raise ValueError("Annotated GitHub release tag exceeds maximum depth of 5")
        seen.add(sha)
        value = _github_api(f"repos/{repository}/git/tags/{sha}")
        if not isinstance(value, dict):
            raise ValueError(
                "Malformed GitHub annotated tag response: expected an object"
            )
        if "sha" in value and value["sha"] != sha:
            raise ValueError(
                "GitHub annotated tag response SHA does not match requested object"
            )
        kind, sha = _release_tag_object(value.get("object"))
    if sha != commit:
        raise ValueError(
            f"Existing release tag {tag} points to {sha}, not commit {commit}"
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help="Git worktree root")
    parser.add_argument(
        "--github-output", type=Path, help="Append version and tag to this output file"
    )
    args = parser.parse_args(argv)
    try:
        version = version_from_git(args.root)
        if args.github_output is not None:
            with args.github_output.open("a", encoding="utf-8", newline="\n") as output:
                output.write(f"version={version}\ntag=v{version}\n")
    except (ValueError, OSError) as error:
        print(f"Release version error: {error}", file=sys.stderr)
        return 1
    print(version)
    return 0


if __name__ == "__main__":
    sys.exit(main())
