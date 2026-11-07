"""Exercise the changelog checker through its command-line interface."""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check_changelog.py")
VALID = """# Changelog

## Unreleased

### Added

- Add a feature.
- Add another feature.

### Fixed

- Fix a bug.

## 0.2.0-beta.1+build.2 - 2026-10-06

Historical prose and wrapped entries keep their format.

### Added

- An old entry that
  spans two lines.

- Another old entry.
""".replace("  spans two lines.", "  spans two lines.  ")


class ChangelogTest(unittest.TestCase):
    def run_checker(self, text):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "CHANGELOG.md"
            path.write_text(text, encoding="utf-8")
            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(path)],
                capture_output=True,
                text=True,
                check=False,
                cwd=directory,
            )
        return result

    def test_valid_changelog_and_historical_format(self):
        for text in (
            VALID,
            VALID.replace("- Add a feature.\n", "- 支持中文记录。\n"),
            "# Changelog\n\n## Unreleased\n",
        ):
            with self.subTest(text=text):
                result = self.run_checker(text)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("checks passed", result.stdout)
                self.assertEqual(result.stderr, "")

    def test_unreleased_format_errors_report_the_line(self):
        cases = [
            ("- Add a feature.\n  Wrapped prose.", "do not wrap entries", 8),
            ("- Add a feature.\n\n", "keep the list dense", 10),
            ("- Add a feature.  ", "trailing whitespace", 7),
            ("- Add a feature.\\", "line breaks are not allowed", 7),
            ("- ", "single-line '- ' entry", 7),
            ("* Add a feature.", "single-line '- ' entry", 7),
            ("#### Nested category", "do not wrap entries", 7),
        ]
        for replacement, message, number in cases:
            with self.subTest(replacement=replacement):
                result = self.run_checker(
                    VALID.replace("- Add a feature.", replacement)
                )
                self.assertEqual(result.returncode, 1)
                self.assertIn(f"CHANGELOG.md:{number}:", result.stderr)
                self.assertIn(message, result.stderr)
                self.assertEqual(result.stdout, "")

    def test_sections_and_release_headers(self):
        cases = [
            (VALID.replace("## Unreleased", "## 0.3.0 - 2026-10-09"), "exactly one"),
            (VALID + "\n## Unreleased\n", "exactly one"),
            ("## 0.3.0 - 2026-10-09\n\n" + VALID, "must be the first"),
            (VALID.replace("### Fixed", "### Added"), "duplicate category"),
            (VALID.replace("### Added", "### Features", 1), "unknown category"),
            (VALID.replace("### Added\n\n", "", 1), "under a category"),
            (VALID.replace("2026-10-06", "2026-02-30"), "invalid release date"),
            (VALID + "\n## 0.2.0-beta.1+build.2 - 2026-10-07\n", "duplicate release"),
        ]
        for version in ("[0.2.0]", "v0.2.0", "0.2", "00.2.0", "0.2.0-beta.01"):
            cases.append(
                (VALID.replace("0.2.0-beta.1+build.2", version), "unbracketed SemVer")
            )
        for text, message in cases:
            with self.subTest(message=message, text=text):
                result = self.run_checker(text)
                self.assertEqual(result.returncode, 1)
                self.assertIn(message, result.stderr)

    def test_repository_changelog_from_another_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(
                [sys.executable, str(SCRIPT)],
                capture_output=True,
                text=True,
                check=False,
                cwd=directory,
            )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "missing.md"
            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(path)],
                capture_output=True,
                text=True,
                check=False,
            )
        self.assertEqual(result.returncode, 1)
        self.assertIn(str(path), result.stderr)
        self.assertNotIn("Traceback", result.stderr)


class ChangelogHistoryTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.path = self.root / "CHANGELOG.md"
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Changelog Test")
        self.git("config", "user.email", "changelog@example.com")
        self.git("config", "commit.gpgsign", "false")
        self.path.write_text(
            VALID + "\n## 0.1.0 - 2026-09-01\n\n### Fixed\n\n- An older fix.\n",
            encoding="utf-8",
        )
        self.commit()
        self.git("tag", "-a", "v0.2.0-beta.1+build.2", "-m", "release")

    def git(self, *args):
        return subprocess.run(
            ["git", "-C", str(self.root), *args],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()

    def commit(self):
        self.git("add", "--all")
        self.git("commit", "-m", "test: update fixture")
        return self.git("rev-parse", "HEAD")[:12]

    def run_checker(self):
        return subprocess.run(
            [sys.executable, str(SCRIPT), str(self.path)],
            cwd=self.root,
            capture_output=True,
            text=True,
        )

    def edit(self, before, after):
        self.path.write_text(
            self.path.read_text(encoding="utf-8").replace(before, after),
            encoding="utf-8",
        )

    def test_unreleased_and_new_release_changes_are_allowed(self):
        self.edit("Add a feature.", "Improve the feature.")
        (self.root / "code.txt").write_text("code")
        self.assertEqual(self.run_checker().returncode, 0)
        self.commit()
        self.edit("## Unreleased", "## Unreleased\n\n## 0.3.0 - 2026-11-07")
        (self.root / "code.txt").write_text("release")
        self.commit()
        result = self.run_checker()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_published_changes_require_changelog_only_commits(self):
        self.edit("Another old entry.", "Corrected old entry.")
        result = self.run_checker()
        self.assertEqual(result.returncode, 1)
        self.assertIn("uncommitted changes", result.stderr)
        self.commit()
        self.assertEqual(self.run_checker().returncode, 0)
        self.edit("Add a feature.", "Improve the feature.")
        (self.root / "code.txt").write_text("code")
        self.commit()
        result = self.run_checker()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_published_heading_correction_preserves_the_escape_hatch(self):
        self.edit("2026-10-06", "2026-10-07")
        self.commit()
        self.edit("Add a feature.", "Improve the feature.")
        (self.root / "code.txt").write_text("code")
        self.commit()
        result = self.run_checker()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.edit("Another old entry.", "Changed old entry.")
        (self.root / "code.txt").write_text("changed code")
        offending = self.commit()
        result = self.run_checker()
        self.assertEqual(result.returncode, 1)
        self.assertIn(offending, result.stderr)

    def test_mixed_commit_cannot_be_hidden_by_later_correction(self):
        self.edit("Another old entry.", "Changed old entry.")
        (self.root / "code.txt").write_text("code")
        offending = self.commit()
        self.edit("Changed old entry.", "Another old entry.")
        self.commit()
        result = self.run_checker()
        self.assertEqual(result.returncode, 1)
        self.assertIn(offending, result.stderr)
        self.assertIn("must change only the changelog", result.stderr)

    def test_all_historical_sections_and_headings_are_protected(self):
        for before, after in (
            ("## 0.2.0-beta.1+build.2 - 2026-10-06", "## 0.2.0 - 2026-10-07"),
            ("Another old entry.", ""),
            ("An older fix.", "Changed older fix."),
        ):
            with self.subTest(before=before):
                self.edit(before, after)
                (self.root / "code.txt").write_text(after)
                offending = self.commit()
                result = self.run_checker()
                self.assertEqual(result.returncode, 1)
                self.assertIn(offending, result.stderr)
                self.git("reset", "--hard", "v0.2.0-beta.1+build.2")

    def test_latest_reachable_release_is_used_and_other_tags_are_ignored(self):
        self.edit("## Unreleased", "## Unreleased\n\n## 0.3.0 - 2026-11-07")
        self.commit()
        self.git("tag", "v0.3.0")
        self.git("tag", "unrelated-tag")
        self.git("checkout", "-b", "other-release")
        self.git("commit", "--allow-empty", "-m", "test: unrelated release")
        self.git("tag", "v9.0.0")
        self.git("checkout", "main")
        self.edit("Add a feature.", "Change a now-published feature.")
        (self.root / "code.txt").write_text("code")
        self.commit()
        result = self.run_checker()
        self.assertEqual(result.returncode, 1)
        self.assertIn("published in v0.3.0", result.stderr)

    def test_merge_cannot_hide_mixed_changes(self):
        self.git("checkout", "-b", "feature")
        self.edit("Another old entry.", "Changed old entry.")
        (self.root / "code.txt").write_text("code")
        offending = self.commit()
        self.git("checkout", "main")
        self.git("merge", "--no-ff", "feature", "-m", "test: merge feature")
        result = self.run_checker()
        self.assertEqual(result.returncode, 1)
        self.assertIn(offending, result.stderr)

    def test_no_release_yet_and_shallow_clone(self):
        self.git("tag", "-d", "v0.2.0-beta.1+build.2")
        self.assertEqual(self.run_checker().returncode, 0)
        self.git("tag", "v0.2.0")
        with tempfile.TemporaryDirectory() as directory:
            self.git("clone", "--depth=1", self.root.as_uri(), directory)
            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(Path(directory) / "CHANGELOG.md")],
                capture_output=True,
                text=True,
            )
        self.assertEqual(result.returncode, 1)
        self.assertIn("full clone", result.stderr)


if __name__ == "__main__":
    unittest.main()
