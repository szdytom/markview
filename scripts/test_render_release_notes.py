"""Exercise the release-notes command with cargo-dist and GitHub inputs."""

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from render_release_notes import VSCODE_NOTE, VSCODE_PLATFORMS

SCRIPT = Path(__file__).with_name("render_release_notes.py")


class ReleaseNotesTest(unittest.TestCase):
    def run_renderer(self, version, missing=None, body=None, missing_digest=False):
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
            "markview-x86_64-pc-windows-msvc.msi",
            "markview-x86_64-pc-windows-msvc.zip",
            "markview-aarch64-pc-windows-msvc.msi",
            "markview-aarch64-pc-windows-msvc.zip",
            f"markview-{version}-x86_64.AppImage",
            f"markview-{version}-android.apk",
        ]
        extension = json.loads((SCRIPT.parents[1] /
                                "web/app/vsc-ext/vscode/package.json").read_text())
        names.extend(f"{extension['name']}-{extension['version']}-{target}.vsix"
                     for target, _ in VSCODE_PLATFORMS)
        prefix = "## Release Notes\n\nChanges.\n\n## Install markview\n\n```sh\ninstall\n```\n\n"
        suffix = "## Verifying GitHub Artifact Attestations\n\nVerification.\n"
        release = {
            "assets": [
                {
                    "name": name,
                    "digest": None if missing_digest else "sha256:" + hashlib.sha256(name.encode()).hexdigest(),
                }
                for name in names if name != missing
            ],
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

    def test_package_downloads_preserve_other_sections(self):
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
                self.assertEqual(len(rows), 12)
                self.assertNotIn('darwin-x64', result.stdout)
                for row, name, platform in zip(
                    rows,
                    names,
                    (
                        "Apple Silicon macOS",
                        "x64 Windows",
                        "x64 Windows (Portable)",
                        "ARM64 Windows",
                        "ARM64 Windows (Portable)",
                        "x64 Linux",
                        "Android 11+ (ARM64)",
                        *(label for _, label in VSCODE_PLATFORMS),
                    ),
                ):
                    self.assertEqual(
                        row,
                        f"| [{name}]({base}/{name}) | {platform} | `{hashlib.sha256(name.encode()).hexdigest()}` |",
                    )
                self.assertNotIn("old.tar.gz", result.stdout)
                self.assertIn("Checksum (SHA-256)", result.stdout)
                self.assertNotIn("[checksum]", result.stdout)
                note = (
                    "**macOS:** Extract the `.app.zip`, drag `Markview.app` to Applications, "
                    "then run `xattr -d com.apple.quarantine /Applications/Markview.app` in Terminal "
                    "to remove the quarantine flag before opening the app."
                )
                self.assertIn(rows[-1] + "\n\n" + note + "\n\n" + VSCODE_NOTE +
                              "\n\n" + suffix, result.stdout)
                repeated, *_ = self.run_renderer(version, body=result.stdout)
                self.assertEqual(repeated.returncode, 0, repeated.stderr)
                self.assertEqual(repeated.stdout, result.stdout)

    def test_missing_package_prevents_update(self):
        extension = json.loads((SCRIPT.parents[1] /
                                "web/app/vsc-ext/vscode/package.json").read_text())
        for name in (
            "markview-0.1.10-x86_64.AppImage",
            "markview-0.1.10-android.apk",
            "markview-aarch64-pc-windows-msvc.msi",
            "markview-aarch64-pc-windows-msvc.zip",
            f"{extension['name']}-{extension['version']}-win32-arm64.vsix",
        ):
            with self.subTest(name=name):
                result, *_ = self.run_renderer("0.1.10", missing=name)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn(name, result.stderr)

    def test_missing_digest_prevents_update(self):
        result, *_ = self.run_renderer("0.1.10", missing_digest=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("SHA-256 is missing", result.stderr)


if __name__ == "__main__":
    unittest.main()
