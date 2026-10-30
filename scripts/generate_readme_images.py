#!/usr/bin/env python3
"""Capture real reader windows and compose the localized README images.

Requires Pillow. Desktop capture also needs
Spectacle, a graphical desktop and a release binary; Android capture needs a
configured emulator with Markview installed. Omit capture flags to recompose.
"""

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs/screenshots/readme"
THEMES = ("light", "dark", "celadon", "rosewood", "blueprint", "8-bit")


def save(canvas, name):
    canvas.save(OUTPUT / name, optimize=True)


def capture_profile(directory, language, folder=False):
    current = (
        Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / "markview"
    )
    profile = directory / "markview"
    profile.mkdir()
    settings = (current / "settings.toml").read_text()
    overrides = {
        "single-instance": str(folder).lower(),
        "restore-session": str(folder).lower(),
        "tab-style": '"connected"',
        "window-layout": '"windows"',
        "language": '"en"' if language == "en" else '"zh-hans"',
        "markdown-flavor": '"obsidian"',
        "footer-path": "false",
    }
    settings = re.sub(rf"(?m)^({'|'.join(overrides)})\s*=.*\n?", "", settings)
    (profile / "settings.toml").write_text(
        "".join(f"{key} = {value}\n" for key, value in overrides.items()) + settings
    )
    for resource in ("fonts", "styles"):
        if (current / resource).is_dir():
            (profile / resource).symlink_to(
                current / resource, target_is_directory=True
            )
    if folder:
        root = (
            ROOT
            / f"docs/screenshots/source/{language}"
            / ("Field notes" if language == "en" else "阅读笔记")
        )
        names = (
            ("Welcome.md", "Reading list.md", "Connected ideas.md")
            if language == "en"
            else ("从这里开始.md", "阅读清单.md", "让笔记连成一片.md")
        )
        position = dict(
            block=0, occurrence=0, source=0, node=None, byte=0, local=0, fallback=0
        )
        session = dict(
            version=1,
            folder=str(root),
            folder_open=True,
            active=2,
            tabs=[
                dict(
                    path=str(root / name),
                    reading=dict(content=0, position=position, details={}),
                )
                for name in names
            ],
        )
        (profile / "session.json").write_text(json.dumps(session))


def capture(binary):
    for language in ("en", "zh"):
        source = ROOT / f"docs/screenshots/source/{language}/reading.md"
        views = [(theme, 1440, theme) for theme in THEMES] + [
            ("light", 1440, "desktop"),
            ("light", 1440, "library"),
        ]
        for theme, width, name in views:
            destination = OUTPUT / f"{language}-{name}.png"
            command = [
                str(binary),
                "--foreground",
                "--offline",
                "--style",
                theme,
                "--width",
                str(width),
                "--height",
                "900",
                "--column",
                "850",
                "--font-size",
                "18" if name == "library" else "20",
            ]
            if name != "library":
                command.append(str(source))
            with tempfile.TemporaryDirectory() as temporary:
                shot_path = Path(temporary) / "window.png"
                capture_profile(Path(temporary), language, folder=name == "library")
                environment = os.environ | {
                    "XDG_CONFIG_HOME": temporary,
                    "XDG_DATA_HOME": temporary,
                }
                with subprocess.Popen(command, env=environment) as reader:
                    try:
                        time.sleep(3)
                        if reader.poll() is not None:
                            raise RuntimeError(
                                f"Reader exited while capturing {destination.name}"
                            )
                        subprocess.run(
                            ["spectacle", "-i", "-b", "-n", "-a", "-o", str(shot_path)],
                            check=True,
                        )
                        for _ in range(100):
                            if shot_path.exists():
                                break
                            time.sleep(0.1)
                        shot = Image.open(shot_path).convert("RGBA")
                        frame = (
                            shot.getchannel("A")
                            .point(lambda alpha: 255 if alpha >= 250 else 0)
                            .getbbox()
                        )
                        shot.crop(frame).save(destination, optimize=True)
                        print(f"Captured {destination.name}", flush=True)
                    finally:
                        reader.terminate()
                        reader.wait()


def surface(size):
    canvas = Image.new("RGBA", size)
    draw = ImageDraw.Draw(canvas)
    for y in range(size[1]):
        tone = round(245 - 12 * (y / size[1]) ** 2)
        draw.line((0, y, size[0], y), fill=(tone, tone + 1, tone + 2))
    return canvas


def shadow(canvas, box, radius, blur=24):
    layer = Image.new("RGBA", canvas.size)
    x, y, right, bottom = box
    ImageDraw.Draw(layer).rounded_rectangle(
        (x, y + 18, right, bottom + 18), radius, fill=(20, 28, 38, 65)
    )
    canvas.alpha_composite(layer.filter(ImageFilter.GaussianBlur(blur)))


def device(canvas, screenshot, position, width, kind):
    x, y = position
    screen = screenshot.convert("RGB").resize(
        (width, round(width * screenshot.height / screenshot.width)),
        Image.Resampling.LANCZOS,
    )
    bezel = 18 if kind == "phone" else 24
    radius = 46 if kind == "phone" else 28 if kind == "tablet" else 12
    box = (x, y, x + width + bezel * 2, y + screen.height + bezel * 2)
    shadow(canvas, box, radius)
    draw = ImageDraw.Draw(canvas)
    draw.rounded_rectangle(box, radius, fill="#92969C", outline="#C6C9CD", width=2)
    draw.rounded_rectangle(
        (x + 3, y + 3, box[2] - 3, box[3] - 3), radius - 2, fill="#25272B"
    )
    draw.rounded_rectangle(
        (x + 7, y + 7, box[2] - 7, box[3] - 7), radius - 5, fill="#101215"
    )
    mask = Image.new("L", screen.size)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, screen.width, screen.height), max(2, radius - bezel), fill=255
    )
    canvas.paste(screen, (x + bezel, y + bezel), mask)
    if kind == "tablet":
        draw.ellipse(
            (x + width // 2 + bezel - 3, y + 8, x + width // 2 + bezel + 3, y + 14),
            fill="#48505B",
        )


def hero(language):
    canvas = surface((2400, 1540))
    desktop = Image.open(OUTPUT / f"{language}-desktop.png")
    tablet = Image.open(OUTPUT / f"tablet-{language}.png")
    phone = Image.open(OUTPUT / f"android-{language}.png")
    # The stand sits behind the three real screen captures.
    shadow(canvas, (850, 1230, 1590, 1270), 35, 30)
    draw = ImageDraw.Draw(canvas)
    draw.polygon([(1170, 980), (1290, 980), (1350, 1220), (1090, 1220)], fill="#666B73")
    draw.polygon([(1190, 980), (1270, 980), (1300, 1220), (1160, 1220)], fill="#868B93")
    draw.rounded_rectangle(
        (900, 1210, 1540, 1248), 16, fill="#8C9097", outline="#BABEC4", width=3
    )
    device(canvas, desktop, (500, 70), 1400, "monitor")
    device(canvas, tablet, (1220, 760), 960, "tablet")
    device(canvas, phone, (290, 610), 330, "phone")
    save(canvas.convert("RGB"), f"{language}-hero.png")


def compose(language):
    desktop = Image.open(OUTPUT / f"{language}-light.png")
    hero(language)

    light = desktop.convert("RGB")
    dark = Image.open(OUTPUT / f"{language}-dark.png").convert("RGB")
    width, height = light.size
    mask = Image.new("L", light.size)
    ImageDraw.Draw(mask).polygon(
        [
            (round(width * 0.65), 0),
            (width, 0),
            (width, height),
            (round(width * 0.35), height),
        ],
        fill=255,
    )
    light.paste(dark, (0, 0), mask)
    save(light, f"{language}-themes.png")


def capture_android(serial, kind="android"):
    adb = ["adb", "-s", serial]
    for language in ("en", "zh"):
        source = (ROOT / f"docs/screenshots/source/{language}/reading.md").read_text()
        command = shlex.join(
            [
                "am",
                "start",
                "-a",
                "android.intent.action.SEND",
                "-t",
                "text/plain",
                "-n",
                "io.github.szdytom.markview/.ReadInMarkview",
                "--es",
                "android.intent.extra.TEXT",
                source,
            ]
        )
        subprocess.run(adb + ["shell", command], check=True)
        time.sleep(3)
        subprocess.run(adb + ["shell", "input", "keyevent", "122"], check=True)
        time.sleep(1)
        with (OUTPUT / f"{kind}-{language}.png").open("wb") as output:
            subprocess.run(
                adb + ["exec-out", "screencap", "-p"], stdout=output, check=True
            )


def main():
    global OUTPUT
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture", action="store_true")
    parser.add_argument(
        "--android-serial", help="Capture both languages on this emulator"
    )
    parser.add_argument(
        "--tablet-serial", help="Capture both languages on a landscape tablet emulator"
    )
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/markview")
    parser.add_argument(
        "--android-en", type=Path, help="English Android reader screenshot"
    )
    parser.add_argument(
        "--android-zh", type=Path, help="Chinese Android reader screenshot"
    )
    parser.add_argument("--output-directory", type=Path, default=OUTPUT)
    args = parser.parse_args()
    OUTPUT = args.output_directory
    OUTPUT.mkdir(parents=True, exist_ok=True)
    for language in ("en", "zh"):
        android = getattr(args, f"android_{language}")
        if android:
            shutil.copyfile(android, OUTPUT / f"android-{language}.png")
    if args.capture:
        capture(args.binary.resolve())
    if args.android_serial:
        capture_android(args.android_serial)
    if args.tablet_serial:
        capture_android(args.tablet_serial, "tablet")
    for language in ("en", "zh"):
        compose(language)


if __name__ == "__main__":
    main()
