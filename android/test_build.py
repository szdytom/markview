"""Exercise APK assembly without compiling Rust or downloading an Android SDK."""

import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET
import zipfile

import build


class AndroidBuildTest(unittest.TestCase):
    def test_sdk_location_is_shared_across_checkouts_and_can_be_overridden(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(Path, "home", return_value=Path(directory)):
            with patch.dict(os.environ, {}, clear=True):
                self.assertEqual(build.sdk_directory(), Path(directory) / "Android/Sdk")
            custom = Path(directory) / "custom-sdk"
            with patch.dict(os.environ, {"ANDROID_HOME": str(custom)}, clear=True):
                self.assertEqual(build.sdk_directory(), custom)

    def prepare_sources(self, root):
        (root / "android").mkdir()
        shutil.copy2(build.ROOT / "android/AndroidManifest.xml", root / "android/AndroidManifest.xml")
        (root / "Cargo.toml").write_text(
            '[workspace.package]\nversion = "0.2.0-beta.1"\n'
            '[workspace.metadata.android]\nversion-code = 196\nrust-version = "1.99.0"\n'
            'java-version = "21"\nbuild-tools = "36.0.0"\nndk = "28.2.13676358"\n'
        )
        for name in ("LICENSE", "THIRD_PARTY.md", "about.toml"):
            shutil.copy2(build.ROOT / name, root / name)
        shutil.copytree(build.ROOT / "licenses", root / "licenses")
        dependency = root / "dependency"
        dependency.mkdir()
        (dependency / "NOTICE").write_text("Additional dependency attribution\n")
        (dependency / "COPYRIGHT").write_text("Dependency copyright holder\n")

    def test_debug_pr_and_distribution_apk_assembly(self):
        cases = (
            ("debug", False, False, None, set(build.TARGETS)),
            ("debug", False, False, "x86_64", {"x86_64"}),
            ("release", False, False, None, {"arm64-v8a"}),
            ("release", True, False, None, {"arm64-v8a"}),
            ("release", True, False, "arm64-v8a", {"arm64-v8a"}),
            ("release", False, False, "all", set(build.TARGETS)),
            ("release", False, False, "x86_64", {"x86_64"}),
            ("release", False, True, None, {"arm64-v8a"}),
            ("release", False, True, "all", set(build.TARGETS)),
            ("release", False, True, "x86_64", {"x86_64"}),
        )
        for profile, distribution, unsigned, abi, expected_abis in cases:
            release = profile == "release"
            with self.subTest(profile=profile, distribution=distribution, unsigned=unsigned, abi=abi), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.prepare_sources(root)
                key = root / "distribution.keystore"
                key.write_bytes(b"distribution key")
                commands = []

                def run(*args, env=None):
                    args = list(map(str, args))
                    commands.append(args)
                    tool = Path(args[0]).name
                    if tool == "cargo" and args[2] == "build":
                        target = args[args.index("--target") + 1]
                        profile = "release" if "--release" in args else "debug"
                        library = root / "target" / target / profile / "libmarkview.so"
                        library.parent.mkdir(parents=True)
                        library.write_bytes(target.encode())
                        self.assertIn("--locked", args)
                        self.assertEqual(args[1], "+1.99.0")
                        self.assertIn("/ndk/28.2.13676358/", env[f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER"])
                        self.assertIn("max-page-size=16384", env[f"CARGO_TARGET_{target.upper().replace('-', '_')}_RUSTFLAGS"])
                    elif tool == "cargo" and args[2] == "about":
                        self.assertIn("--locked", args)
                        self.assertIn("--fail", args)
                        config = tomllib.loads(Path(args[args.index("--config") + 1]).read_text())
                        self.assertTrue(config["ignore-dev-dependencies"])
                        self.assertTrue(config["ignore-build-dependencies"])
                        targets = {args[i + 1] for i, value in enumerate(args) if value == "--target"}
                        self.assertEqual(targets, {build.TARGETS[abi] for abi in expected_abis})
                        output = Path(args[args.index("-o") + 1])
                        if "--format" in args:
                            output.write_text(json.dumps({"crates": [{"package": {"name": "fixture", "version": "1.0.0", "manifest_path": str(root / "dependency/Cargo.toml")}}]}))
                        else:
                            output.write_text("Test dependency license text")
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
                    options += ["--release"]
                    if not unsigned:
                        options += ["--version-code", "42"]
                if abi:
                    options += ["--abi", abi]
                if distribution:
                    options += ["--keystore", str(key), "--key-alias", "distribution"]
                if unsigned:
                    options += ["--unsigned"]
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
                self.assertEqual(manifest.getroot().get(android + "versionCode"), "42" if release and not unsigned else "196")
                self.assertEqual(manifest.find("application").get(android + "debuggable"), str(not release).lower())
                suffix = "-unsigned" if unsigned else ""
                apk = root / f"target/android/markview-android-{profile}{suffix}.apk"
                with zipfile.ZipFile(apk) as archive:
                    licenses = {"assets/licenses/" + name for name in ("LICENSE", "THIRD_PARTY.md", "third-party-notices.html")}
                    licenses |= {"assets/licenses/" + file.name for file in (root / "licenses").glob("*.txt")}
                    licenses |= {"assets/licenses/dependencies/fixture-1.0.0/" + name for name in ("NOTICE", "COPYRIGHT")}
                    self.assertEqual(set(archive.namelist()), {"classes.dex"} | {f"lib/{abi}/libmarkview.so" for abi in expected_abis} | licenses)
                    self.assertEqual(archive.read("assets/licenses/LICENSE"), (root / "LICENSE").read_bytes())
                    self.assertEqual(archive.read("assets/licenses/third-party-notices.html"), b"Test dependency license text")
                    self.assertEqual(archive.read("assets/licenses/KaTeX-OFL.txt"), (root / "licenses/KaTeX-OFL.txt").read_bytes())
                    self.assertEqual(archive.read("assets/licenses/dependencies/fixture-1.0.0/NOTICE"), b"Additional dependency attribution\n")
                targets = {args[args.index("--target") + 1] for args in commands if args[0] == "cargo" and args[2] == "build"}
                self.assertEqual(targets, {build.TARGETS[abi] for abi in expected_abis})
                align = next(args for args in commands if Path(args[0]).name == "zipalign")
                self.assertIn("/build-tools/36.0.0/", align[0])
                self.assertEqual(align[1:6], ["-f", "-P", "16", "4", str(root / "target/android/markview-android-unaligned.apk")])
                javac = next(args for args in commands if args[0] == "javac")
                self.assertEqual(javac[javac.index("--release") + 1], "21")
                if unsigned:
                    self.assertFalse(any(Path(args[0]).name in ("keytool", "apksigner") for args in commands))
                    self.assertFalse((root / "target/android/debug.keystore").exists())
                    self.assertEqual(commands[-1], align)
                    continue
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
            for options in (["--version-code", "0"], ["--version-code", "2100000001"], ["--keystore", key.name], ["--release", "--keystore", key.name], ["--unsigned"], ["--release", "--unsigned", "--keystore", key.name]):
                with self.subTest(options=options), patch.object(sys, "argv", ["build.py", *options]), patch.dict(os.environ, {}, clear=True), patch.object(build, "run") as run, contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                    build.main()
                self.assertEqual(error.exception.code, 2)
                run.assert_not_called()

    def test_workspace_default_version_code_is_validated(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.prepare_sources(root)
            manifest = root / "Cargo.toml"
            manifest.write_text(manifest.read_text().replace("version-code = 196", "version-code = 0"))
            with patch.object(build, "ROOT", root), patch.object(sys, "argv", ["build.py", "--release", "--unsigned"]), patch.object(build, "run") as run, contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                build.main()
            run.assert_not_called()

    def test_failed_license_collection_cannot_package_stale_notices(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.prepare_sources(root)
            output = root / "target/android"
            output.mkdir(parents=True)
            (output / "third-party-notices.html").write_text("Stale notices")
            commands = []

            def run(*args, env=None):
                commands.append(args)
                if args[:3] == ("cargo", "+1.99.0", "build"):
                    library = root / "target/aarch64-linux-android/release/libmarkview.so"
                    library.parent.mkdir(parents=True)
                    library.write_bytes(b"native library")
                elif args[:3] == ("cargo", "+1.99.0", "about"):
                    raise subprocess.CalledProcessError(1, args)
                elif Path(args[0]).name == "d8":
                    (output / "dex/classes.dex").write_bytes(b"dex")

            with patch.object(build, "ROOT", root), patch.object(sys, "argv", ["build.py", "--release"]), patch.object(build, "run", side_effect=run), self.assertRaises(subprocess.CalledProcessError):
                build.main()
            self.assertFalse(list(output.glob("*.apk")))
            self.assertFalse(any(Path(args[0]).name in ("aapt2", "apksigner", "keytool") for args in commands))


if __name__ == "__main__":
    unittest.main()
