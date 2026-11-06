#!/usr/bin/env python3
"""Check workspace conditional compilation against native platform boundaries."""
from pathlib import Path
import os
import re
import tomllib

ROOT = Path(__file__).resolve().parents[1]
CONDITIONS = re.compile(r"#\s*!?\s*\[\s*cfg(?:_attr)?\s*\(|\bcfg\s*!\s*\(")
ALLOWED = {"test", "debug_assertions", "not", "all", "any", "(", ")", ","}
FEATURE = re.compile(r'\bfeature\s*=\s*"([^"\\]*)"')
# Ignore Rust literals and comments, including fixtures containing Rust source.
IGNORED = re.compile(
    r'//[^\n]*|/\*|(?:br|cr|r)(?P<hashes>\#*)".*?"(?P=hashes)'
    r'|(?:b|c)?"(?:\\.|[^"\\])*"|(?:b)?\'(?:\\.|[^\'\\\n])\'',
    re.S,
)
COMPLEXITY_MARKER = re.compile(
    r'any\s*\(\s*\)\s*,\s*markview\s*::\s*allow_complexity\s*\('
    r'\s*reason\s*=\s*"(?:\\.|[^"\\])+"\s*,?\s*\)\s*,?'
)
LINK_ATTRIBUTE = 'not(debug_assertions), windows_subsystem = "windows"'
WASM = {'target_arch = "wasm32"', 'not(target_arch = "wasm32")'}
PARALLEL = 'all(feature = "parallel", not(target_arch = "wasm32"))'
EXCEPTIONS = {
    # The final build script embeds the native executable icon.
    "build.rs": {"windows", "not(windows)"},
    # Browser callbacks cannot use native worker threads.
    "crates/markview-layout/src/background.rs": WASM,
    "crates/markview-layout/src/layout.rs": {PARALLEL, f"not({PARALLEL})"},
    "crates/markview-layout/src/layout/progressive.rs": {PARALLEL, f"not({PARALLEL})"},
    "crates/markview-layout/src/shaping.rs": {PARALLEL},
    "crates/markview-layout/tests/parallel.rs": {
        PARALLEL, 'all(test, feature = "parallel", not(target_arch = "wasm32"))',
    },
    # `MediaContext::native` supplies default MVSS media until the host overrides it.
    "crates/markview-layout/src/style/media.rs": {
        'target_arch = "wasm32"', 'any(target_os = "android", target_os = "ios")',
    },
    # Only the browser entry point needs JavaScript bindings.
    "crates/markview-web/src/lib.rs": {
        'target_arch = "wasm32"', 'not(target_arch = "wasm32"), allow(dead_code)',
    },
    # Benchmark metadata records the layout feature used by the executable.
    "examples/webpage_bench.rs": {'feature = "parallel-layout"'},
    # Pixel baselines use the Linux software renderer.
    "crates/markview-render/tests/golden.rs": {'target_os = "linux"'},
}


def mask(source, strings=True):
    """Blank comments and optionally literals while preserving source positions."""
    parts = []
    consumed = 0
    while match := IGNORED.search(source, consumed):
        start, end = match.span()
        comment = source.startswith(("//", "/*"), start)
        if source.startswith("/*", start):
            depth = 1
            for delimiter in re.finditer(r"/\*|\*/", source[end:]):
                depth += 1 if delimiter.group() == "/*" else -1
                if not depth:
                    end += delimiter.end()
                    break
        parts.append(source[consumed:start])
        token = source[start:end]
        parts.append(re.sub(r"[^\n]", " ", token) if comment or strings else token)
        consumed = end
    return "".join(parts) + source[consumed:]


def tokens(condition):
    return tuple(re.findall(r'"(?:\\.|[^"\\])*"|[A-Za-z_]\w*|[^\s]', condition))


def violations(source, main=False, features=("diagnostics",), exceptions=()):
    source = mask(source, strings=False)
    code = mask(source)
    exceptions = {tokens(condition) for condition in exceptions}
    consumed = 0
    for match in CONDITIONS.finditer(code):
        if match.start() < consumed:
            continue
        start = end = match.end()
        depth = 1
        while end < len(code) and depth:
            depth += (code[end] == "(") - (code[end] == ")")
            end += 1
        consumed = end
        condition = re.sub(r"\s+", " ", source[start:end - 1]).strip()
        complexity_marker = "cfg_attr" in match.group() and COMPLEXITY_MARKER.fullmatch(condition)
        feature_condition = FEATURE.sub(
            lambda match: "test" if match[1] in features else match[0], condition
        )
        if (
            set(tokens(feature_condition)) <= ALLOWED
            or (main and tokens(condition) == tokens(LINK_ATTRIBUTE))
            or complexity_marker
            or tokens(condition) in exceptions
        ):
            continue
        yield source.count("\n", 0, match.start()) + 1, condition


def workspace_packages(root):
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    workspace = manifest.get("workspace", {})
    excluded = {path for pattern in workspace.get("exclude", []) for path in root.glob(pattern)}
    packages = {path for pattern in workspace.get("members", []) for path in root.glob(pattern)}
    if "package" in manifest:
        packages.add(root)
    for directory in sorted(packages - excluded):
        yield directory, tomllib.loads((directory / "Cargo.toml").read_text(encoding="utf-8"))


def package_sources(directory):
    """Include custom source layouts, excluding outputs and nested packages."""
    for parent, directories, files in os.walk(directory):
        parent = Path(parent)
        directories[:] = [
            name for name in directories
            if name != ".git" and parent / name not in (directory / "target", directory / "artifacts")
            and not (parent / name / "Cargo.toml").is_file()
        ]
        for name in sorted(files):
            if name.endswith(".rs"):
                yield parent / name


def check(root):
    failures = []
    for directory, manifest in workspace_packages(root):
        if directory == root / "crates/markview-platform":
            continue
        features = manifest.get("features", {})
        if directory == root:
            # The CLI deliberately exposes only `diagnostics` as a source condition.
            features = {"diagnostics"}
        for path in sorted(package_sources(directory)):
            relative = path.relative_to(root).as_posix()
            for line, condition in violations(
                path.read_text(encoding="utf-8"), path == root / "src/main.rs",
                features, EXCEPTIONS.get(relative, ()),
            ):
                failures.append(
                    f"{relative}:{line}: conditional `{condition}` is not allowed here; "
                    "use declared features or move native conditions into markview-platform"
                )
    return failures


def main():
    failures = check(ROOT)
    for failure in failures:
        print(failure)
    if failures:
        print(
            "If a report is a false positive, fix this checker (including its "
            "file-specific exceptions) instead of contorting the reported code "
            "to evade it."
        )
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
