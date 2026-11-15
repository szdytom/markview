#!/usr/bin/env python3
"""Run workspace tests with ordinary and symlinked temporary roots on Unix."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
# Visual baselines have their own controlled runner and output directories.
TEST_ARGUMENTS = ("--skip", "rendering_baselines")


def build_binaries(diagnostics=False):
    command = ["cargo", "test", "--workspace", "--locked", "--all-targets", "--no-run",
               "--message-format=json"]
    if diagnostics:
        command.extend(("--features", "markview/diagnostics"))
    result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, text=True)
    executables = set()
    for line in result.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-message":
            print(message["message"]["rendered"], end="", file=sys.stderr)
        if (message.get("reason") == "compiler-artifact"
                and message["profile"]["test"] and message.get("executable")):
            executables.add(Path(message["executable"]))
    result.check_returncode()
    if not executables:
        raise RuntimeError("Cargo produced no test binaries")
    return sorted(executables)


def runnable_tests(binary, environment):
    listings = []
    for flags in (("--list",), ("--list", "--ignored")):
        listed = subprocess.run(
            [str(binary), *TEST_ARGUMENTS, *flags], cwd=ROOT, env=environment,
            capture_output=True, text=True,
        )
        if listed.returncode:
            print(listed.stdout, end="")
            print(listed.stderr, end="", file=sys.stderr)
            listed.check_returncode()
        listings.append({line for line in listed.stdout.splitlines() if line.endswith(": test")})
    return listings[0] - listings[1]


def runtime_environment(binaries, temporary_root):
    environment = dict(os.environ, TMPDIR=str(temporary_root))
    compiler = environment.get("RUSTC", "rustc")
    result = subprocess.run([compiler, "--print", "target-libdir"], cwd=ROOT,
                            check=True, capture_output=True, text=True)
    variable = "DYLD_FALLBACK_LIBRARY_PATH" if sys.platform == "darwin" else "LD_LIBRARY_PATH"
    directories = [result.stdout.strip(), *(str(path) for path in sorted({binary.parent for binary in binaries}))]
    if environment.get(variable):
        directories.append(environment[variable])
    environment[variable] = os.pathsep.join(directories)
    return environment


def run_suite(binaries, temporary_root):
    environment = runtime_environment(binaries, temporary_root)
    targets = [(binary, len(runnable_tests(binary, environment))) for binary in binaries]
    count = sum(size for _, size in targets)
    if not count:
        raise RuntimeError("No runnable tests; refusing an empty path check")
    for binary, size in targets:
        if not size:
            continue
        print(f"Path regression: {binary.name}: {size} runnable tests", flush=True)
        subprocess.run(
            [str(binary), *TEST_ARGUMENTS, "--test-threads=1"],
            cwd=ROOT, env=environment, check=True,
        )
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, action="append",
                        help="reuse a test binary; repeat for multiple targets")
    parser.add_argument("--diagnostics", action="store_true",
                        help="also compile the application's diagnostics tests")
    parser.add_argument("--symlink-only", action="store_true",
                        help="only run the symlink pass after an ordinary CI test run")
    args = parser.parse_args()
    if os.name != "posix":
        parser.error("this regression runner requires Unix symlinks; use native Windows CI on Windows")
    binaries = sorted({path.resolve() for path in args.binary}) if args.binary else build_binaries(args.diagnostics)
    with tempfile.TemporaryDirectory(prefix="markview-path-tests-") as temporary:
        parent = Path(temporary).resolve()
        ordinary = parent / "ordinary"
        ordinary.mkdir()
        linked = parent / "linked"
        linked.symlink_to(ordinary, target_is_directory=True)
        passes = [("symlink", linked)] if args.symlink_only else [("ordinary", ordinary), ("symlink", linked)]
        for label, root in passes:
            start = time.monotonic()
            print(f"Path regression: {label} TMPDIR={root}", flush=True)
            count = run_suite(binaries, root)
            print(f"Path regression: {label}: {count} tests in {time.monotonic() - start:.2f}s", flush=True)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Path regression failed: {error}", file=sys.stderr)
        sys.exit(1)
