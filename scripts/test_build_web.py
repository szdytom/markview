"""Exercise build orchestration without compiling Rust or downloading tools."""

import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("build-web.py")
spec = importlib.util.spec_from_file_location("build_web", SCRIPT)
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class BuildTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="markview build 空格 ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        manifest = self.root / "crates/markview-web/Cargo.toml"
        manifest.parent.mkdir(parents=True)
        manifest.write_text(
            """[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "=0.2.999"
""",
            encoding="utf-8",
        )
        self.cli = self.root / "bindings tool.exe"
        self.cli.touch()
        self.output = self.root / "web/packages/markview/wasm"
        self.output.mkdir(parents=True)
        (self.output / "old.js").write_text("previous build", encoding="utf-8")
        self.target = self.root / "custom cargo output"
        self.version = "0.2.999"
        self.installed = build.TARGET
        self.fail_bindings = False
        self.calls = []
        patches = [
            patch.object(build, "ROOT", self.root),
            patch.object(build, "OUTPUT", self.output),
            patch.dict(os.environ, {"WASM_BINDGEN": str(self.cli)}),
            patch.object(
                build.shutil,
                "which",
                side_effect=lambda name: name if name in ("cargo", "rustup") else None,
            ),
            patch.object(build.subprocess, "run", side_effect=self.run_tool),
        ]
        for mock in patches:
            mock.start()
            self.addCleanup(mock.stop)

    def run_tool(self, args, **options):
        self.calls.append(args)
        if args[-1] == "--version":
            return subprocess.CompletedProcess(
                args, 0, f"wasm-bindgen {self.version}\n"
            )
        self.assertEqual(options["cwd"], self.root)
        self.assertNotIn("shell", options)
        stdout = ""
        if args[:2] == ["cargo", "metadata"]:
            stdout = json.dumps({"target_directory": str(self.target)})
        elif args[:3] == ["rustup", "target", "list"]:
            stdout = self.installed
        elif "--out-dir" in args:
            self.assertEqual(
                Path(args[-1]), self.target / build.TARGET / "release/markview_web.wasm"
            )
            if self.fail_bindings:
                raise subprocess.CalledProcessError(1, args)
            destination = Path(args[args.index("--out-dir") + 1])
            (destination / "markview_web_bg.wasm").write_bytes(b"new wasm")
        return subprocess.CompletedProcess(args, 0, stdout)

    def main(self, *args):
        with patch("sys.argv", [str(SCRIPT), *args]):
            build.main()

    def test_build_uses_cargo_directory_and_handles_spaces(self):
        self.main()
        self.assertEqual(
            (self.output / "markview_web_bg.wasm").read_bytes(), b"new wasm"
        )
        self.assertFalse((self.output / "old.js").exists())

    def test_failed_generation_preserves_previous_build(self):
        self.fail_bindings = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.main()
        self.assertEqual(
            (self.output / "old.js").read_text(encoding="utf-8"), "previous build"
        )
        self.assertEqual(list(self.output.parent.iterdir()), [self.output])

    def test_mismatched_override_fails_before_compilation(self):
        self.version = "0.2.123"
        with self.assertRaisesRegex(
            SystemExit, "WASM_BINDGEN must point to wasm-bindgen 0.2.999"
        ):
            self.main()
        self.assertEqual(len(self.calls), 1)

    def test_missing_target_explains_setup(self):
        self.installed = ""
        with self.assertRaisesRegex(SystemExit, "pnpm --dir web run setup"):
            self.main()
        self.assertFalse(any(args[:2] == ["cargo", "build"] for args in self.calls))

    def test_setup_reads_version_and_installs_locally(self):
        with patch.dict(os.environ, {"WASM_BINDGEN": ""}):
            self.main("--setup")
        install = next(args for args in self.calls if args[:2] == ["cargo", "install"])
        self.assertEqual(install[install.index("--version") + 1], "0.2.999")
        self.assertEqual(
            Path(install[install.index("--root") + 1]),
            self.root / ".tools/wasm-bindgen-0.2.999",
        )

    def test_setup_reuses_matching_tool(self):
        self.main("--setup")
        self.assertFalse(any(args[:2] == ["cargo", "install"] for args in self.calls))


class LauncherTests(unittest.TestCase):
    def test_launcher_from_another_directory_with_spaces(self):
        with tempfile.TemporaryDirectory(prefix="markview launcher 空格 ") as temporary:
            root = Path(temporary)
            launcher = root / "web/scripts/wasm.mjs"
            launcher.parent.mkdir(parents=True)
            launcher.write_bytes(
                (SCRIPT.parent.parent / "web/scripts/wasm.mjs").read_bytes()
            )
            script = root / "scripts/build-web.py"
            script.parent.mkdir()
            script.write_bytes(SCRIPT.read_bytes())
            result = subprocess.run(
                ["node", str(launcher), "--help"],
                cwd=SCRIPT.parent,
                capture_output=True,
                text=True,
                check=True,
            )
            self.assertIn("--setup", result.stdout)


if __name__ == "__main__":
    unittest.main()
