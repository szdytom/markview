#!/usr/bin/env python3
"""Check changelog formatting and protect entries published in the latest release."""

import argparse
import re
import subprocess
import sys
from datetime import date
from difflib import SequenceMatcher
from pathlib import Path

CHANGELOG = Path(__file__).resolve().parents[1] / "CHANGELOG.md"
CATEGORIES = {
    "Added",
    "Changed",
    "Deprecated",
    "Removed",
    "Fixed",
    "Security",
    "Documentation",
}
NUMBER = r"(?:0|[1-9][0-9]*)"
IDENTIFIER = rf"(?:{NUMBER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION = (
    rf"{NUMBER}\.{NUMBER}\.{NUMBER}"
    rf"(?:-{IDENTIFIER}(?:\.{IDENTIFIER})*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)
RELEASE = re.compile(rf"## ({VERSION}) - ([0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}})")


def check(text):
    errors = []
    headings = []
    versions = set()
    lines = text.splitlines()
    for number, line in enumerate(lines, 1):
        if not re.match(r"^##(?:\s|$)", line):
            continue
        headings.append((number, line))
        if line == "## Unreleased":
            continue
        release = RELEASE.fullmatch(line)
        if not release:
            errors.append(
                (
                    number,
                    "expected '## VERSION - YYYY-MM-DD' with an unbracketed SemVer version",
                )
            )
            continue
        version, released = release.groups()
        if version in versions:
            errors.append((number, f"duplicate release version: {version}"))
        versions.add(version)
        try:
            date.fromisoformat(released)
        except ValueError:
            errors.append((number, f"invalid release date: {released}"))

    unreleased = [number for number, line in headings if line == "## Unreleased"]
    if len(unreleased) != 1:
        errors.append(
            (
                unreleased[-1] if unreleased else 1,
                "expected exactly one '## Unreleased' section",
            )
        )
    if headings and headings[0][1] != "## Unreleased":
        errors.append((headings[0][0], "Unreleased must be the first release section"))

    for start in unreleased:
        end = next((number for number, _ in headings if number > start), len(lines) + 1)
        categories = set()
        category = None
        previous_entry = None
        for number in range(start + 1, end):
            line = lines[number - 1]
            if line != line.rstrip():
                errors.append((number, "trailing whitespace is not allowed"))
            if not line.strip():
                continue
            if line.startswith("### "):
                category = line.removeprefix("### ")
                if category not in CATEGORIES:
                    errors.append((number, f"unknown category: {category}"))
                if category in categories:
                    errors.append((number, f"duplicate category: {category}"))
                categories.add(category)
                previous_entry = None
            elif line.startswith("- ") and line[2:].strip():
                if category is None:
                    errors.append((number, "entry must be under a category heading"))
                if previous_entry is not None and number != previous_entry + 1:
                    errors.append(
                        (
                            number,
                            "remove blank lines between entries to keep the list dense",
                        )
                    )
                if line.endswith("\\"):
                    errors.append(
                        (number, "Markdown line breaks are not allowed in entries")
                    )
                previous_entry = number
            else:
                errors.append(
                    (
                        number,
                        "expected '### CATEGORY' or a single-line '- ' entry; do not wrap entries",
                    )
                )
    return errors


def git(directory, *args):
    return subprocess.run(
        ["git", "-C", str(directory), *args],
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def published(text, baseline, start):
    lines = text.splitlines(keepends=True)
    heading = baseline[start].rstrip("\r\n")
    for index, line in enumerate(lines):
        if line.rstrip("\r\n") == heading:
            return "".join(lines[index:])
    # Follow the boundary when a dedicated correction renames or removes a heading.
    matcher = SequenceMatcher(None, baseline, lines, autojunk=False)
    for operation, old_start, old_end, new_start, _ in matcher.get_opcodes():
        if old_start <= start < old_end:
            offset = start - old_start if operation == "equal" else 0
            return "".join(lines[new_start + offset:])
    return ""


def check_history(path, text):
    try:
        root = Path(git(path.parent, "rev-parse", "--show-toplevel").strip())
    except subprocess.CalledProcessError:
        return []  # Standalone files only receive formatting checks.
    relative = path.relative_to(root).as_posix()
    if git(root, "rev-parse", "--is-shallow-repository").strip() == "true":
        return [(1, "history checks require a full clone with release tags (fetch-depth: 0)")]
    tags = [
        tag
        for tag in git(root, "tag", "--merged", "HEAD").splitlines()
        if re.fullmatch(rf"v{VERSION}", tag)
    ]
    if not tags:
        return []  # There is no published release to protect yet.
    tag = git(
        root,
        "describe",
        "--tags",
        "--abbrev=0",
        *(argument for tag in tags for argument in ("--match", tag)),
        "HEAD",
    ).strip()
    if not git(root, "ls-tree", "--name-only", tag, "--", relative).strip():
        return []
    baseline = git(root, "show", f"{tag}:{relative}")
    baseline = baseline.splitlines(keepends=True)
    start = next(
        (
            index
            for index, line in enumerate(baseline)
            if RELEASE.fullmatch(line.rstrip("\r\n"))
        ),
        None,
    )
    if start is None:
        return []
    errors = []
    commits = git(
        root, "rev-list", "--reverse", "--full-history", f"{tag}..HEAD", "--", relative
    )
    for commit in commits.splitlines():
        parents = git(root, "rev-list", "--parents", "-n", "1", commit).split()[1:]
        for parent in parents:
            before = git(root, "show", f"{parent}:{relative}")
            after = git(root, "show", f"{commit}:{relative}")
            if published(before, baseline, start) == published(after, baseline, start):
                continue
            files = git(
                root, "diff", "--no-renames", "--name-only", "-z", parent, commit
            ).strip("\0").split("\0")
            if files != [relative]:
                errors.append(
                    (
                        1,
                        f"{commit[:12]} changes entries published in {tag}; "
                        "such commits must change only the changelog file",
                    )
                )
                break
    head = git(root, "show", f"HEAD:{relative}")
    if published(head, baseline, start) != published(text, baseline, start):
        errors.append(
            (
                1,
                f"uncommitted changes modify entries published in {tag}; "
                "commit these corrections separately, changing only the changelog file",
            )
        )
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=CHANGELOG)
    path = parser.parse_args().path.resolve()
    try:
        text = path.read_text(encoding="utf-8")
        errors = check(text)
        errors.extend(check_history(path, text))
    except (OSError, UnicodeError, subprocess.CalledProcessError) as error:
        print(f"{path}: {error}", file=sys.stderr)
        return 1
    for number, message in errors:
        print(f"{path}:{number}: {message}", file=sys.stderr)
    if errors:
        return 1
    print(f"{path}: changelog checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
