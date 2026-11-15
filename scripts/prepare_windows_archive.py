#!/usr/bin/env python3
"""Include the portable-mode helper in this Windows build's dist archives."""

import json
from pathlib import Path
import re
import tomllib


def prepare(config: Path) -> None:
    source = config.read_text(encoding="utf-8")
    includes = tomllib.loads(source)["dist"]["include"]
    helper = "packaging/windows/enable-portable.bat"
    if helper in includes:
        return
    includes.append(helper)
    source = re.sub(
        r"(?ms)^include\s*=\s*\[.*?\]",
        "include = " + json.dumps(includes),
        source,
        count=1,
    )
    config.write_text(source, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    prepare(Path(__file__).resolve().parents[1] / "dist-workspace.toml")
