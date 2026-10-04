#!/usr/bin/env python3
"""Build Markview using Rust and the Android SDK, without a Gradle runtime."""
import argparse
import fcntl
import os
from pathlib import Path
import shutil
import subprocess
import tomllib
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {"x86_64": "x86_64-linux-android", "arm64-v8a": "aarch64-linux-android"}


def run(*args, env=None):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--abi", choices=[*TARGETS, "all"], default="all")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--version-code", type=int, default=1)
    parser.add_argument("--keystore", type=Path, help="Release keystore; passwords come from ANDROID_KEYSTORE_PASSWORD and ANDROID_KEY_PASSWORD")
    parser.add_argument("--key-alias", default="markview")
    args = parser.parse_args()
    if not 1 <= args.version_code <= 2100000000:
        parser.error("--version-code must be between 1 and 2100000000")
    if args.keystore:
        if not args.release:
            parser.error("--keystore requires --release")
        if not args.keystore.is_file():
            parser.error("--keystore must name an existing file")
        for name in ("ANDROID_KEYSTORE_PASSWORD", "ANDROID_KEY_PASSWORD"):
            if not os.environ.get(name):
                parser.error(f"{name} is required with --keystore")
    sdk = Path(os.environ.get("ANDROID_HOME", ROOT / ".tools/android-sdk"))
    ndk = Path(os.environ.get("ANDROID_NDK_HOME", sdk / "ndk/29.0.14206865"))
    tools = sdk / "build-tools/35.0.0"
    android = sdk / "platforms/android-35/android.jar"
    host = {"linux": "linux-x86_64", "darwin": "darwin-x86_64"}[os.sys.platform]
    llvm = ndk / "toolchains/llvm/prebuilt" / host / "bin"
    build = ROOT / "target/android"
    build.mkdir(parents=True, exist_ok=True)
    lock = (build / ".build.lock").open("w")
    fcntl.flock(lock, fcntl.LOCK_EX)
    libs = {}
    profile = "release" if args.release else "debug"
    for abi in TARGETS if args.abi == "all" else [args.abi]:
        target = TARGETS[abi]
        clang = llvm / f"{target}28-clang"
        env = dict(os.environ)
        env[f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER"] = str(clang)
        env[f"CARGO_TARGET_{target.upper().replace('-', '_')}_RUSTFLAGS"] = "-C link-arg=-Wl,-z,max-page-size=16384"
        env[f"CC_{target.replace('-', '_')}"] = str(clang)
        env[f"AR_{target.replace('-', '_')}"] = str(llvm / "llvm-ar")
        run("cargo", "build", "--locked", "--lib", "--target", target, *(["--release"] if args.release else []), env=env)
        library = build / f"libmarkview-{abi}.so"
        shutil.copy2(ROOT / "target" / target / profile / "libmarkview.so", library)
        run(llvm / "llvm-strip", "--strip-debug", library)
        libs[abi] = library
    classes = build / "classes"
    shutil.rmtree(classes, ignore_errors=True)
    classes.mkdir()
    run("javac", "--release", "8", "-Xlint:deprecation", "-classpath", android, "-d", classes, *sorted((ROOT / "android/java").rglob("*.java")))
    dex = build / "dex"
    dex.mkdir(exist_ok=True)
    run(tools / "d8", "--min-api", "28", "--lib", android, "--output", dex, *sorted(classes.rglob("*.class")))
    resources = build / "resources.zip"
    run(tools / "aapt2", "compile", "--dir", ROOT / "android/res", "-o", resources)
    unaligned = build / "markview-android-unaligned.apk"
    manifest = build / "AndroidManifest.xml"
    namespace = "http://schemas.android.com/apk/res/android"
    ET.register_namespace("android", namespace)
    tree = ET.parse(ROOT / "android/AndroidManifest.xml")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    tree.getroot().set(f"{{{namespace}}}versionName", version)
    tree.getroot().set(f"{{{namespace}}}versionCode", str(args.version_code))
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
    apk = build / f"markview-android-{profile}.apk"
    run(tools / "zipalign", "-f", "-P", "16", "4", unaligned, apk)
    key = args.keystore or build / "debug.keystore"
    if not args.keystore and not key.exists():
        run("keytool", "-genkeypair", "-keystore", key, "-storepass", "android", "-keypass", "android", "-alias", "androiddebugkey", "-dname", "CN=Markview Development", "-keyalg", "RSA", "-validity", "10000")
    run(tools / "apksigner", "sign", "--ks", key,
        "--ks-key-alias", args.key_alias if args.keystore else "androiddebugkey",
        "--ks-pass", "env:ANDROID_KEYSTORE_PASSWORD" if args.keystore else "pass:android",
        "--key-pass", "env:ANDROID_KEY_PASSWORD" if args.keystore else "pass:android", apk)
    run(tools / "apksigner", "verify", apk)
    lock.close()
    print(f"Markview APK: {apk}")


if __name__ == "__main__":
    main()
