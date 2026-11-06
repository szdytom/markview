"""Exercise Android case selection and isolation through a stateful fake device."""
import ast
import contextlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

import test as runner

ROOT = Path(__file__).resolve().parent.parent
ADB = r'''
import json
import os
from pathlib import Path
import sys
import time

args = sys.argv[3:]
trace = Path(os.environ["ANDROID_TEST_TRACE"])
with trace.open("a") as output:
    output.write(json.dumps(args) + "\n")
state_file = trace.with_suffix(".state")
state = json.loads(state_file.read_text()) if state_file.exists() else {}
if args[:3] == ["shell", "pm", "clear"]:
    state = {"fresh": True}
elif args[:4] == ["shell", "run-as", "io.github.szdytom.markview", "cp"]:
    state["cache"] = True
elif args[:3] == ["shell", "am", "instrument"]:
    name = args[-1].rsplit(".", 1)[-1]
    phase = args[args.index("session-phase") + 1] if "session-phase" in args else None
    if phase == "restore":
        assert state.get("session") == "seeded", state
    else:
        assert state.get("fresh") and state.get("cache"), state
    state["fresh"] = False
    state["case"] = name
    if phase == "seed":
        state["session"] = "seeded"
    if name == os.environ.get("ANDROID_TEST_TIMEOUT"):
        print("partial timeout output", flush=True)
        time.sleep(5)
    if name == os.environ.get("ANDROID_TEST_FAILURE"):
        print("FAIL: simulated instrumentation assertion")
        print("INSTRUMENTATION_CODE: 0")
    else:
        print("MARKVIEW_ANDROID_INTEGRATION_OK")
elif args[:4] == ["shell", "settings", "get", "secure"]:
    print("null")
elif args[:4] == ["shell", "run-as", "io.github.szdytom.markview", "ls"]:
    print(state["case"] + ".png")
elif args[:4] == ["exec-out", "run-as", "io.github.szdytom.markview", "cat"]:
    sys.stdout.buffer.write(b"test screenshot")
state_file.write_text(json.dumps(state))
'''


class AndroidCacheFixtureTest(unittest.TestCase):
    def test_seeded_image_matches_the_engine_cache_contract(self):
        source = (ROOT / "src/images/cache.rs").read_text()
        magic = ast.literal_eval(re.search(r'const MAGIC:.*?= (b".*?");', source).group(1))
        metadata = re.search(r'struct Meta \{(.*?)^\}', source, re.S | re.M).group(1)
        fields = set(re.findall(r'^\s+(\w+):', metadata, re.M))
        with tempfile.TemporaryDirectory() as directory, patch.object(runner, "run"), patch.object(runner.subprocess, "check_output", return_value="fixture.img\n"):
            data = runner.cache_fixture(Path(directory)).read_bytes()
        self.assertEqual(data[:len(magic)], magic)
        length = struct.unpack("<I", data[len(magic):len(magic) + 4])[0]
        header = json.loads(data[len(magic) + 4:len(magic) + 4 + length])
        body = data[len(magic) + 4 + length:]
        self.assertLessEqual(fields, header.keys())
        self.assertTrue(header["lifetime_from_max_age"])
        self.assertEqual(header["bytes"], len(body))
        self.assertEqual(body, (ROOT / "assets/markview-icon-color.svg").read_bytes())


class AndroidRunnerTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        manifest = self.root / "android/tests/AndroidManifest.xml"
        manifest.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / "android/tests/AndroidManifest.xml", manifest)
        self.trace = self.root / "trace.jsonl"
        self.trace.touch()
        adb = self.root / ".tools/android-sdk/platform-tools/adb"
        adb.parent.mkdir(parents=True)
        adb.write_text(f"#!{sys.executable}\n" + ADB)
        adb.chmod(0o755)
        fixture = self.root / "fixture.img"
        fixture.touch()
        for replacement in (
            patch.object(runner, "ROOT", self.root),
            patch.object(runner, "build_tests"),
            patch.object(runner, "cache_fixture", return_value=fixture),
            patch.dict(os.environ, {"ANDROID_TEST_TRACE": str(self.trace)}, clear=True),
        ):
            replacement.start()
            self.addCleanup(replacement.stop)

    def run_suite(self, *args):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            status = runner.main(list(args))
        return status, output.getvalue()

    def calls(self):
        return [json.loads(line) for line in self.trace.read_text().splitlines()]

    def instrumentations(self):
        return [call for call in self.calls() if call[:3] == ["shell", "am", "instrument"]]

    def test_full_suite_shuffles_and_resets_each_case_but_preserves_session_phases(self):
        orders = []
        for seed in (7, 42):
            self.trace.write_text("")
            status, _ = self.run_suite("--filter", "*", "--shuffle", str(seed))
            self.assertEqual(status, 0)
            runs = self.instrumentations()
            names = [call[-1].rsplit(".", 1)[-1] for call in runs]
            orders.append(names)
            expected = [name.rsplit(".", 1)[-1] for name in runner.test_classes()]
            self.assertCountEqual(names, expected + ["SessionTest"])
            self.assertEqual(sum(call[:3] == ["shell", "pm", "clear"] for call in self.calls()), len(expected))
            session = [call for call in runs if call[-1].endswith(".SessionTest")]
            self.assertEqual([call[call.index("session-phase") + 1] for call in session], ["seed", "restore"])
            for name in expected:
                directory = self.root / "artifacts/android/phone" / name
                self.assertTrue((directory / "integration.txt").is_file())
                self.assertEqual((directory / (name + ".png")).read_bytes(), b"test screenshot")
            self.assertIn(["shell", "settings", "delete", "secure", "show_ime_with_hard_keyboard"], self.calls())
            self.assertIn(["shell", "ip", "addr", "del", "198.18.0.1/32", "dev", "lo"], self.calls())
        self.assertNotEqual(*orders)

    def test_filters_and_legacy_shortcuts_execute_only_selected_cases(self):
        default = [name.rsplit(".", 1)[-1] for name in runner.test_classes() if not name.endswith(".SessionTest")]
        for options, expected in (
            ([], default),
            (["--filter", "*Search*", "--filter", "SettingsTest"], ["SettingsTest", "SearchTest"]),
            (["--session-only"], ["SessionTest", "SessionTest"]),
            (["--links-only"], ["LinksTest"]),
            (["--layout-only"], ["LayoutTest"]),
            (["--lifecycle-only"], ["LifecycleTest"]),
            (["--mermaid-only"], ["MermaidTest"]),
        ):
            with self.subTest(options=options):
                self.trace.write_text("")
                status, _ = self.run_suite(*options, "--layout", "tablet")
                self.assertEqual(status, 0)
                self.assertEqual([call[-1].rsplit(".", 1)[-1] for call in self.instrumentations()], expected)
                self.assertTrue(all(call[call.index("layout") + 1] == "tablet" for call in self.instrumentations()))
                self.assertEqual(["root"] in self.calls(), "LinksTest" in expected)

    def test_failures_and_timeouts_keep_reports_restore_keyboard_and_run_next_case(self):
        for mode in ("FAILURE", "TIMEOUT"):
            with self.subTest(mode=mode), patch.dict(os.environ, {"ANDROID_TEST_" + mode: "SearchTest"}):
                self.trace.write_text("")
                status, _ = self.run_suite("--filter", "SearchTest", "--filter", "InputTest", "--timeout", "1")
                self.assertEqual(status, 1)
                directory = self.root / "artifacts/android/phone"
                summary = (directory / "integration.txt").read_text()
                self.assertEqual(summary, "FAIL: SearchTest\nPASS: InputTest\n")
                report = (directory / "SearchTest/integration.txt").read_text()
                self.assertIn("TIMEOUT" if mode == "TIMEOUT" else "INSTRUMENTATION_CODE: 0", report)
                if mode == "TIMEOUT":
                    self.assertIn("partial timeout output", report)
                self.assertIn(["shell", "settings", "delete", "secure", "show_ime_with_hard_keyboard"], self.calls())

    def test_listing_and_invalid_filters_do_not_build_or_contact_the_device(self):
        status, output = self.run_suite("--filter", "*Search*", "--list")
        self.assertEqual((status, output), (0, "SearchTest\n"))
        for args in (["--filter", "missing"], ["--filter", "SearchTest", "--layout-only"]):
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                self.run_suite(*args)
            self.assertEqual(error.exception.code, 2)
        runner.build_tests.assert_not_called()
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
