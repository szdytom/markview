import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from check_platform_boundary import check, violations


class PlatformBoundaryTests(unittest.TestCase):
    def test_platform_conditions_include_nested_and_multiline_attributes(self):
        for source in ('#[cfg(unix)]', 'cfg!(windows)', 'cfg ! (target_os = "macos")', '#[cfg(all(test,\n target_os = "android"))]', '#[cfg_attr(not(windows), allow(dead_code))]'):
            self.assertTrue(list(violations(source)))

    def test_only_tests_debugging_and_final_link_attribute_are_allowed(self):
        source = '#[cfg(test)]\n#[cfg(not(test))]\n#[cfg(debug_assertions)]\n#[cfg(all(test, not(debug_assertions)))]'
        self.assertEqual(list(violations(source)), [])
        link = '#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]'
        self.assertEqual(list(violations(link, main=True)), [])
        self.assertTrue(list(violations(link)))

    def test_only_the_diagnostics_feature_may_condition_the_cli(self):
        self.assertEqual(
            list(violations('#[cfg(feature = "diagnostics")]')), []
        )
        self.assertEqual(
            list(violations('#[cfg(any(test, feature = "diagnostics"))]')), []
        )
        self.assertTrue(
            list(violations('#[cfg(feature = "other")]'))
        )

    def test_literals_and_nested_comments_do_not_hide_live_conditions(self):
        source = '''// cfg!(windows)
/* " /* cfg!(unix) */ */ cfg!(target_os = "macos");
const FIXTURE: &str = r##"#[cfg(unix)] cfg!(windows)"##;
const TEXT: &str = "cfg!(windows)";
const PAREN: char = ')';
#[cfg(all(test, target_os = "linux"))]
'''
        self.assertEqual(list(violations(source)), [
            (2, 'target_os = "macos"'),
            (6, 'all(test, target_os = "linux")'),
        ])


    def test_source_only_complexity_markers_do_not_introduce_platform_conditions(self):
        for reason in ('Keep the mapping together', 'Keep a ) literal', 'Keep #[cfg(windows)] as literal text'):
            source = f'#[cfg_attr(any(), markview::allow_complexity(reason = "{reason}"))]'
            self.assertEqual(list(violations(source)), [], source)

    def test_complexity_markers_cannot_hide_other_attributes_or_live_conditions(self):
        for source in (
            '#[cfg_attr(windows, markview::allow_complexity(reason = "mapping"))]',
            '#[cfg_attr(any(windows), markview::allow_complexity(reason = "mapping"))]',
            '#[cfg_attr(any(), markview::allow_complexity(reason = "mapping"), cfg(windows))]',
            '#[cfg_attr(any(), other::allow_complexity(reason = "mapping"))]',
            '#[cfg_attr(any(), markview::allow_complexity(reason = ""))]',
            'cfg!(any(), markview::allow_complexity(reason = "mapping"))',
        ):
            self.assertTrue(list(violations(source)), source)


class WorkspaceBoundaryTests(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.write("Cargo.toml", '[workspace]\nmembers = ["crates/*"]\nexclude = ["crates/excluded"]\n')

    def write(self, path, source):
        path = self.root / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source, encoding="utf-8")

    def package(self, name, extra=""):
        self.write(f"crates/{name}/Cargo.toml", f'[package]\nname = "{name}"\nversion = "0.1.0"\n{extra}')

    def test_member_discovery_covers_crates_and_all_source_directories(self):
        self.package("reader", '[features]\ntexture-readback = []\n')
        self.write("crates/reader/src/lib.rs", '#[cfg(feature = "texture-readback")]\n#[cfg(all(test, not(feature = "texture-readback")))]')
        self.package("markview-platform")
        self.write("crates/markview-platform/src/lib.rs", 'cfg!(target_os = "macos")')
        self.package("excluded")
        self.write("crates/excluded/src/lib.rs", "cfg!(windows)")
        self.assertEqual(check(self.root), [])

        # A newly added member must be checked without changing the script.
        self.package("new-reader")
        paths = ["src/gpu.rs", "src/target/native.rs", "tests/native.rs", "examples/native.rs", "benches/native.rs", "build.rs"]
        for path in paths:
            self.write(f"crates/new-reader/{path}", 'cfg!(target_os = "macos")')
        self.write("crates/reader/src/mixed.rs", '#[cfg(all(feature = "texture-readback", target_os = "macos"))]')
        self.write("crates/reader/tests/undeclared.rs", '#[cfg(feature = "other")]')
        failures = check(self.root)
        self.assertEqual(len(failures), len(paths) + 2, failures)
        for path in paths:
            self.assertTrue(any(f"crates/new-reader/{path}:1:" in failure for failure in failures), path)
        self.assertTrue(any("src/mixed.rs:1:" in failure for failure in failures))
        self.assertTrue(any("tests/undeclared.rs:1:" in failure for failure in failures))

    def test_custom_target_and_build_paths_are_checked(self):
        self.package("reader", 'build = "tools/build.rs"\n[lib]\npath = "library.rs"\n[[test]]\nname = "native"\npath = "integration/native.rs"\n')
        for path in ("tools/build.rs", "library.rs", "integration/native.rs", "integration/support/mod.rs"):
            self.write(f"crates/reader/{path}", "cfg!(unix)")
        self.write("crates/reader/target/generated.rs", "cfg!(unix)")
        self.write("crates/reader/nested/Cargo.toml", '[package]\nname = "nested"\nversion = "0.1.0"\n')
        self.write("crates/reader/nested/src/lib.rs", "cfg!(unix)")
        self.assertEqual(len(check(self.root)), 4)

    def test_root_application_keeps_its_cli_and_link_rules(self):
        self.write("Cargo.toml", '[package]\nname = "reader"\nversion = "0.1.0"\n[workspace]\nmembers = []\n[features]\ndiagnostics = []\nparallel-layout = []\n')
        self.write("src/main.rs", '#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]\n#[cfg(any(test, feature = "diagnostics"))]')
        self.write("build.rs", "#[cfg(windows)]\n#[cfg(not(windows))]")
        self.assertEqual(check(self.root), [])
        self.write("tests/native.rs", '#[cfg(feature = "parallel-layout")]')
        self.assertEqual(len(check(self.root)), 1)

    def test_webpage_benchmark_may_report_the_layout_feature(self):
        self.write("Cargo.toml", '[package]\nname = "reader"\nversion = "0.1.0"\n[workspace]\nmembers = []\n[features]\nparallel-layout = []\n')
        self.write("examples/webpage_bench.rs", 'cfg!(feature = "parallel-layout")')
        self.assertEqual(check(self.root), [])
        self.write("examples/other.rs", 'cfg!(feature = "parallel-layout")')
        self.assertEqual(len(check(self.root)), 1)

    def test_target_exceptions_are_scoped_to_files_and_complete_conditions(self):
        self.package("markview-web")
        self.write("crates/markview-web/src/lib.rs", '#[cfg(target_arch = "wasm32")]\n#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]')
        self.assertEqual(check(self.root), [])
        self.write("crates/markview-web/src/other.rs", '#[cfg(target_arch = "wasm32")]')
        self.write("crates/markview-web/src/lib.rs", '#[cfg(any(target_arch = "wasm32", target_os = "macos"))]')
        failures = check(self.root)
        self.assertEqual(len(failures), 2)
        self.assertTrue(any("src/lib.rs:1:" in failure for failure in failures))
        self.assertTrue(any("src/other.rs:1:" in failure for failure in failures))


if __name__ == "__main__":
    unittest.main()
