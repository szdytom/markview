#!/usr/bin/env python3
"""Fingerprint the webpage refactor inputs and package its uncommitted delivery."""
import argparse
import hashlib
import json
import subprocess
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "artifacts/webpage-refactor"


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def snapshot():
    names = sorted(set(git("ls-files", "--cached", "--others", "--exclude-standard", "-z").decode().split("\0")) - {""})
    inputs = {}
    for name in names:
        path = ROOT / name
        if path.is_file() and not name.endswith(".md") and not name.startswith(("docs/", ".github/", ".agents/")):
            inputs[name] = digest(path.read_bytes())
    identifier = digest(json.dumps(inputs, sort_keys=True, separators=(",", ":")).encode())
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    result = {"source_snapshot_sha256": identifier, "git_head": git("rev-parse", "HEAD").decode().strip(), "inputs": inputs}
    (EVIDENCE / "snapshot.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def bundle():
    record = snapshot()
    (EVIDENCE / "working-tree.patch").write_bytes(git("diff", "--binary", "HEAD"))
    (EVIDENCE / "git-status.txt").write_bytes(git("status", "--short"))
    untracked = git("ls-files", "--others", "--exclude-standard", "-z").decode().split("\0")
    evidence = [ROOT / "artifacts/wp-refactor-remaining-tasks.md", ROOT / "artifacts/webpage-document-refactor-plan.md", ROOT / "artifacts/mvd-problems.md", ROOT / "artifacts/dom-smoothis-coupling.md"]
    evidence += [p for folder in (EVIDENCE, ROOT / "artifacts/android") for p in folder.rglob("*") if p.is_file() and "bin" not in p.relative_to(folder).parts and p.name != "delivery-manifest.json"]
    paths = {ROOT / p for p in untracked if p} | set(evidence)
    paths |= {ROOT / p for p in git("diff", "--name-only", "HEAD").decode().splitlines() if (ROOT / p).is_file()}
    manifest = {str(p.relative_to(ROOT)): digest(p.read_bytes()) for p in sorted(paths) if p.is_file()}
    manifest_path = EVIDENCE / "delivery-manifest.json"
    manifest_path.write_text(json.dumps({"source_snapshot_sha256": record["source_snapshot_sha256"], "files": manifest}, indent=2) + "\n")
    paths.add(manifest_path)
    destination = ROOT / "artifacts/wp-refactor-delivery.tar.gz"
    with tarfile.open(destination, "w:gz") as archive:
        for path in sorted(paths):
            archive.add(path, arcname=path.relative_to(ROOT), recursive=False)
    print(json.dumps({"bundle": str(destination), "sha256": digest(destination.read_bytes()), "files": len(paths), "source_snapshot_sha256": record["source_snapshot_sha256"]}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", action="store_true")
    args = parser.parse_args()
    if args.bundle:
        bundle()
    else:
        print(snapshot()["source_snapshot_sha256"])
