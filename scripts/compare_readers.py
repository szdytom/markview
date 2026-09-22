#!/usr/bin/env python3
"""Measure what a reader costs to put a document on screen, and to update it.

Every application is started the same way: one command, one file, a cold process
on the X server named by `--display`, and its window is captured directly through
Xlib from before the launch until the document has settled. "The document is on
screen" is defined against the frame the application settles on by itself, so the
definition does not depend on the toolkit: the reported time is the first frame
that agrees with the settled frame, and every frame carries the time it was
taken, so the launch and the frames share one clock.

Why the window and not the screen: under a Wayland compositor the X root window
holds nothing, so a screen grab is blank. Why not Xvfb or Xephyr: Markview renders
through Vulkan, which needs DRI3 to present, and neither server provides it to
clients. The applications therefore run on a real X server, and only the top of
each window is read, which is the region a reader sees first.

The applications are Markview, MarkText, SuperGoodViewer and VS Code. MarkText
and SuperGoodViewer render the document in their window; VS Code is asked for
its Markdown preview, because its editor shows the source rather than the
document. Every application is also weighed once its document has settled: the
resident memory of all of its processes together, because an Electron reader is
several processes and the number a person would read off a monitor is their sum.

Usage: scripts/compare_readers.py [--task open,edit] [--runs 3]
                                  [--fixtures 10k,100k]
                                  [--apps markview,marktext,supergoodviewer,vscode]
                                  [--json FILE]
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import signal
import statistics
import subprocess
import sys
import time

import numpy as np
from Xlib import X, display as xdisplay

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import comparison_fixtures

ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/markview"
# SuperGoodViewer ships prebuilt Linux archives; unpack one here and point
# `SUPERGOODVIEWER` at its executable to include it in a comparison.
SGV = pathlib.Path(os.environ.get("SUPERGOODVIEWER",
                                  ROOT / "artifacts/supergoodviewer/supergoodviewer"))
WINDOW_ROWS = 400          # physical rows of the window that are captured
STRIDE = 8                 # analysis downsample factor
SETTLE_QUIET = 1.5         # long enough to outlast a second rendering pass
SETTLE_LIMIT = 40.0        # a reader that never settles is reported, not hidden
INK_FLOOR = 0.01           # below this the window is loading, not showing a document
POST_EDIT_WAIT = 15.0      # keep watching this long after the write, quiet or not
TOLERANCE = 24
WORK = pathlib.Path(os.environ.get("MARKVIEW_COMPARE_WORK",
                                   "/tmp/markview-compare"))


def find_window(display, needle, deadline):
    """Poll the window tree for a window whose title contains `needle`."""
    root = display.screen().root
    while time.time() < deadline:
        try:
            for window in root.query_tree().children:
                title = window_title(display, window)
                if title and needle in title:
                    return window
        except Exception:
            pass
        time.sleep(0.02)
    return None


def window_title(display, window):
    """The window's title: Electron spells it `_NET_WM_NAME` and nothing else."""
    try:
        name = window.get_wm_name()
        if name:
            return name
    except Exception:
        pass
    try:
        value = window.get_full_property(display.intern_atom("_NET_WM_NAME"),
                                         display.intern_atom("UTF8_STRING"))
        return value.value.decode() if value else None
    except Exception:
        return None


def process_field(entry, index):
    """One numeric field of a `/proc` status line, or `None` when unreadable.

    The fields after the process name are `state`, `ppid`, `pgrp` and `session`,
    which is how a process is placed in the group or session it belongs to.
    """
    try:
        fields = (entry / "stat").read_bytes().decode(errors="replace")
        return int(fields[fields.rindex(")") + 2:].split()[index])
    except (OSError, ValueError, IndexError):
        return None


def sweep(work):
    """Kill anything still running out of this run's private directories."""
    ours = str(work).encode()
    own_session = os.getsid(0)
    for entry in pathlib.Path("/proc").iterdir():
        if not entry.name.isdigit() or entry.name == str(os.getpid()):
            continue
        try:
            command = (entry / "cmdline").read_bytes()
        except OSError:
            continue
        if ours not in command:
            continue
        # The path can also appear in the command line of the shell that started
        # this run, so only ever kill processes in a session of their own. Both
        # applications are launched with `start_new_session=True`.
        if process_field(entry, 3) == own_session:
            continue
        try:
            os.kill(int(entry.name), signal.SIGKILL)
        except OSError:
            pass


def group_rss_kib(pgid):
    """Resident memory of every process in the application's process group.

    An Electron reader is four to eight processes, so the number a person would
    read off a system monitor is their sum. Each application is started with
    `start_new_session=True`, so its group is the process that was launched.
    """
    total = 0
    for entry in pathlib.Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        if process_field(entry, 2) != pgid:
            continue
        try:
            for line in (entry / "status").read_text().splitlines():
                if line.startswith("VmRSS:"):
                    total += int(line.split()[1])
        except (OSError, ValueError, IndexError):
            continue
    return total


def grab(window, rows):
    """One window frame as a small grayscale array."""
    geometry = window.get_geometry()
    image = window.get_image(0, 0, geometry.width, rows, X.ZPixmap, 0xFFFFFFFF)
    pixels = np.frombuffer(image.data, dtype=np.uint8)
    frame = pixels.reshape(rows, geometry.width, 4)[::STRIDE, ::STRIDE, :3]
    return frame.mean(axis=2).astype(np.uint8)


def ink(frame):
    """Fraction of the window that is not its own background.

    Used to tell a document from the loading state that precedes it, because a
    loading page is perfectly still: MarkText holds a static page for two seconds
    before its document arrives, with 0.2 per cent ink against 4.8 per cent
    afterwards.
    """
    return float((np.abs(frame.astype(np.int16) - np.median(frame)) > TOLERANCE)
                 .mean())


def record(window, rows, quiet=SETTLE_QUIET, limit=SETTLE_LIMIT, minimum=0.0):
    """Capture frames until a document has been quiet for `quiet` seconds.

    A frame is stamped with the middle of its own transfer, because the pixels
    are what the window held while the transfer ran, not what it held before or
    after it.

    Quiet is not the same as finished, so two conditions have to hold: the window
    must be unchanged, and it must carry at least `INK_FLOOR` of ink, which a
    loading page does not. The caller is told when the limit was reached instead,
    so that a run which never settled is reported as such rather than mistaken
    for a finished one.
    """
    frames, previous, quiet_since = [], None, None
    started = time.perf_counter()
    deadline = started + limit
    while time.perf_counter() < deadline:
        start = time.perf_counter()
        try:
            frame = grab(window, rows)
        except Exception:
            time.sleep(0.005)
            continue
        taken = (start + time.perf_counter()) / 2
        frames.append((taken, frame))
        still = previous is not None and frame.shape == previous.shape and \
            np.abs(frame.astype(np.int16) - previous.astype(np.int16)).mean() <= 1.0
        if still and ink(frame) >= INK_FLOOR:
            if quiet_since is None:
                quiet_since = taken
            elif taken - quiet_since > quiet and taken - started > minimum:
                return frames, False
        else:
            quiet_since = None
        previous = frame
    return frames, True


def content_score(stack, reference):
    """How much of the settled document's ink is on screen in each frame.

    Agreement over the whole window is useless here: a page is mostly background,
    and a loading page has the same background as a finished one, so every frame
    would "agree" from the moment the window was painted. The score is therefore
    taken over the pixels where the settled page actually has ink, which is the
    part a reader is waiting for.
    """
    background = np.median(reference)
    mask = np.abs(reference.astype(np.int16) - background) > TOLERANCE
    if not mask.any():
        return np.zeros(len(stack))
    present = (np.abs(stack.astype(np.int16) - reference.astype(np.int16))
               <= TOLERANCE)
    return present[:, mask].mean(axis=1)


def first_after(times, score, threshold, start):
    for index in range(start, len(times)):
        if score[index] >= threshold:
            return float(times[index])
    return None


def launch(app, document, log):
    environment = dict(os.environ)
    environment.pop("WAYLAND_DISPLAY", None)
    environment["DISPLAY"] = app["display"]
    for key, value in app.get("env", {}).items():
        environment[key] = str(value).format(work=WORK)
    for directory in app.get("env_dirs", []):
        pathlib.Path(str(directory).format(work=WORK)).mkdir(parents=True,
                                                             exist_ok=True)
    handle = open(log, "wb")
    return subprocess.Popen(app["command"](document), env=environment,
                            stdout=handle, stderr=handle, start_new_session=True)


def stop(process):
    """Close the application. The frames are already captured, so there is
    nothing to wait for: an Electron application takes seconds to shut down
    politely and none of that time is being measured."""
    if process.poll() is None:
        try:
            os.killpg(os.getpgid(process.pid), signal.SIGKILL)
        except ProcessLookupError:
            return
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass


def send_keys(display, window, keys):
    """Focus the window, then type `keys` with xdotool's XTEST client."""
    window.set_input_focus(X.RevertToParent, X.CurrentTime)
    display.sync()
    subprocess.run(["xdotool", "key", "--clearmodifiers", keys],
                   env={**os.environ, "DISPLAY": os.environ["DISPLAY"]},
                   capture_output=True)


def edit_document(path):
    """Change one character at the very top of the document."""
    path.write_text(path.read_text().replace("# A comparison document",
                                             "# A comparison document!", 1))


def one_run(app, fixture, work, index, tasks):
    document = work / f"doc-{app['name']}-{fixture.stem}-{index}.md"
    log_path = work / f"log-{app['name']}-{fixture.stem}-{index}.log"
    shutil.copy(fixture, document)
    display = xdisplay.Display(app["display"])
    marks = {"launch": time.perf_counter()}
    print(f"    {app['name']} started", flush=True)
    application = launch(app, document, log_path)
    try:
        needle = app["title"].format(document=document.name, stem=document.stem)
        window = find_window(display, needle, time.time() + 40)
        if window is None:
            raise RuntimeError("no window appeared")
        print(f"    window after {time.perf_counter() - marks['launch']:.2f}s",
              flush=True)
        if app.get("open_preview"):
            send_keys(display, window, app["open_preview"])
        rows = min(WINDOW_ROWS, window.get_geometry().height)
        watch_started = time.perf_counter()
        frames, timed_out = record(window, rows)
        print(f"    settled after {time.perf_counter() - marks['launch']:.2f}s: "
              f"{len(frames)} frames in {time.perf_counter() - watch_started:.2f}s"
              + ("  [HIT THE LIMIT: never settled]" if timed_out else ""),
              flush=True)
        marks["rss_kib"] = group_rss_kib(application.pid)
        if "edit" in tasks:
            time.sleep(0.5)
            marks["edit"] = time.perf_counter()
            edit_document(document)
            more, _ = record(window, rows, minimum=POST_EDIT_WAIT)
            frames += more
    finally:
        closing = time.perf_counter()
        stop(application)
        sweep(work)
        display.close()
        print(f"    closed in {time.perf_counter() - closing:.2f}s", flush=True)
    if os.environ.get("MARKVIEW_COMPARE_DUMP"):
        from PIL import Image
        stack = np.stack([f for _, f in frames])
        changed = np.abs(np.diff(stack.astype(np.int16), axis=0)).mean(axis=(1, 2))
        moved = np.flatnonzero(changed > 0.5)
        settled = int(moved[-1]) + 1 if moved.size else 0
        Image.fromarray(stack[settled]).resize(
            (stack.shape[2] * 2, stack.shape[1] * 2), Image.NEAREST).save(
            os.environ["MARKVIEW_COMPARE_DUMP"])
    result = analyze(frames, marks, tasks)
    # A reader that refuses the document still paints a still, inked window, so
    # its own log is what says whether the frame is a page or an error message.
    if pattern := app.get("failure"):
        match = re.search(pattern, log_path.read_text(errors="replace"))
        if match:
            return {"error": f"{app['name']}: {match.group(1).strip()[:160]}",
                    "rss_mib": result.get("rss_mib")}
    return result


def analyze(frames, marks, tasks):
    times = np.array([t for t, _ in frames])
    stack = np.stack([f for _, f in frames])
    changed = np.abs(np.diff(stack.astype(np.int16), axis=0)).mean(axis=(1, 2))
    moved = np.flatnonzero(changed > 0.5)
    settled = int(moved[-1]) + 1 if moved.size else 0
    result = {"frames": len(frames),
              "settled_s": round(times[settled] - marks["launch"], 3),
              "reference_ink": round(ink(stack[settled]), 4)}
    if "rss_kib" in marks:
        result["rss_mib"] = round(marks["rss_kib"] / 1024, 1)
    if "open" in tasks:
        before = int(np.searchsorted(times, marks.get("edit", times[-1])))
        moved_before = np.flatnonzero(changed[:max(before - 1, 1)] > 0.5)
        opened = int(moved_before[-1]) + 1 if moved_before.size else 0
        score = content_score(stack, stack[opened])
        start = int(np.searchsorted(times, marks["launch"]))
        for name, threshold in (("first_s", 0.5), ("complete_s", 0.95)):
            when = first_after(times, score, threshold, start)
            result[name] = None if when is None else round(when - marks["launch"], 3)
    if "edit" in tasks:
        # One character is far too small for an agreement threshold, so the
        # change is looked for directly: the frames after the write are compared
        # with the last frame before it, over the first lines of the document.
        # The very top of the window is toolbar, not text, so the band starts
        # below it and is wide enough to hold the first heading.
        before = max(int(np.searchsorted(times, marks["edit"])) - 3, 0)
        after_edit = int(np.searchsorted(times, marks["edit"]))
        rows = stack.shape[1]
        lines = slice(max(rows // 12, 2), max(rows // 3, 3))
        base = stack[before][lines].astype(np.int16)
        moved = (np.abs(stack[:, lines].astype(np.int16) - base) > TOLERANCE)
        changed = moved.mean(axis=(1, 2))
        # The threshold is the application's own jitter, tripled: a caret blinking
        # moves a pixel or two, a rewritten heading moves a good many more.
        noise = float(changed[:after_edit].max()) if after_edit else 0.0
        threshold = max(3 * noise, 0.0002)
        when = None
        for index in range(after_edit, len(times)):
            if changed[index] > threshold:
                when = float(times[index])
                break
        result["edit_s"] = None if when is None else round(when - marks["edit"], 3)
        result["edit_threshold"] = round(threshold, 5)
    return result


def applications(display):
    def markview(document):
        return [str(BINARY), str(document)]

    def marktext(document):
        return ["marktext", "--no-sandbox", "--ozone-platform=x11", str(document)]

    def vscode(document):
        return ["code", "--ozone-platform=x11", "--disable-extensions",
                "--disable-workspace-trust", "--new-window",
                f"--user-data-dir={WORK}/vscode",
                f"--extensions-dir={WORK}/vscode-ext", str(document)]

    def supergoodviewer(document):
        return [str(SGV), str(document)]

    applications = {
        "markview": {"name": "markview", "command": markview, "display": display,
                     "title": "{document}",
                     "env": {"XDG_CONFIG_HOME": "{work}/config"}},
        "marktext": {"name": "marktext", "command": marktext, "display": display,
                     "title": "{document}",
                     "env": {"XDG_CONFIG_HOME": "{work}/config"}},
        "supergoodviewer": {"name": "supergoodviewer", "command": supergoodviewer,
                            "display": display,
                            "title": "{stem} - SuperGoodViewer",
                            # Its compiled-document cache lives under `$HOME`, so
                            # a private `HOME` keeps every run a first open. A
                            # repeat open of the same file is served from that
                            # cache instead, which is a different measurement.
                            "env": {"GDK_BACKEND": "x11", "HOME": "{work}/home"},
                            "env_dirs": ["{work}/home"],
                            # Its LaTeX-to-Typst path (mitex) covers the matrix
                            # environments since 1.0.8, so the mathematics
                            # fixtures are rendered rather than skipped. A
                            # compile failure is still caught below.
                            "failure": r"compileDocument: FAILED \(([^)]*)\)"},
        "vscode": {"name": "vscode", "command": vscode, "display": display,
                   "title": "Visual Studio Code",
                   "open_preview": "ctrl+shift+v"},
    }
    if not SGV.exists():
        applications.pop("supergoodviewer")
        print(f"skipping supergoodviewer: {SGV} not found", file=sys.stderr)
    return applications


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--task", default="open", help="comma-separated: open,edit")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--fixtures", default="10k")
    parser.add_argument("--apps", default="markview,marktext,supergoodviewer")
    parser.add_argument("--display", default=":0")
    parser.add_argument("--json", help="write the raw report here")
    parser.add_argument("--save-settled",
                        help="directory to write each settled frame into")
    args = parser.parse_args()
    tasks = tuple(args.task.split(","))
    os.environ["DISPLAY"] = args.display
    if not BINARY.exists():
        sys.exit(f"build {BINARY} first: cargo build --release")
    WORK.mkdir(parents=True, exist_ok=True)
    fixtures = comparison_fixtures.write(WORK / "fixtures",
                                         tuple(args.fixtures.split(",")))
    available = applications(args.display)
    unknown = [name for name in args.apps.split(",") if name not in available]
    if unknown:
        sys.exit(f"unknown app(s) {', '.join(unknown)}; "
                 f"available: {', '.join(sorted(available))}")
    chosen = [available[name] for name in args.apps.split(",")]
    sweep(WORK)
    report = {}
    # Interleaved, so every application meets the same machine load.
    for index in range(args.runs):
        for app in chosen:
            for fixture in fixtures:
                key = f"{app['name']}|{fixture.stem}"
                print(f"run {index + 1}/{args.runs}: {key}", flush=True)
                # A reader that cannot parse a fixture is recorded, not run: its
                # window stays in a compile-error state and never settles.
                if fixture.stem in app.get("skip", ()):
                    report.setdefault(key, []).append(
                        {"error": f"{app['name']} {app['skip_reason']}"})
                    print(f"    skipped: {app['skip_reason']}", flush=True)
                    continue
                try:
                    if args.save_settled:
                        pathlib.Path(args.save_settled).mkdir(parents=True,
                                                              exist_ok=True)
                        os.environ["MARKVIEW_COMPARE_DUMP"] = str(
                            pathlib.Path(args.save_settled)
                            / f"{app['name']}-{fixture.stem}-{index}.png")
                    result = one_run(app, fixture, WORK, index, tasks)
                except Exception as error:  # a broken run is data, not a crash
                    result = {"error": str(error)}
                report.setdefault(key, []).append(result)
                print(f"    {result}", flush=True)
    summary = {}
    for key, results in report.items():
        entry = {"runs": results}
        for field in ("first_s", "complete_s", "edit_s", "rss_mib"):
            values = [r[field] for r in results if r.get(field) is not None]
            if values:
                entry[f"{field}_median"] = round(statistics.median(values), 3)
        summary[key] = entry
    print()
    for key, entry in sorted(summary.items()):
        fields = " ".join(f"{name}={entry.get(name + '_median')}"
                          for name in ("first_s", "complete_s", "edit_s",
                                       "rss_mib")
                          if entry.get(name + "_median") is not None)
        print(f"{key:20} {fields}")
    if args.json:
        pathlib.Path(args.json).write_text(json.dumps(summary, indent=2) + "\n")
        print(f"wrote {args.json}")


if __name__ == "__main__":
    main()
