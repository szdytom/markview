#!/usr/bin/env python3
"""Build Markview using Rust and the Android SDK, without a Gradle runtime."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import xml.etree.ElementTree as ET
import zipfile

from config import load, sdk_directory

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {"x86_64": "x86_64-linux-android", "arm64-v8a": "aarch64-linux-android"}


def run(*args, env=None):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, env=env, check=True)


def license_files(build, cargo, abis):
    config = build / "about.toml"
    config.write_text("ignore-build-dependencies = true\nignore-dev-dependencies = true\n" + (ROOT / "about.toml").read_text())
    command = (*cargo, "about", "generate", "--locked", "--fail", "--config", config,
               *[arg for abi in abis for arg in ("--target", TARGETS[abi])])
    notices = build / "third-party-notices.html"
    run(*command, ROOT / "packaging/about.hbs", "-o", notices)
    report = build / "dependencies.json"
    run(*command, "--format", "json", "-o", report)
    files = [(file, "assets/licenses/" + file.name) for file in
             [ROOT / "LICENSE", ROOT / "THIRD_PARTY.md", *sorted((ROOT / "licenses").glob("*.txt")), notices]]
    for crate in json.loads(report.read_text())["crates"]:
        package = crate["package"]
        for file in sorted(Path(package["manifest_path"]).parent.iterdir()):
            if file.is_file() and file.name.upper().startswith(("NOTICE", "COPYRIGHT")):
                files.append((file, f"assets/licenses/dependencies/{package['name']}-{package['version']}/{file.name}"))
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--abi", choices=[*TARGETS, "all"], help="APK architectures (default: arm64-v8a for release, all for debug)")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--version-code", type=int, help="Override workspace.metadata.android.version-code")
    signing = parser.add_mutually_exclusive_group()
    signing.add_argument("--unsigned", action="store_true", help="Produce an aligned, unsigned release APK for external signing")
    signing.add_argument("--keystore", type=Path, help="Release keystore; passwords come from ANDROID_KEYSTORE_PASSWORD and ANDROID_KEY_PASSWORD")
    parser.add_argument("--key-alias", default="markview")
    args = parser.parse_args()
    selected_abi = args.abi or ("arm64-v8a" if args.release else "all")
    config = load(ROOT)
    version_code = config["version-code"] if args.version_code is None else args.version_code
    if not 1 <= version_code <= 2100000000:
        parser.error("--version-code must be between 1 and 2100000000")
    if args.unsigned and not args.release:
        parser.error("--unsigned requires --release")
    if args.keystore:
        if not args.release:
            parser.error("--keystore requires --release")
        if not args.keystore.is_file():
            parser.error("--keystore must name an existing file")
        for name in ("ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_PASSWORD"):
            if not os.environ.get(name):
                parser.error(f"{name} is required with --keystore")
    sdk = sdk_directory()
    ndk = Path(os.environ.get("ANDROID_NDK_HOME", sdk / "ndk" / config["ndk"]))
    tools = sdk / "build-tools" / config["build-tools"]
    android = sdk / f"platforms/android-{config['target-sdk']}/android.jar"
    host = {"linux": "linux-x86_64", "darwin": "darwin-x86_64"}[os.sys.platform]
    llvm = ndk / "toolchains/llvm/prebuilt" / host / "bin"
    build = ROOT / "target/android"
    build.mkdir(parents=True, exist_ok=True)
    with (build / ".build.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        libs = {}
        profile = "release" if args.release else "debug"
        abis = list(TARGETS) if selected_abi == "all" else [selected_abi]
        cargo = ("cargo", f"+{config['rust-version']}")
        for abi in abis:
            target = TARGETS[abi]
            clang = llvm / f"{target}{config['min-sdk']}-clang"
            env = dict(os.environ)
            env[f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER"] = str(clang)
            env[f"CARGO_TARGET_{target.upper().replace('-', '_')}_RUSTFLAGS"] = "-C link-arg=-Wl,-z,max-page-size=16384"
            env[f"CC_{target.replace('-', '_')}"] = str(clang)
            env[f"AR_{target.replace('-', '_')}"] = str(llvm / "llvm-ar")
            run(*cargo, "build", "--locked", "--lib", "--target", target, *(["--release"] if args.release else []), env=env)
            library = build / f"libmarkview-{abi}.so"
            shutil.copy2(ROOT / "target" / target / profile / "libmarkview.so", library)
            run(llvm / "llvm-strip", "--strip-debug", library)
            libs[abi] = library
        classes = build / "classes"
        shutil.rmtree(classes, ignore_errors=True)
        classes.mkdir()
        run("javac", "--release", config["java-version"], "-Xlint:deprecation", "-classpath", android, "-d", classes, *sorted((ROOT / "android/java").rglob("*.java")))
        dex = build / "dex"
        dex.mkdir(exist_ok=True)
        run(tools / "d8", "--min-api", config["min-sdk"], "--lib", android, "--output", dex, *sorted(classes.rglob("*.class")))
        licenses = license_files(build, cargo, abis)
        resources = build / "resources.zip"
        run(tools / "aapt2", "compile", "--dir", ROOT / "android/res", "-o", resources)
        unaligned = build / "markview-android-unaligned.apk"
        manifest = build / "AndroidManifest.xml"
        namespace = "http://schemas.android.com/apk/res/android"
        ET.register_namespace("android", namespace)
        tree = ET.parse(ROOT / "android/AndroidManifest.xml")
        tree.getroot().set(f"{{{namespace}}}versionName", config["version-name"])
        tree.getroot().set(f"{{{namespace}}}versionCode", str(version_code))
        tree.find("application").set(f"{{{namespace}}}debuggable", str(not args.release).lower())
        tree.write(manifest, encoding="unicode")
        run(tools / "aapt2", "link", "-I", android, "--manifest", manifest, "-o", unaligned, resources)
        with zipfile.ZipFile(unaligned, "a", compression=zipfile.ZIP_DEFLATED) as apk:
            apk.write(dex / "classes.dex", "classes.dex")
            for abi, library in libs.items():
                apk.write(library, f"lib/{abi}/libmarkview.so")
            for file in sorted((ROOT / "android/assets").rglob("*")):
                if file.is_file():
                    apk.write(file, "assets/" + str(file.relative_to(ROOT / "android/assets")))
            for file, name in licenses:
                apk.write(file, name)
        suffix = "-unsigned" if args.unsigned else ""
        apk = build / f"markview-android-{profile}{suffix}.apk"
        run(tools / "zipalign", "-f", "-P", "16", "4", unaligned, apk)
        if not args.unsigned:
            key = args.keystore or build / "debug.keystore"
            if not args.keystore and not key.exists():
                run("keytool", "-genkeypair", "-keystore", key, "-storepass", "android", "-keypass", "android", "-alias", "androiddebugkey", "-dname", "CN=Markview Development", "-keyalg", "RSA", "-validity", "10000")
            run(tools / "apksigner", "sign", "--ks", key,
                "--ks-key-alias", args.key_alias if args.keystore else "androiddebugkey",
                "--ks-pass", "env:ANDROID_KEYSTORE_PASSWORD" if args.keystore else "pass:android",
                "--key-pass", "env:ANDROID_KEY_PASSWORD" if args.keystore else "pass:android", apk)
            run(tools / "apksigner", "verify", apk)
    print(f"Markview APK: {apk}")


if __name__ == "__main__":
    main()
