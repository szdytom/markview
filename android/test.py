#!/usr/bin/env python3
"""Run isolated Markview integration tests on an Android emulator."""
import argparse
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import struct
import subprocess
import time
import xml.etree.ElementTree as ET
import zipfile
from build import ROOT, run

PACKAGE = "io.github.szdytom.markview"
TEST_PACKAGE = PACKAGE + ".test"
SUCCESS = "MARKVIEW_ANDROID_INTEGRATION_OK"


def test_classes():
    namespace = "{http://schemas.android.com/apk/res/android}"
    return [node.get(namespace + "name") for node in
            ET.parse(ROOT / "android/tests/AndroidManifest.xml").getroot().findall("instrumentation")]


def build_tests(sdk, build):
    tools = sdk / "build-tools/35.0.0"
    android = sdk / "platforms/android-35/android.jar"
    classes = build / "test-classes"
    shutil.rmtree(classes, ignore_errors=True)
    classes.mkdir()
    run("javac", "--release", "8", "-Xlint:all", "-Werror", "-classpath", str(android) + os.pathsep + str(build / "classes"), "-d", classes, *sorted((ROOT / "android/tests").rglob("*.java")))
    dex = build / "test-dex"
    dex.mkdir(exist_ok=True)
    run(tools / "d8", "--min-api", "30", "--lib", android, "--classpath", build / "classes", "--output", dex, *sorted(classes.rglob("*.class")))
    unsigned = build / "markview-android-tests-unaligned.apk"
    run(tools / "aapt2", "link", "-I", android, "--manifest", ROOT / "android/tests/AndroidManifest.xml", "-o", unsigned)
    with zipfile.ZipFile(unsigned, "a", compression=zipfile.ZIP_DEFLATED) as apk:
        apk.write(dex / "classes.dex", "classes.dex")
        for file in sorted((ROOT / "android/tests/assets").iterdir()):
            apk.write(file, "assets/" + file.name)
        apk.write(ROOT / "crates/markview-layout/tests/fonts/NotoSans-Regular-subset.otf", "assets/Test.otf")
    apk = build / "markview-android-tests.apk"
    run(tools / "zipalign", "-f", "4", unsigned, apk)
    run(tools / "apksigner", "sign", "--ks", build / "debug.keystore", "--ks-pass", "pass:android", apk)


def cache_fixture(build):
    # Match the `Trusted` local-file cache partition; grants are absent.
    url = "https://raw.githubusercontent.com/szdytom/markview/main/assets/markview-icon-color.svg"
    security = hashlib.sha256(b'{"origin":{"Local":"Trusted"},"grants":[],"content":null}').hexdigest()
    helper = build / "cache-key.rs"
    helper.write_text('use std::hash::{Hash,Hasher}; fn main(){ let mut h=std::collections::hash_map::DefaultHasher::new(); let mut args=std::env::args().skip(1); args.next().unwrap().hash(&mut h); Some(args.next().unwrap()).hash(&mut h); println!("{:016x}.img",h.finish()); }')
    run("rustc", "--crate-name", "cache_key", helper, "-o", build / "cache-key")
    name = subprocess.check_output([str(build / "cache-key"), url, security], text=True).strip()
    body = (ROOT / "assets/markview-icon-color.svg").read_bytes()
    header = json.dumps(dict(security=security, url=url, final_url=url, etag=None, last_modified=None, lifetime=3600, date=int(time.time()), no_cache=False, bytes=len(body))).encode()
    fixture = build / name
    fixture.write_bytes(b"MARKVIEW-CACHE/1\n" + struct.pack("<I", len(header)) + header + body)
    return fixture


class Device:
    def __init__(self, sdk, serial):
        self.command = [str(sdk / "platform-tools/adb"), "-s", serial]

    def run(self, *args, **kwargs):
        return subprocess.run([*self.command, *map(str, args)], **kwargs)

    def checked(self, *args):
        self.run(*args, check=True)

    def output(self, *args):
        return self.run(*args, check=True, capture_output=True, text=True).stdout.strip()

    def reset(self, fixture):
        self.checked("shell", "am", "force-stop", PACKAGE)
        self.checked("shell", "pm", "clear", PACKAGE)
        self.checked("shell", "appops", "set", PACKAGE, "MANAGE_EXTERNAL_STORAGE", "allow")
        if fixture:
            directory = "files/markview/cache/images"
            self.checked("shell", "run-as", PACKAGE, "mkdir", "-p", directory)
            self.checked("shell", "run-as", PACKAGE, "cp", "/data/local/tmp/" + fixture.name, directory + "/" + fixture.name)

    def screenshots(self, directory):
        listing = self.run("shell", "run-as", PACKAGE, "ls", "files/test-artifacts", capture_output=True, text=True)
        if listing.returncode == 0:
            for name in listing.stdout.splitlines():
                if name.endswith(".png"):
                    with (directory / name).open("wb") as output:
                        self.run("exec-out", "run-as", PACKAGE, "cat", "files/test-artifacts/" + name, stdout=output, check=True)


def run_case(device, name, args, artifacts, fixture):
    short = name.rsplit(".", 1)[-1]
    directory = artifacts / short
    shutil.rmtree(directory, ignore_errors=True)
    directory.mkdir(parents=True)
    device.reset(fixture)
    reports = []
    success = True
    try:
        # A process restart is part of this single case, so retain its seed data.
        phases = ("seed", "restore") if short == "SessionTest" else (None,)
        for phase in phases:
            device.checked("shell", "am", "force-stop", PACKAGE)
            command = ["shell", "am", "instrument", "-w", "-e", "layout", args.layout]
            if phase:
                command += ["-e", "session-phase", phase]
            command += [TEST_PACKAGE + "/" + name]
            try:
                result = device.run(*command, capture_output=True, text=True, timeout=args.timeout)
                report = result.stdout + result.stderr
                success = result.returncode == 0 and SUCCESS in report
            except subprocess.TimeoutExpired as error:
                report = f"TIMEOUT: {short} exceeded {args.timeout} seconds\n"
                for output in (error.stdout, error.stderr):
                    if output:
                        report += output.decode(errors="replace") if isinstance(output, bytes) else output
                success = False
            reports.append((f"PHASE: {phase}\n" if phase else "") + report)
            if not success:
                break
        device.screenshots(directory)
    finally:
        device.checked("shell", "am", "force-stop", PACKAGE)
        (directory / "integration.txt").write_text("\n".join(reports))
    print("\n".join(reports), flush=True)
    return success


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", default="emulator-5554")
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--filter", action="append", metavar="GLOB", help="Select test class names with a shell glob; repeat to include more classes")
    for flag, name in (("session", "SessionTest"), ("lifecycle", "LifecycleTest"), ("mermaid", "MermaidTest"), ("links", "LinksTest"), ("layout", "LayoutTest")):
        selection.add_argument("--" + flag + "-only", dest="only", action="store_const", const=name, help="Run only " + name)
    parser.add_argument("--list", action="store_true", help="List available matching cases without building or connecting to a device")
    parser.add_argument("--shuffle", type=int, metavar="SEED", help="Shuffle cases reproducibly with this seed")
    parser.add_argument("--layout", choices=["phone", "tablet"], default="phone", help="Expected sw600dp layout on the test device")
    parser.add_argument("--online", action="store_true", help="Fetch the image from GitHub instead of using a deterministic warm-cache fixture")
    parser.add_argument("--timeout", type=int, default=180, help="Maximum runtime in seconds for each instrumentation phase")
    args = parser.parse_args(argv)
    patterns = args.filter or [args.only or "*"]
    cases = [name for name in test_classes() if any(fnmatch.fnmatchcase(name.rsplit(".", 1)[-1], pattern) for pattern in patterns)]
    # Process-restart checks retain their existing opt-in entry point.
    if not (args.filter or args.only or args.list):
        cases = [name for name in cases if not name.endswith(".SessionTest")]
    if not cases:
        parser.error("no test classes match " + ", ".join(patterns))
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    if args.shuffle is not None:
        random.Random(args.shuffle).shuffle(cases)
    if args.list:
        print("\n".join(name.rsplit(".", 1)[-1] for name in cases))
        return 0

    sdk = Path(os.environ.get("ANDROID_HOME", ROOT / ".tools/android-sdk"))
    build = ROOT / "target/android"
    suffix = {"LinksTest": "-links", "MermaidTest": "-mermaid", "LifecycleTest": "-lifecycle", "LayoutTest": "-boundary", "SessionTest": "-session"}.get(args.only, "")
    artifacts = ROOT / "artifacts/android" / (args.layout + suffix)
    artifacts.mkdir(parents=True, exist_ok=True)
    build_tests(sdk, build)
    device = Device(sdk, args.serial)
    device.checked("install", "--no-incremental", "-r", build / "markview-android-debug.apk")
    device.checked("install", "--no-incremental", "-r", build / "markview-android-tests.apk")
    fixture = None if args.online else cache_fixture(build)
    if fixture:
        device.checked("push", fixture, "/data/local/tmp/" + fixture.name)
    keyboard = device.output("shell", "settings", "get", "secure", "show_ime_with_hard_keyboard")
    links = any(name.endswith(".LinksTest") for name in cases)
    outcomes = []
    try:
        if links:
            # Keep production network checks enabled while routing the fixture locally.
            device.checked("root")
            device.checked("wait-for-device")
            device.checked("shell", "ip", "addr", "replace", "198.18.0.1/32", "dev", "lo")
        device.checked("shell", "settings", "put", "secure", "show_ime_with_hard_keyboard", "1")
        for name in cases:
            print("RUN: " + name, flush=True)
            passed = run_case(device, name, args, artifacts, fixture)
            outcomes.append((name.rsplit(".", 1)[-1], passed))
    finally:
        if links:
            device.checked("shell", "ip", "addr", "del", "198.18.0.1/32", "dev", "lo")
        if keyboard == "null":
            device.checked("shell", "settings", "delete", "secure", "show_ime_with_hard_keyboard")
        else:
            device.checked("shell", "settings", "put", "secure", "show_ime_with_hard_keyboard", keyboard)
        summary = "\n".join(("PASS: " if passed else "FAIL: ") + name for name, passed in outcomes) + "\n"
        (artifacts / "integration.txt").write_text(summary)
    print(summary)
    return 0 if all(passed for _, passed in outcomes) else 1


if __name__ == "__main__":
    raise SystemExit(main())
