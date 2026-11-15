"""Exercise path regression orchestration with a real, small Rust test binary."""

from contextlib import redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import test_paths


@unittest.skipUnless(os.name == "posix", "Unix symlink regression runner")
class PathRunnerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.root = Path(cls.temporary.name).resolve()
        cls.binary = cls.root / "path-tests"
        source = cls.root / "fixture.rs"
        source.write_text(r'''
            #[test]
            fn observes_temporary_root() {
                let root = std::env::temp_dir();
                assert_eq!(root, std::path::PathBuf::from(std::env::var_os("TMPDIR").unwrap()));
                let canonical = root.canonicalize().unwrap();
                let mode = if root == canonical { "ordinary" } else { "symlink" };
                if let Some(trace) = std::env::var_os("PATH_TEST_TRACE") {
                    use std::io::Write;
                    let mut output = std::fs::OpenOptions::new().append(true).open(trace).unwrap();
                    writeln!(output, "{mode}").unwrap();
                }
                assert_ne!(std::env::var("PATH_TEST_FAIL").ok().as_deref(), Some(mode));
            }
            #[test]
            #[ignore]
            fn ignored_only() {}
        ''')
        subprocess.run(["rustc", "--test", "-C", "prefer-dynamic", str(source), "-o", str(cls.binary)], check=True)
        cls.empty_binary = cls.root / "empty-tests"
        source.write_text("fn main() {}\n")
        subprocess.run(["rustc", "--test", "-C", "prefer-dynamic", str(source), "-o", str(cls.empty_binary)], check=True)

    def test_runs_same_binary_twice_with_distinct_path_representations(self):
        trace = self.root / "trace"
        trace.write_text("")
        with (
            patch.object(sys, "argv", ["test_paths.py", "--binary", str(self.empty_binary),
                                       "--binary", str(self.binary)]),
            patch.dict(os.environ, PATH_TEST_TRACE=str(trace)),
            patch.object(test_paths, "build_binaries", side_effect=AssertionError("unexpected build")),
            redirect_stdout(io.StringIO()),
        ):
            self.assertEqual(test_paths.main(), 0)
        self.assertEqual(trace.read_text().splitlines(), ["ordinary", "symlink"])

    def test_ci_symlink_pass_reuses_multiple_binaries_and_preserves_parent_environment(self):
        trace = self.root / "ci-trace"
        trace.write_text("")
        previous = os.environ.get("TMPDIR")
        with (
            patch.object(sys, "argv", ["test_paths.py", "--symlink-only", "--binary", str(self.binary),
                                       "--binary", str(self.empty_binary)]),
            patch.dict(os.environ, PATH_TEST_TRACE=str(trace)),
            redirect_stdout(io.StringIO()),
        ):
            self.assertEqual(test_paths.main(), 0)
        self.assertEqual(trace.read_text().splitlines(), ["symlink"])
        self.assertEqual(os.environ.get("TMPDIR"), previous)

    def test_rejects_empty_filters_and_propagates_test_failures(self):
        for arguments in (("missing_test",), ("ignored_only",)):
            with patch.object(test_paths, "TEST_ARGUMENTS", arguments):
                with self.assertRaisesRegex(RuntimeError, "No runnable tests"):
                    test_paths.run_suite([self.binary], self.root)
        with patch.dict(os.environ, PATH_TEST_FAIL="ordinary"):
            with self.assertRaises(subprocess.CalledProcessError):
                test_paths.run_suite([self.binary], self.root)

    def test_selects_workspace_test_artifacts_and_preserves_compile_failures(self):
        messages = [
            dict(reason="compiler-artifact", target=dict(name="dependency"),
                 profile=dict(test=True), executable=str(self.root / "dependency-tests")),
            dict(reason="compiler-artifact", target=dict(name="markview"),
                 profile=dict(test=False), executable="/app"),
            dict(reason="compiler-artifact", target=dict(name="markview"),
                 profile=dict(test=True), executable=str(self.binary)),
            dict(reason="compiler-message", message=dict(rendered="diagnostic\n")),
        ]
        result = subprocess.CompletedProcess([], 0, "\n".join(map(json.dumps, messages)))
        with patch.object(subprocess, "run", return_value=result), redirect_stderr(io.StringIO()):
            self.assertEqual(test_paths.build_binaries(), sorted([self.root / "dependency-tests", self.binary]))
        result.returncode = 101
        diagnostics = io.StringIO()
        with patch.object(subprocess, "run", return_value=result), redirect_stderr(diagnostics):
            with self.assertRaises(subprocess.CalledProcessError):
                test_paths.build_binaries()
        self.assertIn("diagnostic", diagnostics.getvalue())
        result.returncode = 0
        result.stdout = ""
        with patch.object(subprocess, "run", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "no test binaries"):
                test_paths.build_binaries()


if __name__ == "__main__":
    unittest.main()
