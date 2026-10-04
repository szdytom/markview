#!/usr/bin/env python3
"""Build Markview using Rust and the Android SDK, without a Gradle runtime."""
import argparse
import fcntl
import os
from pathlib import Path
import shutil
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {"x86_64": "x86_64-linux-android", "arm64-v8a": "aarch64-linux-android"}


def run(*args, env=None):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--abi", choices=[*TARGETS, "all"], default="all")
    parser.add_argument("--release", action="store_true")
    args = parser.parse_args()
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
    text = (ROOT / "android/AndroidManifest.xml").read_text()
    if args.release:
        text = text.replace('android:debuggable="true"', 'android:debuggable="false"')
    manifest.write_text(text)
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
    key = build / "debug.keystore"
    if not key.exists():
        run("keytool", "-genkeypair", "-keystore", key, "-storepass", "android", "-keypass", "android", "-alias", "androiddebugkey", "-dname", "CN=Markview Development", "-keyalg", "RSA", "-validity", "10000")
    run(tools / "apksigner", "sign", "--ks", key, "--ks-pass", "pass:android", "--key-pass", "pass:android", apk)
    run(tools / "apksigner", "verify", apk)
    print(f"Markview APK: {apk}")


if __name__ == "__main__":
    main()
