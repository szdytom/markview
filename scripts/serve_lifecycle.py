#!/usr/bin/env python3
"""Verify EOF and killed-host cleanup using only this probe's engine PIDs."""
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def main():
    binary = str(pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/markview").resolve())
    with tempfile.TemporaryDirectory() as state:
        command = [binary, "serve", "--offline", "--state-dir", state]
        for label in ["quit", "empty-input"]:
            with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL) as engine:
                if label == "quit":
                    time.sleep(0.3)
                    assert engine.poll() is None, "engine must start before testing EOF"
                engine.stdin.close()
                assert engine.wait(timeout=5) == 0
                print(f"{label}: exited")
        script = (
            "import subprocess,time; "
            f"p=subprocess.Popen({command!r}, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL); "
            "print(p.pid, flush=True); time.sleep(60)"
        )
        with subprocess.Popen([sys.executable, "-c", script], stdout=subprocess.PIPE, text=True) as host:
            pid = int(host.stdout.readline())
            try:
                time.sleep(0.3)
                assert alive(pid), "engine must be alive before killing the host"
                host.kill()
                host.wait(timeout=5)
                deadline = time.monotonic() + 5
                while alive(pid) and time.monotonic() < deadline:
                    time.sleep(0.05)
                assert not alive(pid), "engine survived its host"
                print("killed-host: exited")
            finally:
                if alive(pid):
                    os.kill(pid, signal.SIGKILL)
                if host.poll() is None:
                    host.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
