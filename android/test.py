#!/usr/bin/env python3
"""Run Markview integration tests on an Android emulator and keep screenshots."""
import argparse
from functools import partial
import os
import json
import struct
import time
from pathlib import Path
import shutil
import subprocess
import zipfile
from build import ROOT, run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", default="emulator-5554")
    parser.add_argument("--lifecycle-only", action="store_true", help="Check repeated Activity destruction and recreation in one process")
    parser.add_argument("--layout-only", action="store_true", help="Check resource selection, orientation and controls at a screen-size boundary")
    parser.add_argument("--layout", choices=["phone", "tablet"], default="phone", help="Expected sw600dp layout on the test device")
    parser.add_argument("--online", action="store_true", help="Fetch the image from GitHub instead of using a deterministic warm-cache fixture")
    parser.add_argument("--timeout", type=int, default=180, help="Maximum instrumentation runtime in seconds")
    args = parser.parse_args()
    sdk = Path(os.environ.get("ANDROID_HOME", ROOT / ".tools/android-sdk"))
    tools = sdk / "build-tools/35.0.0"
    android = sdk / "platforms/android-35/android.jar"
    build = ROOT / "target/android"
    artifacts = ROOT / "artifacts/android" / (args.layout + ("-lifecycle" if args.lifecycle_only else "-boundary" if args.layout_only else ""))
    artifacts.mkdir(parents=True, exist_ok=True)
    adb = partial(run, sdk / "platform-tools/adb", "-s", args.serial)
    classes = build / "test-classes"
    shutil.rmtree(classes, ignore_errors=True)
    classes.mkdir()
    run("javac", "--release", "8", "-Xlint:-options", "-classpath", str(android) + os.pathsep + str(build / "classes"), "-d", classes, *sorted((ROOT / "android/tests").rglob("*.java")))
    dex = build / "test-dex"
    dex.mkdir(exist_ok=True)
    run(tools / "d8", "--min-api", "28", "--lib", android, "--classpath", build / "classes", "--output", dex, *sorted(classes.rglob("*.class")))
    unsigned = build / "markview-android-tests-unaligned.apk"
    run(tools / "aapt2", "link", "-I", android, "--manifest", ROOT / "android/tests/AndroidManifest.xml", "-o", unsigned)
    with zipfile.ZipFile(unsigned, "a", compression=zipfile.ZIP_DEFLATED) as apk:
        apk.write(dex / "classes.dex", "classes.dex")
        for file in sorted((ROOT / "android/tests/assets").iterdir()):
            apk.write(file, "assets/" + file.name)
        apk.write(ROOT / "crates/markview-core/tests/fonts/NotoSans-Regular-subset.otf", "assets/Test.otf")
    apk = build / "markview-android-tests.apk"
    run(tools / "zipalign", "-f", "4", unsigned, apk)
    run(tools / "apksigner", "sign", "--ks", build / "debug.keystore", "--ks-pass", "pass:android", apk)
    adb("install", "--no-incremental", "-r", build / "markview-android-debug.apk")
    adb("install", "--no-incremental", "-r", apk)
    adb("shell", "run-as", "io.github.szdytom.markview", "rm", "-rf", "files/test-artifacts")
    # Seed a real disk entry so cache tests do not depend on public connectivity.
    adb("shell", "run-as", "io.github.szdytom.markview", "rm", "-rf", "files/markview/cache/images")
    if not args.online:
        url = "https://raw.githubusercontent.com/szdytom/markview/main/assets/markview-icon-color.svg"
        helper = build / "cache-key.rs"
        helper.write_text('use std::hash::{Hash,Hasher}; fn main(){ let mut h=std::collections::hash_map::DefaultHasher::new(); std::env::args().nth(1).unwrap().hash(&mut h); println!("{:016x}.img",h.finish()); }')
        run("rustc", "--crate-name", "cache_key", helper, "-o", build / "cache-key")
        name = subprocess.check_output([str(build / "cache-key"), url], text=True).strip()
        body = (ROOT / "assets/markview-icon-color.svg").read_bytes()
        header = json.dumps(dict(url=url, final_url=url, etag=None, last_modified=None, lifetime=3600, date=int(time.time()), no_cache=False, bytes=len(body))).encode()
        fixture = build / name
        fixture.write_bytes(b"MARKVIEW-CACHE/1\n" + struct.pack("<I", len(header)) + header + body)
        adb("push", fixture, "/data/local/tmp/" + name)
        adb("shell", "run-as", "io.github.szdytom.markview", "mkdir", "-p", "files/markview/cache/images")
        adb("shell", "run-as", "io.github.szdytom.markview", "cp", "/data/local/tmp/" + name, "files/markview/cache/images/" + name)
    result = subprocess.run([str(sdk / "platform-tools/adb"), "-s", args.serial, "shell", "am", "instrument", "-w", "-e", "layout", args.layout, "-e", "layout-only", str(args.layout_only).lower(), "-e", "lifecycle-only", str(args.lifecycle_only).lower(), "io.github.szdytom.markview.test/io.github.szdytom.markview.Smoke"], capture_output=True, text=True, timeout=args.timeout)
    report = result.stdout + result.stderr
    (artifacts / "integration.txt").write_text(report)
    print(report)
    success = result.returncode == 0 and "MARKVIEW_ANDROID_INTEGRATION_OK" in report
    screenshots = ["reader", "dark-reader", "settings", "fonts", "font-choices", "dark-styles", "diagnostics", "search", "resumed", "folder"] + (["tab-drawer", "tab-drawer-light"] if args.layout == "phone" else ["tablet-tabs", "landscape", "landscape-settings"])
    if args.layout_only:
        screenshots = ["reader", "settings", "layout"] + (["landscape-settings"] if args.layout == "tablet" else [])
    if args.lifecycle_only:
        screenshots = []
    if not success:
        screenshots.extend(["system-bars", "failure"])
    for name in screenshots:
        with (artifacts / f"{name}.png").open("wb") as output:
            capture = subprocess.run([str(sdk / "platform-tools/adb"), "-s", args.serial, "exec-out", "run-as", "io.github.szdytom.markview", "cat", f"files/test-artifacts/{name}.png"], stdout=output, stderr=subprocess.DEVNULL, check=success)
        if capture.returncode != 0:
            (artifacts / f"{name}.png").unlink()
    assert success, "Android integration tests failed"

if __name__ == "__main__":
    main()
