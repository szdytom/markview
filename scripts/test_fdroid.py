"""Validate F-Droid store assets and automatic version detection."""

from html.parser import HTMLParser
from pathlib import Path
import re
import struct
import tomllib
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
METADATA = ROOT / "fastlane/metadata/android"
RECIPE = ROOT / "packaging/fdroid/io.github.szdytom.markview.yml"
ASSETS = {
    "en-US/images/icon.png": "assets/icons/markview-512.png",
    "zh-CN/images/icon.png": "assets/icons/markview-512.png",
    "en-US/images/phoneScreenshots/1.png": "docs/screenshots/readme/android-en.png",
    "zh-CN/images/phoneScreenshots/1.png": "docs/screenshots/readme/android-zh.png",
    "en-US/images/phoneScreenshots/2.png": "docs/screenshots/android/settings.png",
    "en-US/images/phoneScreenshots/3.png": "docs/screenshots/android/fonts.png",
    "en-US/images/tenInchScreenshots/1.png": "docs/screenshots/android/tablet-tabs.png",
}


class DescriptionParser(HTMLParser):
    def handle_starttag(self, tag, attrs):
        if tag not in {"ul", "li", "b", "i", "em", "strong", "br"} or attrs:
            raise ValueError(f"Unsupported description markup: {tag} {attrs}")


class FdroidMetadataTest(unittest.TestCase):
    def test_localized_store_text_and_current_release_notes(self):
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]
        code = workspace["metadata"]["android"]["version-code"]
        for locale in ("en-US", "zh-CN"):
            for name, limit in (("title.txt", 50), ("short_description.txt", 79),
                                ("full_description.txt", 4000), (f"changelogs/{code}.txt", 500)):
                with self.subTest(locale=locale, file=name):
                    text = (METADATA / locale / name).read_text().strip()
                    self.assertTrue(text)
                    self.assertLessEqual(len(text), limit)
                    self.assertNotIn("\r", text)
                    if name == "short_description.txt":
                        self.assertNotIn("\n", text)
                        self.assertFalse(text.endswith((".", "。")))
                    if name == "full_description.txt":
                        DescriptionParser().feed(text)

    def test_images_are_valid_sized_copies_of_documented_sources(self):
        actual = {str(path.relative_to(METADATA)) for path in METADATA.rglob("*.png")}
        self.assertEqual(actual, set(ASSETS))
        for name, source in ASSETS.items():
            with self.subTest(image=name):
                data = (METADATA / name).read_bytes()
                self.assertEqual(data, (ROOT / source).read_bytes())
                self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
                self.assertEqual(data[12:16], b"IHDR")
                width, height = struct.unpack(">II", data[16:24])
                self.assertGreaterEqual(min(width, height), 320)
                self.assertLessEqual(max(width, height), 3840)
                if name.endswith("/icon.png"):
                    self.assertEqual((width, height), (512, 512))

    def test_update_patterns_extract_version_and_code_without_building(self):
        recipe = RECIPE.read_text()
        update = next(line.split(": ", 1)[1].strip("'") for line in recipe.splitlines()
                      if line.startswith("UpdateCheckData: "))
        code_path, code_regex, version_path, version_regex = update.split("|")
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]
        for path, pattern, expected in (
            (code_path, code_regex, str(workspace["metadata"]["android"]["version-code"])),
            (version_path, version_regex, workspace["package"]["version"]),
        ):
            content = (ROOT / path).read_text() if path else "v" + workspace["package"]["version"]
            self.assertEqual(re.findall(pattern, content), [expected])
        tags = next(line.removeprefix("UpdateCheckMode: Tags ") for line in recipe.splitlines()
                    if line.startswith("UpdateCheckMode: Tags "))
        for tag in ("v0.5.2", "v1.10.123"):
            self.assertIsNotNone(re.fullmatch(tags, tag))
        for tag in ("v0.5.3-beta.1", "v0.5.3-rc.1", "main", "0.5.3"):
            self.assertIsNone(re.fullmatch(tags, tag))

    def test_recipe_uses_pinned_tools_and_f_droid_signing(self):
        recipe = RECIPE.read_text()
        android = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["metadata"]["android"]
        self.assertIn(f"rustup toolchain install {android['rust-version']} ", recipe)
        self.assertIn(f"cargo-about@{android['cargo-about']} --locked", recipe)
        self.assertIn(f"    ndk: {android['ndk']}\n", recipe)
        self.assertIn(f"'build-tools;{android['build-tools']}'", recipe)
        sdk = ET.parse(ROOT / "android/AndroidManifest.xml").getroot().find("uses-sdk")
        target = sdk.get("{http://schemas.android.com/apk/res/android}targetSdkVersion")
        self.assertIn(f"'platforms;android-{target}'", recipe)
        self.assertIn("python3 android/build.py --release --unsigned --abi arm64-v8a", recipe)
        self.assertIn("output: target/android/markview-android-release-unsigned.apk", recipe)
        self.assertNotIn("Binaries:", recipe)
        self.assertNotIn("AllowedAPKSigningKeys:", recipe)


if __name__ == "__main__":
    unittest.main()
