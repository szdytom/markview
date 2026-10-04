"""Exercise APK assembly without compiling Rust or downloading an Android SDK."""

import contextlib
import io
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET
import zipfile

import build


class AndroidBuildTest(unittest.TestCase):
    def test_debug_pr_and_distribution_apk_assembly(self):
        for profile, distribution in (("debug", False), ("release", False), ("release", True)):
            release = profile == "release"
            with self.subTest(profile=profile, distribution=distribution), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "android").mkdir()
                shutil.copy2(build.ROOT / "android/AndroidManifest.xml", root / "android/AndroidManifest.xml")
                (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.2.0-beta.1"\n')
                key = root / "distribution.keystore"
                key.write_bytes(b"distribution key")
                commands = []

                def run(*args, env=None):
                    args = list(map(str, args))
                    commands.append(args)
                    tool = Path(args[0]).name
                    if tool == "cargo":
                        target = args[args.index("--target") + 1]
                        profile = "release" if "--release" in args else "debug"
                        library = root / "target" / target / profile / "libmarkview.so"
                        library.parent.mkdir(parents=True)
                        library.write_bytes(target.encode())
                        self.assertIn("--locked", args)
                        self.assertIn("max-page-size=16384", env[f"CARGO_TARGET_{target.upper().replace('-', '_')}_RUSTFLAGS"])
                    elif tool == "d8":
                        (Path(args[args.index("--output") + 1]) / "classes.dex").write_bytes(b"dex")
                    elif tool == "aapt2":
                        with zipfile.ZipFile(args[args.index("-o") + 1], "w"):
                            pass
                    elif tool == "zipalign":
                        shutil.copy2(args[-2], args[-1])
                    elif tool == "keytool":
                        Path(args[args.index("-keystore") + 1]).write_bytes(b"development key")

                options = ["build.py"]
                if release:
                    options += ["--release", "--version-code", "42"]
                if distribution:
                    options += ["--keystore", str(key), "--key-alias", "distribution"]
                credentials = {"ANDROID_KEYSTORE_PASSWORD": "store secret", "ANDROID_KEY_PASSWORD": "key secret"} if distribution else {}
                with (
                    patch.object(build, "ROOT", root),
                    patch.object(build, "run", side_effect=run),
                    patch.object(sys, "argv", options),
                    patch.dict(os.environ, credentials, clear=True),
                    contextlib.redirect_stdout(io.StringIO()),
                ):
                    build.main()

                manifest = ET.parse(root / "target/android/AndroidManifest.xml")
                android = "{http://schemas.android.com/apk/res/android}"
                self.assertEqual(manifest.getroot().get(android + "versionName"), "0.2.0-beta.1")
                self.assertEqual(manifest.getroot().get(android + "versionCode"), "42" if release else "1")
                self.assertEqual(manifest.find("application").get(android + "debuggable"), str(not release).lower())
                apk = root / f"target/android/markview-android-{profile}.apk"
                with zipfile.ZipFile(apk) as archive:
                    self.assertEqual(set(archive.namelist()), {"classes.dex", "lib/arm64-v8a/libmarkview.so", "lib/x86_64/libmarkview.so"})
                sign = next(args for args in commands if Path(args[0]).name == "apksigner" and args[1] == "sign")
                self.assertEqual(sign[sign.index("--ks") + 1], str(key) if distribution else str(root / "target/android/debug.keystore"))
                self.assertEqual(sign[sign.index("--ks-key-alias") + 1], "distribution" if distribution else "androiddebugkey")
                self.assertEqual(sign[sign.index("--ks-pass") + 1], "env:ANDROID_KEYSTORE_PASSWORD" if distribution else "pass:android")
                self.assertEqual(sign[sign.index("--key-pass") + 1], "env:ANDROID_KEY_PASSWORD" if distribution else "pass:android")
                self.assertNotIn("store secret", " ".join(sign))
                self.assertNotIn("key secret", " ".join(sign))
                self.assertEqual((root / "target/android/debug.keystore").exists(), not distribution)
                self.assertEqual(Path(commands[-1][0]).name, "apksigner")
                self.assertEqual(commands[-1][1], "verify")

    def test_invalid_signing_inputs_fail_before_building(self):
        with tempfile.NamedTemporaryFile() as key:
            for options in (["--version-code", "0"], ["--version-code", "2100000001"], ["--keystore", key.name], ["--release", "--keystore", key.name]):
                with self.subTest(options=options), patch.object(sys, "argv", ["build.py", *options]), patch.dict(os.environ, {}, clear=True), patch.object(build, "run") as run, contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                    build.main()
                self.assertEqual(error.exception.code, 2)
                run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
