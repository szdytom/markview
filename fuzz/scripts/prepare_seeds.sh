#!/usr/bin/env bash
# Prepare the fuzz seed corpora (C1-C4).
#
# Shallow-clones the official CommonMark, GFM, and KaTeX repositories into a
# temporary directory and takes the one file each campaign needs. The local
# script first prepares the deterministic integration, highlight, seam and
# project-dialect seeds; the cloned spec examples are then distributed to
# every target whose unit is Markdown so the structural oracles start from real
# documents instead of an empty corpus.
#
#   parse/   CommonMark and GFM spec examples, plus the project fixtures and
#            dialect seeds (also copied to prefix, refdef, sourcepos_content,
#            details_structure, anchors, layout, layout_parallel and pdf)
#   math/    LaTeX inputs from KaTeX's parser and builder test suite
#   mvss/    the built-in stylesheets of `markview-layout`
#
# Seeds are inputs, not run artifacts: they live with the corpus, stay out of
# the repository (`fuzz/corpus` is gitignored), and are rebuilt on demand.
# Run it before the first campaign: `fuzz/scripts/prepare_seeds.sh`.

set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
out="$root/fuzz/corpus"
python3 "$root/fuzz/scripts/prepare_pipeline_seeds.py"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

fetch() {
	# fetch URL REPO FILE DEST: shallow-clone URL into REPO and take FILE.
	local url=$1 repo=$2 path=$3 dest=$4
	for attempt in 1 2 3; do
		rm -rf "$repo"
		if git clone --depth 1 --single-branch --filter=blob:none \
			--no-checkout "$url" "$repo" &&
			git -C "$repo" show "HEAD:$path" >"$dest"; then
			return
		fi
		sleep 5
	done
	echo "failed to fetch $url ($path)" >&2
	return 1
}

fetch \
	https://github.com/commonmark/cmark.git \
	"$tmp/cmark" test/spec.txt \
	"$tmp/cmark-spec.txt"
fetch \
	https://github.com/github/cmark-gfm.git \
	"$tmp/cmark-gfm" test/spec.txt \
	"$tmp/gfm-spec.txt"
fetch \
	https://github.com/KaTeX/KaTeX.git \
	"$tmp/katex" test/katex-spec.ts \
	"$tmp/katex-spec.ts"

python3 "$root/fuzz/scripts/prepare_pipeline_seeds.py" --spec-dir "$tmp"

python3 - "$tmp" "$out" "$root/crates/markview-layout/styles" "$root/tests/fixtures" <<'PY'
import pathlib, re, sys

tmp, out, styles = (
	pathlib.Path(sys.argv[1]),
	pathlib.Path(sys.argv[2]),
	pathlib.Path(sys.argv[3]),
)

math_dir = out / "math"
math_dir.mkdir(parents=True, exist_ok=True)
katex = (tmp / "katex-spec.ts").read_text(encoding="utf-8")
# The test suite feeds LaTeX through template literals and string arguments;
# both carry the exact input the reference parser must accept.
tex_inputs = re.findall(
	r"expect(?:\(\s*([\"'])(.*?)\1\s*\)|`((?:[^`\\]|\\.)*)`)",
	katex,
	re.S,
)
count = 0
for raw, quoted, literal in tex_inputs:
	example = literal or quoted
	if not example.strip() or "${" in example:
		continue
	(math_dir / f"katex-{count:04}.tex").write_text(example + "\n")
	count += 1

# The committed web fixtures are local seeds: no download, and they already
# train the extractor's cleanup passes.
webpage_dir = out / "webpage"
webpage_dir.mkdir(parents=True, exist_ok=True)
fixtures = pathlib.Path(sys.argv[4])
for page in sorted(fixtures.glob("web-*.html")):
	(webpage_dir / page.name).write_text(page.read_text(encoding="utf-8"))

mvss_dir = out / "mvss"
mvss_dir.mkdir(parents=True, exist_ok=True)
count = 0
for style in sorted(styles.glob("*.mvss.toml")):
	(mvss_dir / f"builtin-{style.stem}.mvss").write_text(style.read_text())
	count += 1

webpage = sum(1 for _ in webpage_dir.iterdir())
print(f"seeds: math={sum(1 for _ in math_dir.iterdir())} mvss={count} webpage={webpage}")
PY
