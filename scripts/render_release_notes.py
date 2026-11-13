#!/usr/bin/env python3
"""Replace cargo-dist's download table with the installable packages."""

import json
import sys
from pathlib import Path

MACOS_NOTE = (
    "**macOS:** Extract the `.app.zip`, drag `Markview.app` to Applications, "
    "then run `xattr -d com.apple.quarantine /Applications/Markview.app` in Terminal "
    "to remove the quarantine flag before opening the app."
)
VSCODE_NOTE = (
    "**VS Code:** Install Markview Integration from the "
    "[Marketplace](https://marketplace.visualstudio.com/items?itemName=Stevvven.markview-export), "
    "or download the `.vsix` for your platform and choose **Extensions → "
    "Install from VSIX…** in VS Code. The exporter is included."
)
VSCODE_PLATFORMS = (
    ("darwin-arm64", "VS Code: Apple Silicon macOS"),
    ("win32-x64", "VS Code: x64 Windows"),
    ("win32-arm64", "VS Code: ARM64 Windows"),
    ("linux-x64", "VS Code: x64 Linux"),
    ("linux-arm64", "VS Code: ARM64 Linux"),
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
        ),
        (
            "markview-x86_64-pc-windows-msvc.msi",
            "x64 Windows",
        ),
        (
            "markview-x86_64-pc-windows-msvc.zip",
            "x64 Windows (Portable)",
        ),
        (
            "markview-aarch64-pc-windows-msvc.msi",
            "ARM64 Windows",
        ),
        (
            "markview-aarch64-pc-windows-msvc.zip",
            "ARM64 Windows (Portable)",
        ),
        (
            f"markview-{version}-x86_64.AppImage",
            "x64 Linux",
        ),
        (
            f"markview-{version}-android.apk",
            "Android 11+ (ARM64)",
        ),
    ]
    extension = json.loads((Path(__file__).resolve().parents[1] /
                            "web/app/vsc-ext/vscode/package.json").read_text())
    downloads.extend((f"{extension['name']}-{extension['version']}-{target}.vsix", label)
                     for target, label in VSCODE_PLATFORMS)
    assets = {asset["name"]: asset for asset in release["assets"]}
    table = ["| File | Platform | Checksum (SHA-256) |", "|------|----------|--------------------|"]
    for name, platform in downloads:
        if name not in assets:
            raise ValueError(f"Release asset is missing: {name}")
        digest = assets[name].get("digest")
        if not digest or not digest.startswith("sha256:"):
            raise ValueError(f"Release asset SHA-256 is missing: {name}")
        checksum = digest.removeprefix("sha256:")
        table.append(f"| [{name}]({base_url}/{name}) | {platform} | `{checksum}` |")

    heading = f"## Download {app['app_name']} {version}\n\n"
    before, separator, after = release["body"].partition(heading)
    if not separator:
        raise ValueError(f"Release download heading is missing: {heading.strip()}")
    _, _, suffix = after.partition("\n\n")
    suffix = suffix.removeprefix(MACOS_NOTE + "\n\n")
    suffix = suffix.removeprefix(VSCODE_NOTE + "\n\n")
    return (before + heading + "\n".join(table) + "\n\n" + MACOS_NOTE + "\n\n" +
            VSCODE_NOTE + "\n\n" + suffix)


if __name__ == "__main__":
    plan, release = (json.loads(Path(path).read_text()) for path in sys.argv[1:])
    sys.stdout.write(render(plan, release))
