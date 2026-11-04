#!/usr/bin/env python3
"""Measure both implementations with matched executable backing and CPU affinity."""
import argparse
import datetime
import json
import os
import shutil
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "artifacts/webpage-refactor"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_name")
    parser.add_argument("--baseline-checkout", required=True, type=Path)
    parser.add_argument("--cpu", type=int)
    args = parser.parse_args()
    old = args.baseline_checkout.resolve()
    assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=old).decode().strip() == "4328b6246ac6deed0243bff3869fef1dcbc74d2b"
    subprocess.run(["git", "diff", "HEAD", "--exit-code", "--", "crates", "src", "Cargo.toml", "Cargo.lock"], cwd=old, check=True)
    binaries = EVIDENCE / "bin"
    binaries.mkdir(parents=True, exist_ok=True)
    for checkout, names in ((old, ("webpage_baseline", "webpage_baseline_rss")), (ROOT, ("webpage_bench", "webpage_rss", "webpage_replay_rss"))):
        for name in names:
            shutil.copy2(checkout / "target/release/examples" / name, binaries / name)
    run = EVIDENCE / "runs" / args.run_name
    run.mkdir(parents=True, exist_ok=True)
    records = []
    for name, cwd, executable in (("baseline", old, "webpage_baseline"), ("after", ROOT, "webpage_bench")):
        env = dict(os.environ, FONTCONFIG_FILE=str(cwd / "tests/fontconfig.conf"), VK_DRIVER_FILES="/usr/share/vulkan/icd.d/lvp_icd.json", WGPU_BACKEND="vulkan", LP_NUM_THREADS="1")
        command = [str(binaries / executable)]
        if args.cpu is not None:
            command = ["taskset", "-c", str(args.cpu), *command]
        start = datetime.datetime.now(datetime.timezone.utc).isoformat()
        clock = time.monotonic()
        with (run / f"{name}-stdout.json").open("w") as out, (run / f"{name}-stderr.log").open("w") as err:
            result = subprocess.run(command, cwd=cwd, env=env, stdout=out, stderr=err)
        records.append(dict(command=command, cwd=str(cwd), environment={k: env[k] for k in ("FONTCONFIG_FILE", "VK_DRIVER_FILES", "WGPU_BACKEND", "LP_NUM_THREADS")}, cpu_affinity=args.cpu, start_utc=start, elapsed_seconds=time.monotonic() - clock, exit_code=result.returncode))
        (run / "commands.json").write_text(json.dumps(records, indent=2) + "\n")
        result.check_returncode()
        if name == "baseline":
            shutil.copyfile(run / "baseline-stdout.json", EVIDENCE / "baseline.json")
        else:
            after = json.loads((EVIDENCE / "after.json").read_text())
            after["metadata"]["environment"]["cpu_affinity"] = args.cpu
            after["metadata"]["environment"]["executable_backing"] = "both implementations and their RSS probes copied to artifacts/webpage-refactor/bin on the same filesystem"
            (EVIDENCE / "after.json").write_text(json.dumps(after, indent=2) + "\n")
            for file in ("after.json", "replay.json"):
                shutil.copyfile(EVIDENCE / file, run / file)
    shutil.copyfile(EVIDENCE / "baseline.json", run / "baseline.json")


if __name__ == "__main__":
    main()
