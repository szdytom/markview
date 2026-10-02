#!/usr/bin/env python3
"""Replace cargo-dist's download table with the desktop packages."""

import json
import sys
from pathlib import Path

MACOS_NOTE = (
    "**macOS:** Extract the `.app.zip`, drag `Markview.app` to Applications, "
    "then run `xattr -d com.apple.quarantine /Applications/Markview.app` in Terminal "
    "to remove the quarantine flag before opening the app."
)


def render(plan, release):
    app = plan["releases"][0]
    version = app["app_version"]
    hosting = app["hosting"]["github"]
    base_url = hosting["artifact_base_url"] + hosting["artifact_download_path"]
    downloads = [
        (
            f"markview-{version}-aarch64.app.zip",
            "Apple Silicon macOS",
            "markview-macos-SHA256SUMS",
        ),
        (
            "markview-x86_64-pc-windows-msvc.msi",
            "x64 Windows",
            "markview-x86_64-pc-windows-msvc.msi.sha256",
        ),
        (
            "markview-x86_64-pc-windows-msvc.zip",
            "x64 Windows (Portable)",
            "markview-x86_64-pc-windows-msvc.zip.sha256",
        ),
        (
            f"markview-{version}-x86_64.AppImage",
            "x64 Linux",
            "markview-linux-SHA256SUMS",
        ),
    ]
    assets = {asset["name"] for asset in release["assets"]}
    table = ["| File | Platform | Checksum |", "|------|----------|----------|"]
    for name, platform, checksum in downloads:
        for asset in (name, checksum):
            if asset not in assets:
                raise ValueError(f"Release asset is missing: {asset}")
        table.append(
            f"| [{name}]({base_url}/{name}) | {platform} | "
            f"[checksum]({base_url}/{checksum}) |"
        )

    heading = f"## Download {app['app_name']} {version}\n\n"
    before, separator, after = release["body"].partition(heading)
    if not separator:
        raise ValueError(f"Release download heading is missing: {heading.strip()}")
    _, _, suffix = after.partition("\n\n")
    suffix = suffix.removeprefix(MACOS_NOTE + "\n\n")
    return before + heading + "\n".join(table) + "\n\n" + MACOS_NOTE + "\n\n" + suffix


if __name__ == "__main__":
    plan, release = (json.loads(Path(path).read_text()) for path in sys.argv[1:])
    sys.stdout.write(render(plan, release))
