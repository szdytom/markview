"""Exercise CI test runners and recovery with real scripts and fake tools."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
TOOL = r'''
import json
import os
from pathlib import Path
import sys

name = Path(sys.argv[0]).name
args = sys.argv[1:]
if name == "sudo":
    os.execvp(args[0], args)
key = name
if name == "apt-get":
    key += ":" + ("update" if "update" in args else "install")
if name == "adb":
    key += ":" + args[2]
trace = Path(os.environ["CI_TEST_TRACE"])
calls = [json.loads(line) for line in trace.read_text().splitlines()]
record = dict(key=key, args=args)
if name == "cargo":
    record["update_goldens"] = os.environ.get("MARKVIEW_UPDATE_RENDER_GOLDENS")
if name == "apt-get":
    sources = next(arg.split("=", 1)[1] for arg in args if arg.startswith("Dir::Etc::sourceparts="))
    record["sources"] = {path.name: path.read_text() for path in Path(sources).iterdir()}
with trace.open("a") as output:
    output.write(json.dumps(record) + "\n")
attempt = 1 + sum(call["key"] == key for call in calls)
failures = json.loads(os.environ.get("CI_TEST_FAILURES", "{}"))
if attempt <= failures.get(key, 0):
    print("simulated infrastructure or application failure", file=sys.stderr)
    sys.exit(73)
if name == "adb" and args[2:] == ["shell", "id", "-u"]:
    print(0)
'''


class CiInfrastructureTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.trace = self.directory / "trace.jsonl"
        self.trace.touch()
        tools = self.directory / "bin"
        tools.mkdir()
        stub = tools / "installer"
        stub.write_text(f"#!{sys.executable}\n" + TOOL)
        stub.chmod(0o755)
        for name in ("sleep", "sudo", "apt-get", "adb", "python3", "cargo"):
            (tools / name).symlink_to(stub)
        self.env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ["PATH"],
                        RUNNER_TEMP=str(self.directory), CI_TEST_TRACE=str(self.trace))

    def run_script(self, script, *args, failures=None):
        self.env["CI_TEST_FAILURES"] = json.dumps(failures or {})
        return subprocess.run(["bash", str(ROOT / script), *args], cwd=self.directory,
                              env=self.env, capture_output=True, text=True, timeout=10)

    def calls(self, key):
        return [call for line in self.trace.read_text().splitlines()
                if (call := json.loads(line))["key"] == key]

    def test_installer_retries_are_bounded_and_preserve_arguments_and_status(self):
        for failures, expected in ((0, 0), (2, 0), (10, 73)):
            with self.subTest(failures=failures):
                self.trace.write_text("")
                result = self.run_script("scripts/ci_retry.sh", "installer", "a b", "$(false)",
                                         failures={"installer": failures})
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertEqual(len(self.calls("installer")), min(failures + 1, 4))
                self.assertTrue(all(call["args"] == ["a b", "$(false)"] for call in self.calls("installer")))
                self.assertEqual([call["args"] for call in self.calls("sleep")],
                                 [[str(10 * attempt)] for attempt in range(1, min(failures, 3) + 1)])

    def test_apt_retries_both_phases_without_changing_package_pins_or_trust(self):
        result = self.run_script("scripts/ci_install_packages.sh", "mesa-vulkan-drivers=25.2.8-0ubuntu0.24.04.4",
                                 failures={"apt-get:update": 1, "apt-get:install": 1})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls("apt-get:update")), 2)
        self.assertEqual(len(self.calls("apt-get:install")), 2)
        for call in self.calls("apt-get:update"):
            self.assertIn("--error-on=any", call["args"])
            original = Path("/etc/apt/sources.list.d/ubuntu.sources")
            self.assertEqual(call["sources"], {"ubuntu.sources": original.read_text()} if original.exists() else {})
        for call in self.calls("apt-get:install"):
            self.assertEqual(call["args"][-3:], ["install", "-y", "mesa-vulkan-drivers=25.2.8-0ubuntu0.24.04.4"])
        self.assertNotIn("--allow-unauthenticated", self.trace.read_text())
        self.trace.write_text("")
        result = self.run_script("scripts/ci_install_packages.sh", "xvfb", failures={"apt-get:update": 10})
        self.assertEqual(result.returncode, 73)
        self.assertEqual(len(self.calls("apt-get:update")), 4)
        self.assertEqual(self.calls("apt-get:install"), [])

    def test_golden_runner_never_updates_in_ci_and_propagates_failures(self):
        self.env.update(VK_DRIVER_FILES="test-driver", MARKVIEW_UPDATE_RENDER_GOLDENS="1")
        for args, failures, expected, updates in (
            ([], {}, 0, ["0", "0"]),
            (["--update"], {}, 0, ["1", "1"]),
            ([], {"cargo": 1}, 73, ["0"]),
            (["--invalid"], {}, 2, []),
        ):
            with self.subTest(args=args, failures=failures):
                self.trace.write_text("")
                result = self.run_script("scripts/test_render_goldens.sh", *args, failures=failures)
                self.assertEqual(result.returncode, expected, result.stderr)
                calls = self.calls("cargo")
                self.assertEqual([call["update_goldens"] for call in calls], updates)
                if calls:
                    self.assertEqual(calls[0]["args"], ["test", "-p", "markview-render", "--locked",
                                                      "--test", "golden", "rendering_baselines", "--", "--nocapture"])
                if len(calls) == 2:
                    self.assertEqual(calls[1]["args"], ["test", "-p", "markview", "--locked",
                                                      "--lib", "rendering_baselines", "--", "--nocapture"])

    def test_fuzz_check_formats_and_lints_the_standalone_workspace(self):
        workflow = (ROOT / ".github/workflows/fuzz.yml").read_text()
        check = workflow.split("\n  smoke:", 1)[0]
        for command in ("cargo +stable fmt -p markview-fuzz -p mvfuzz -- --check",
                        "cargo +stable clippy --workspace --all-targets --locked -- -D warnings"):
            self.assertIn(f"        working-directory: fuzz\n        run: {command}\n", check)

    def test_fuzz_shards_cover_every_harness_and_propagate_failures(self):
        workflow = (ROOT / ".github/workflows/fuzz.yml").read_text()
        shards = re.findall(r"^            targets: (.+)$", workflow, re.MULTILINE)
        manifest = tomllib.loads((ROOT / "fuzz/Cargo.toml").read_text())
        self.assertCountEqual([target for shard in shards for target in shard.split()],
                              [target["name"] for target in manifest["bin"]])
        smoke = re.search(r"- name: Build and smoke selected targets \(ASAN\)\n"
                          r"        working-directory: fuzz\n"
                          r"        run: \|\n((?:          .+\n)+)", workflow).group(1)
        for shard in shards:
            for failures, expected in (({}, 0), ({"cargo": 1}, 73)):
                with self.subTest(shard=shard, failures=failures):
                    self.trace.write_text("")
                    self.env.update(FUZZ_TARGETS=shard, CI_TEST_FAILURES=json.dumps(failures))
                    result = subprocess.run(["bash", "-e", "-c", textwrap.dedent(smoke)],
                                            env=self.env, cwd=self.directory,
                                            capture_output=True, text=True, timeout=10)
                    self.assertEqual(result.returncode, expected, result.stderr)
                    targets = shard.split() if expected == 0 else shard.split()[:1]
                    self.assertEqual([call["args"] for call in self.calls("cargo")],
                                     [["+nightly", "fuzz", "run", target, "--", "-runs=100"]
                                      for target in targets])

    def test_web_ci_covers_lint_and_package_consumers(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        web = re.search(r"^  web:\n(.*?)(?=^  [a-z][\w-]*:)",
                        workflow, re.MULTILINE | re.DOTALL).group(1)
        commands = re.findall(r"working-directory: web\n        run: (.+)", web)
        required = ["pnpm install --frozen-lockfile", "pnpm build", "pnpm lint",
                    "pnpm exec playwright install --with-deps chromium",
                    "pnpm test", "pnpm test:packages"]
        for command in required:
            self.assertIn(command, commands)
        self.assertEqual([command for command in commands if command in required], required)
        self.assertNotIn("continue-on-error:", web)
        self.assertNotIn("if:", web)
        gate = workflow.split("\n  ci:\n", 1)[1]
        self.assertIn("web", re.search(r"needs: \[(.+)\]", gate).group(1).split(", "))
        scripts = json.loads((ROOT / "web/package.json").read_text())["scripts"]
        self.assertIn("pnpm typecheck", scripts["lint"])
        self.assertIn("prettier --check", scripts["lint"])

    def test_emulator_retry_gate_rejects_application_failures(self):
        workflow = (ROOT / ".github/workflows/android.yml").read_text()
        gate = re.search(r"- name: Allow a retry only before application tests start\n"
                         r"        if: steps.android-tests.outcome == 'failure'\n"
                         r"        run: \|\n((?:          .+\n)+)", workflow).group(1)
        for failures, expected, started, test_runs in (
            ({"adb:root": 10}, 1, False, 0),
            ({"python3": 10}, 73, True, 1),
            ({"adb:root": 2}, 0, True, 2),
        ):
            with self.subTest(failures=failures):
                self.trace.write_text("")
                result = self.run_script("android/ci-test.sh", "phone", "1198", failures=failures)
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertEqual(len(self.calls("python3")), test_runs)
                self.assertEqual((self.directory / "markview-android-tests-started").exists(), started)
                decision = subprocess.run(["bash", "-e", "-c", textwrap.dedent(gate)], env=self.env,
                                          capture_output=True, text=True)
                self.assertEqual(decision.returncode == 0, not started)
                if expected == 0:
                    self.assertIn("--layout-only", self.calls("python3")[-1]["args"])
                    self.assertTrue(any(call["args"][-3:] == ["wm", "size", "1198x1600"]
                                        for call in self.calls("adb:shell")))


if __name__ == "__main__":
    unittest.main()
