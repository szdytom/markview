#!/usr/bin/env python3
"""Prepare or build the WASM engine used by the TypeScript workspace."""

import argparse
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGET = "wasm32-unknown-unknown"
OUTPUT = ROOT / "web/packages/markview/wasm"


def run(*args, capture=False):
    return subprocess.run(
        [str(arg) for arg in args],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else None,
    ).stdout


def matches(cli, version):
    result = subprocess.run(
        [str(cli), "--version"],
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode == 0 and result.stdout.strip() == f"wasm-bindgen {version}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--setup",
        action="store_true",
        help="install the WASM target and matching bindings tool",
    )
    args = parser.parse_args()
    for tool in ("cargo", "rustup"):
        if not shutil.which(tool):
            raise SystemExit(
                f"Missing {tool}. Install Rust with rustup: https://rustup.rs, then run pnpm --dir web run setup."
            )

    manifest = tomllib.loads(
        (ROOT / "crates/markview-web/Cargo.toml").read_text(encoding="utf-8")
    )
    version = manifest["target"]['cfg(target_arch = "wasm32")']["dependencies"][
        "wasm-bindgen"
    ].removeprefix("=")
    tool_root = ROOT / ".tools" / f"wasm-bindgen-{version}"
    executable = "wasm-bindgen.exe" if os.name == "nt" else "wasm-bindgen"
    override = os.environ.get("WASM_BINDGEN")
    if override:
        cli = Path(shutil.which(override) or override).resolve()
        if not cli.is_file() or not matches(cli, version):
            raise SystemExit(
                f"WASM_BINDGEN must point to wasm-bindgen {version}: {cli}"
            )
    else:
        candidates = [tool_root / "bin" / executable, tool_root / executable]
        if found := shutil.which("wasm-bindgen"):
            candidates.append(Path(found))
        cli = next(
            (path for path in candidates if path.is_file() and matches(path, version)),
            None,
        )

    if args.setup:
        print("Installing the Rust target for browser builds...", flush=True)
        run("rustup", "target", "add", TARGET)
        if cli is None:
            print(
                f"Installing wasm-bindgen {version} locally (first install can take several minutes)...",
                flush=True,
            )
            run(
                "cargo",
                "install",
                "wasm-bindgen-cli",
                "--version",
                version,
                "--locked",
                "--root",
                tool_root,
                "--force",
            )
        print("WASM tools are ready. Run pnpm --dir web build.")
        return

    if cli is None:
        raise SystemExit(
            f"Missing wasm-bindgen {version}. Run pnpm --dir web run setup."
        )
    if (
        TARGET
        not in run("rustup", "target", "list", "--installed", capture=True).splitlines()
    ):
        raise SystemExit(
            "Missing the browser Rust target. Run pnpm --dir web run setup."
        )

    print("Building the WASM engine (Cargo reuses unchanged code)...", flush=True)
    run(
        "cargo",
        "build",
        "-p",
        "markview-web",
        "--target",
        TARGET,
        "--release",
        "--features",
        "woff",
    )
    metadata = json.loads(
        run("cargo", "metadata", "--no-deps", "--format-version", "1", capture=True)
    )
    wasm = Path(metadata["target_directory"]) / TARGET / "release/markview_web.wasm"
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    # Keep existing bindings until the new generation succeeds.
    with tempfile.TemporaryDirectory(dir=OUTPUT.parent) as temporary:
        run(
            cli,
            "--target",
            "web",
            "--out-dir",
            temporary,
            "--out-name",
            "markview_web",
            wasm,
        )
        shutil.rmtree(OUTPUT, ignore_errors=True)
        shutil.move(temporary, OUTPUT)
    size = (OUTPUT / "markview_web_bg.wasm").stat().st_size / (1024 * 1024)
    print(f"Built {OUTPUT.relative_to(ROOT)} ({size:.1f} MiB)")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode) from None
