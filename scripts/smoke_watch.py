#!/usr/bin/env python3
"""Linux desktop end-to-end reload check; edits only a temporary document.

Starts one native window, measures stable write -> completed GPU frame, then
terminates only that process. Run with access to a real desktop/GPU session.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", nargs="?", default="target/release/markview")
    parser.add_argument("--output", default="artifacts/watch-smoke.json")
    args = parser.parse_args()
    frames = queue.Queue()
    errors = []
    with tempfile.TemporaryDirectory(prefix="markview-watch-") as tmp:
        path = Path(tmp) / "document.md"
        text = "# Live reading\n\n中文段落与 $x^2+y^2$。\n\n"
        path.write_text(text, encoding="utf-8")
        env = {**os.environ, "RUST_LOG": "markview=debug"}
        process = subprocess.Popen(
            [str(Path(args.binary).resolve()), str(path)],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
            env=env,
        )

        def collect():
            for line in process.stderr:
                if "open→GPU complete:" in line:
                    frames.put((time.perf_counter(), line.strip()))
                elif "Error:" in line or "failed" in line.lower():
                    errors.append(line.strip())

        reader = threading.Thread(target=collect, daemon=True)
        reader.start()
        try:
            frames.get(timeout=30)
            samples = []
            for i in range(12):
                # No previous stable write should produce a delayed second frame.
                time.sleep(0.04)
                while not frames.empty():
                    frames.get_nowait()
                text += f"Paragraph {i}: an updated mathematical observation.\n\n"
                start = time.perf_counter()
                if i % 3 == 0:
                    temp = path.with_suffix(".tmp")
                    temp.write_text(text, encoding="utf-8")
                    os.replace(temp, path)
                    kind = "atomic-replace"
                elif i % 3 == 1:
                    path.write_text(text, encoding="utf-8")
                    kind = "in-place"
                else:
                    path.unlink()
                    path.write_text(text, encoding="utf-8")
                    kind = "delete-recreate"
                finished, log = frames.get(timeout=5)
                samples.append({"kind": kind, "write_to_gpu_ms": (finished - start) * 1000, "frame": log})

            time.sleep(0.05)
            while not frames.empty():
                frames.get_nowait()
            start = time.perf_counter()
            for i in range(40):
                with path.open("a", encoding="utf-8") as file:
                    file.write(f"streaming {i} ")
                time.sleep(0.01)
            writing_finished = time.perf_counter()
            time.sleep(0.25)
            streamed = []
            while not frames.empty():
                finished, log = frames.get_nowait()
                streamed.append({"after_start_ms": (finished - start) * 1000, "during_write": finished < writing_finished, "frame": log})
            assert any(s["during_write"] for s in streamed), "Continuous writes starved the reader"
            assert not errors, errors
            values = sorted(s["write_to_gpu_ms"] for s in samples)
            report = {
                "scope": "Actual native window; local writes including syscall time through logged completed GPU frame; compositor presentation excluded.",
                "samples": samples,
                "p50_ms": values[len(values) // 2],
                "p95_ms": values[-1],  # nearest rank for 12 samples
                "continuous_write_frames": streamed,
            }
            output = Path(args.output)
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
            print(json.dumps({"p50_ms": report["p50_ms"], "p95_ms": report["p95_ms"], "streamed_frames": len(streamed), "output": str(output)}, ensure_ascii=False))
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            reader.join(timeout=1)


if __name__ == "__main__":
    main()
