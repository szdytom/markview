"""Exercise the release-notes command with cargo-dist and GitHub inputs."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("render_release_notes.py")


class ReleaseNotesTest(unittest.TestCase):
    def run_renderer(self, version, missing=None, body=None):
        base = f"https://github.com/szdytom/markview/releases/download/v{version}"
        plan = {
            "releases": [
                {
                    "app_name": "markview",
                    "app_version": version,
                    "hosting": {
                        "github": {
                            "artifact_base_url": "https://github.com",
                            "artifact_download_path": f"/szdytom/markview/releases/download/v{version}",
                        }
                    },
                }
            ]
        }
        names = [
            f"markview-{version}-aarch64.app.zip",
            "markview-macos-SHA256SUMS",
            "markview-x86_64-pc-windows-msvc.msi",
            "markview-x86_64-pc-windows-msvc.msi.sha256",
            "markview-x86_64-pc-windows-msvc.zip",
            "markview-x86_64-pc-windows-msvc.zip.sha256",
            f"markview-{version}-x86_64.AppImage",
            "markview-linux-SHA256SUMS",
        ]
        prefix = "## Release Notes\n\nChanges.\n\n## Install markview\n\n```sh\ninstall\n```\n\n"
        suffix = "## Verifying GitHub Artifact Attestations\n\nVerification.\n"
        release = {
            "assets": [{"name": name} for name in names if name != missing],
            "body": prefix + f"## Download markview {version}\n\n"
            "| File | Platform | Checksum |\n|---|---|---|\n"
            "| old.tar.gz | old | old |\n\n" + suffix,
        }
        if body is not None:
            release["body"] = body
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory) / name for name in ("plan.json", "release.json")]
            for path, data in zip(paths, (plan, release)):
                path.write_text(json.dumps(data))
            result = subprocess.run(
                [sys.executable, str(SCRIPT), *map(str, paths)],
                capture_output=True,
                text=True,
                check=False,
            )
        return result, names, base, prefix, suffix

    def test_desktop_downloads_preserve_other_sections(self):
        for version in ("0.1.10", "0.2.0-beta.1"):
            with self.subTest(version=version):
                result, names, base, prefix, suffix = self.run_renderer(version)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(result.stdout.startswith(prefix))
                self.assertTrue(result.stdout.endswith(suffix))
                rows = [
                    line
                    for line in result.stdout.splitlines()
                    if line.startswith("| [")
                ]
                self.assertEqual(len(rows), 4)
                for row, name, platform, checksum in zip(
                    rows,
                    names[::2],
                    (
                        "Apple Silicon macOS",
                        "x64 Windows",
                        "x64 Windows (Portable)",
                        "x64 Linux",
                    ),
                    names[1::2],
                ):
                    self.assertEqual(
                        row,
                        f"| [{name}]({base}/{name}) | {platform} | [checksum]({base}/{checksum}) |",
                    )
                self.assertNotIn("old.tar.gz", result.stdout)
                note = (
                    "**macOS:** Extract the `.app.zip`, drag `Markview.app` to Applications, "
                    "then run `xattr -d com.apple.quarantine /Applications/Markview.app` in Terminal "
                    "to remove the quarantine flag before opening the app."
                )
                self.assertIn(rows[-1] + "\n\n" + note + "\n\n" + suffix, result.stdout)
                repeated, *_ = self.run_renderer(version, body=result.stdout)
                self.assertEqual(repeated.returncode, 0, repeated.stderr)
                self.assertEqual(repeated.stdout, result.stdout)

    def test_missing_package_or_checksum_prevents_update(self):
        for name in ("markview-0.1.10-x86_64.AppImage", "markview-macos-SHA256SUMS"):
            with self.subTest(name=name):
                result, *_ = self.run_renderer("0.1.10", missing=name)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn(name, result.stderr)


if __name__ == "__main__":
    unittest.main()
