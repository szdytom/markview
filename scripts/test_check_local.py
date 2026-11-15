"""Exercise local checks with real commands and temporary Git repositories."""

from argparse import Namespace
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import replace
import io
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import check_local


class FuzzCheckTest(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        files = {
            "Cargo.toml": (
                '[package]\nname = "app"\nversion = "0.0.0"\nedition = "2024"\n'
                '[workspace]\nmembers = ["shared"]\nexclude = ["fuzz"]\n'
            ),
            "src/lib.rs": "pub fn app( ) {}\n",
            "shared/Cargo.toml": (
                '[package]\nname = "shared"\nversion = "0.0.0"\nedition = "2024"\n'
            ),
            "shared/src/lib.rs": "pub fn shared( ) {}\n",
            "fuzz/Cargo.toml": (
                '[package]\nname = "markview-fuzz"\nversion = "0.0.0"\nedition = "2024"\n'
                '[workspace]\nmembers = ["fuzzlib"]\n'
                '[dependencies]\nshared = { path = "../shared" }\n'
                '[[bin]]\nname = "harness"\npath = "src/main.rs"\ntest = false\n'
            ),
            "fuzz/src/main.rs": "fn main() {}\n",
            "fuzz/fuzzlib/Cargo.toml": (
                '[package]\nname = "mvfuzz"\nversion = "0.0.0"\nedition = "2024"\n'
            ),
            "fuzz/fuzzlib/src/lib.rs": "pub fn oracle() {}\n",
            "fuzz/fuzzlib/tests/oracle.rs": "#[test]\nfn oracle() {}\n",
        }
        for name, content in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        subprocess.run(
            ("cargo", "generate-lockfile", "--offline", "--manifest-path", "fuzz/Cargo.toml"),
            cwd=self.root,
            check=True,
            capture_output=True,
        )

    def run_check(self, name, *args):
        options = Namespace(
            all=False,
            icons=False,
            compile="--compile" in args,
            dist=False,
            shell=False,
            full=False,
        )
        check = next(
            check for check in check_local.selected_checks(options) if check.name == name
        )
        output = io.StringIO()
        with (
            patch.object(check_local, "ROOT", self.root),
            redirect_stdout(output),
            redirect_stderr(output),
        ):
            ok, _ = check_local.run(check, verbose=False)
        return ok, output.getvalue()

    def test_default_format_checks_both_fuzz_packages_without_formatting_path_dependencies(self):
        ok, output = self.run_check("fuzz format")
        self.assertTrue(ok, output)
        for name in ("fuzz/src/main.rs", "fuzz/fuzzlib/src/lib.rs"):
            with self.subTest(file=name):
                path = self.root / name
                original = path.read_text(encoding="utf-8")
                path.write_text(original.replace("()", "( )"), encoding="utf-8")
                ok, output = self.run_check("fuzz format")
                self.assertFalse(ok, output)
                self.assertIn(str(path), output)
                self.assertIn("( )", path.read_text(encoding="utf-8"))
                path.write_text(original, encoding="utf-8")

    def test_compile_checks_reject_harness_and_oracle_errors(self):
        for check, body, diagnostic in (
            ("fuzz cargo check", "missing();", "cannot find function"),
            ("fuzz clippy", "let _ = 1_i32.clone();", "clone_on_copy"),
        ):
            ok, output = self.run_check(check, "--compile")
            self.assertTrue(ok, output)
            for name in (
                "fuzz/src/main.rs",
                "fuzz/fuzzlib/src/lib.rs",
                "fuzz/fuzzlib/tests/oracle.rs",
            ):
                with self.subTest(check=check, file=name):
                    path = self.root / name
                    original = path.read_text(encoding="utf-8")
                    path.write_text(
                        original.replace("{}", "{ " + body + " }"), encoding="utf-8"
                    )
                    ok, output = self.run_check(check, "--compile")
                    self.assertFalse(ok, output)
                    self.assertIn(diagnostic, output)
                    path.write_text(original, encoding="utf-8")


class WorkflowLintTest(unittest.TestCase):
    @unittest.skipUnless(shutil.which("actionlint"), "actionlint not installed")
    def test_default_check_rejects_invalid_reusable_workflow(self):
        check = next(check for check in check_local.CHECKS if check.name == "actionlint")
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            subprocess.run(("git", "init", "--quiet", str(root)), check=True)
            workflows = root / ".github/workflows"
            workflows.mkdir(parents=True)
            (workflows / "ci.yml").write_text(
                "on: push\njobs:\n  pbt:\n    uses: ./.github/workflows/pbt.yml\n",
                encoding="utf-8",
            )
            reusable = workflows / "pbt.yml"
            workflow = (
                "on:\n  workflow_call:\njobs:\n  test:\n"
                "    runs-on: ubuntu-latest\n    steps:\n"
                "      - run: 'cargo test --locked -p xtask proptests::'\n"
            )
            reusable.write_text(workflow, encoding="utf-8")
            (workflows / "release.yml").write_text("generated: :\n", encoding="utf-8")
            output = io.StringIO()
            with (
                patch.object(check_local, "ROOT", root),
                redirect_stdout(output),
                redirect_stderr(output),
            ):
                check = replace(check, command=check_local.actionlint_command())
                with patch.object(check_local, "CHECKS", (check,)):
                    self.assertEqual(check_local.main([]), 0, output.getvalue())
                    reusable.write_text(workflow.replace("'", ""), encoding="utf-8")
                    self.assertEqual(check_local.main([]), 1, output.getvalue())
            self.assertIn("pbt.yml", output.getvalue())
            self.assertIn("could not parse as YAML", output.getvalue())
            self.assertEqual(
                (root / ".git/check-local-failures").read_text(), "actionlint\n"
            )


class LocalCheckTest(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        subprocess.run(("git", "init", "--quiet", str(self.root)), check=True)
        self.record = self.root / ".git/check-local-failures"
        self.trace = self.root / "trace"
        self.failures = self.root / "failures"
        tool = self.root / "check.py"
        tool.write_text(
            "from pathlib import Path\n"
            "import sys\n"
            "name = sys.argv[1]\n"
            "with Path('trace').open('a') as trace:\n"
            "    trace.write(name + '\\n')\n"
            "sys.exit(1 if name in Path('failures').read_text().splitlines() else 0)\n",
            encoding="utf-8",
        )
        checks = tuple(
            check_local.Check(name, (sys.executable, str(tool), name), group)
            for name, group in (
                ("first", "light"),
                ("second", "light"),
                ("third", "light"),
                ("compile", "compile"),
            )
        )
        for attribute, value in (("ROOT", self.root), ("CHECKS", checks)):
            replacement = patch.object(check_local, attribute, value)
            replacement.start()
            self.addCleanup(replacement.stop)

    def run_checks(self, *args, failures=()):
        self.trace.write_text("", encoding="utf-8")
        self.failures.write_text("\n".join(failures), encoding="utf-8")
        output = io.StringIO()
        with redirect_stdout(output):
            status = check_local.main(list(args))
        return status, self.trace.read_text(encoding="utf-8").splitlines(), output.getvalue()

    def test_fail_fast_retries_latest_failure_then_restores_standard_order(self):
        status, calls, _ = self.run_checks(failures=("second",))
        self.assertEqual((status, calls), (1, ["first", "second"]))
        self.assertEqual(self.record.read_text(), "second\n")

        status, calls, _ = self.run_checks(failures=("third",))
        self.assertEqual((status, calls), (1, ["second", "first", "third"]))
        self.assertEqual(self.record.read_text(), "third\n")

        status, calls, _ = self.run_checks()
        self.assertEqual((status, calls), (0, ["third", "first", "second"]))
        self.assertFalse(self.record.exists())
        status, calls, _ = self.run_checks()
        self.assertEqual((status, calls), (0, ["first", "second", "third"]))

    def test_keep_going_records_all_failures_and_preserves_standard_order(self):
        status, calls, _ = self.run_checks("--keep-going", failures=("first", "third"))
        self.assertEqual((status, calls), (1, ["first", "second", "third"]))
        self.assertEqual(self.record.read_text(), "first\nthird\n")
        status, calls, _ = self.run_checks()
        self.assertEqual((status, calls), (0, ["first", "third", "second"]))
        self.assertFalse(self.record.exists())

    def test_list_is_read_only_and_only_selected_checks_are_prioritized(self):
        self.run_checks("--list")
        self.assertFalse(self.record.exists())
        remembered = "removed\ncompile\nthird\n"
        self.record.write_text(remembered, encoding="utf-8")
        status, calls, output = self.run_checks("--list")
        self.assertEqual((status, calls), (0, []))
        self.assertLess(output.index("third"), output.index("first"))
        self.assertLess(output.index("first"), output.index("second"))
        self.assertNotIn("removed", output)
        self.assertNotIn("compile", output)
        self.assertEqual(self.record.read_text(), remembered)
        status, calls, _ = self.run_checks("--compile")
        self.assertEqual((status, calls), (0, ["third", "compile", "first", "second"]))
        self.assertFalse(self.record.exists())

    def test_linked_worktree_keeps_its_record_outside_the_working_tree(self):
        worktree = self.root / "linked"
        subprocess.run(
            ("git", "-C", str(self.root), "worktree", "add", "--quiet", "--orphan", str(worktree)),
            check=True,
        )
        self.record.write_text("second\n", encoding="utf-8")
        with patch.object(check_local, "ROOT", worktree):
            record = check_local.failure_record_path()
            self.assertEqual(record, self.root / ".git/worktrees/linked/check-local-failures")
            self.assertFalse(record.exists())
            record.write_text("third\n", encoding="utf-8")
        self.assertEqual(self.record.read_text(), "second\n")
        status = subprocess.run(
            ("git", "-C", str(worktree), "status", "--porcelain"),
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertEqual(status.stdout, "")


if __name__ == "__main__":
    unittest.main()
