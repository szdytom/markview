"""Read Android release and toolchain settings shared by local builds and CI."""

import os
from pathlib import Path
import tomllib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
ANDROID = "{http://schemas.android.com/apk/res/android}"


def sdk_directory():
    return Path(os.environ.get("ANDROID_HOME", Path.home() / "Android/Sdk"))


def load(root=ROOT):
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    sdk = ET.parse(root / "android/AndroidManifest.xml").getroot().find("uses-sdk")
    return {
        **workspace["metadata"]["android"],
        "version-name": workspace["package"]["version"],
        "min-sdk": sdk.get(ANDROID + "minSdkVersion"),
        "target-sdk": sdk.get(ANDROID + "targetSdkVersion"),
    }


if __name__ == "__main__":
    for key, value in load().items():
        print(f"{key}={value}")
