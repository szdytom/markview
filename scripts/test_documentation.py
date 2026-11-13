"""Check the documentation's local links and audience entry points."""

import re
import sys
import tomllib
import unittest
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
GUIDES = set((ROOT / "docs").glob("*/*.md")) | set(
    (ROOT / "docs/developers/history").glob("*.md")
)
PAGES = GUIDES | {ROOT / name for name in (
    "README.md", "README.zh-cn.md", "CONTRIBUTING.md", "docs/README.md",
    "android/README.md", "web/README.md", "fuzz/README.md",
    "tests/fixtures/fonts/README.md", "crates/markview-web/tests/fonts/README.md",
    ".agents/skills/mvss-theme/SKILL.md", ".agents/skills/markview-release/SKILL.md",
)}


ARTIFACT_PAGES = {ROOT / name for name in (
    "artifacts/wp-refactor-remaining-tasks.md",
    "artifacts/webpage-refactor/validation.md",
    "artifacts/android/R7.1-evidence.md",
)}
REQUIRE_ARTIFACTS = "--webpage-artifacts" in sys.argv
if REQUIRE_ARTIFACTS:
    sys.argv.remove("--webpage-artifacts")
PAGES |= ARTIFACT_PAGES if REQUIRE_ARTIFACTS else {p for p in ARTIFACT_PAGES if p.exists()}


def table_width(line):
    line = re.sub(r"(`+).*?\1", "code", line)
    line = line.replace(r"\|", "escaped")
    return len(line.strip().strip("|").split("|"))


def prose(path):
    return re.sub(r"^```[^\n]*\n.*?^```[^\n]*$", "", path.read_text(), flags=re.M | re.S)


def links(path):
    text = re.sub(r"`+[^`]+`+", "", prose(path))
    return re.findall(r"\]\(([^\s)]+)\)", text) + re.findall(
        r'(?:href|src)="([^"]+)"', text
    )


def anchors(path):
    result = set(re.findall(r'(?:id|name)="([^"]+)"', prose(path)))
    counts = {}
    for title in re.findall(r"^#{1,6}\s+(.+?)\s*#*\s*$", prose(path), re.M):
        title = re.sub(r"\[([^]]+)\]\([^)]+\)", r"\1", title)
        title = re.sub(r"<[^>]+>", "", title).lower()
        slug = re.sub(r"[^\w\s-]", "", title).replace(" ", "-")
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        result.add(slug + (f"-{count}" if count else ""))
    return result


class DocumentationTest(unittest.TestCase):
    def test_toml_examples_parse(self):
        for page in sorted(GUIDES):
            if "history" in page.parts:
                continue
            for number, snippet in enumerate(re.findall(
                r"^```toml\n(.*?)^```", page.read_text(), re.M | re.S
            ), 1):
                with self.subTest(page=str(page.relative_to(ROOT)), example=number):
                    tomllib.loads(snippet)

    def test_local_links_images_and_anchors(self):
        for page in sorted(PAGES):
            for link in links(page):
                url = urlsplit(link)
                if url.scheme or url.netloc:
                    continue
                target = (page.parent / unquote(url.path)).resolve() if url.path else page
                with self.subTest(page=str(page.relative_to(ROOT)), link=link):
                    self.assertTrue(target.exists(), f"Missing target: {target}")
                    if target.suffix == ".md" and url.fragment:
                        self.assertIn(unquote(url.fragment), anchors(target))

    def test_markdown_table_columns(self):
        for page in sorted(PAGES):
            lines = prose(page).splitlines()
            for index, line in enumerate(lines):
                if not re.fullmatch(r"\s*\|(?:[ :\-]+\|)+\s*", line):
                    continue
                width = table_width(line)
                first = max(0, index - 1)
                last = index + 1
                while last < len(lines) and lines[last].lstrip().startswith("|"):
                    last += 1
                for row in range(first, last):
                    with self.subTest(page=str(page.relative_to(ROOT)), line=row + 1):
                        self.assertEqual(table_width(lines[row]), width, "Table column count differs")

    def test_webpage_acceptance_functions_exist(self):
        page = ROOT / "artifacts/webpage-refactor/validation.md"
        if not page.exists():
            return
        for line in page.read_text().splitlines():
            if not re.match(r"\| A\d+ \|", line):
                continue
            path = None
            for token in re.findall(r"`([^`]+)`", line):
                if ".rs::" in token:
                    path, name = token.split("::", 1)
                elif token.startswith("::") and path:
                    name = token[2:]
                else:
                    continue
                with self.subTest(row=line.split("|")[1], path=path, function=name):
                    self.assertRegex((ROOT / path).read_text(), r"fn\s+" + re.escape(name.split("::")[-1]) + r"\s*\(")

    def test_audience_indexes_cover_all_guides(self):
        indexes = [ROOT / "docs" / audience / "README.md" for audience in (
            "users", "library", "developers",
        )]
        history = ROOT / "docs/developers/history/README.md"
        linked = {
            (page.parent / urlsplit(link).path).resolve()
            for page in [*indexes, history] for link in links(page)
            if not urlsplit(link).scheme
        }
        self.assertFalse(GUIDES - linked - set(indexes), "Guide missing from audience indexes")
        self.assertIn(history, {
            (indexes[2].parent / urlsplit(link).path).resolve()
            for link in links(indexes[2]) if not urlsplit(link).scheme
        }, "History index missing from developer navigation")
        for index in indexes:
            self.assertIn(index, {
                (ROOT / "docs" / urlsplit(link).path).resolve()
                for link in links(ROOT / "docs/README.md")
            })


if __name__ == "__main__":
    unittest.main()
