#!/usr/bin/env bash
# Prepare the fuzz seed corpora (C1-C4).
#
# Downloads the official CommonMark, GFM, and KaTeX test cases and extracts
# their inputs into the libFuzzer seed corpora `fuzz/corpus/<target>/`:
#
#   parse/   CommonMark spec examples (652) + GFM spec examples (672),
#            which include the table, strikethrough, tasklist, and
#            autolink extension examples
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
	# fetch URL PATH; retry because the hosts are occasionally slow.
	local url=$1 dest=$2
	for attempt in 1 2 3; do
		if curl -fsSL --max-time 300 "$url" -o "$dest"; then
			return
		fi
		sleep 5
	done
	echo "failed to fetch $url" >&2
	return 1
}

fetch \
	https://raw.githubusercontent.com/commonmark/cmark/master/test/spec.txt \
	"$tmp/cmark-spec.txt"
fetch \
	https://raw.githubusercontent.com/github/cmark-gfm/master/test/spec.txt \
	"$tmp/gfm-spec.txt"
fetch \
	https://raw.githubusercontent.com/KaTeX/KaTeX/master/test/katex-spec.ts \
	"$tmp/katex-spec.ts"

python3 - "$tmp" "$out" "$root/crates/markview-layout/styles" "$root/tests/fixtures" <<'PY'
import pathlib, re, sys

tmp, out, styles = (
	pathlib.Path(sys.argv[1]),
	pathlib.Path(sys.argv[2]),
	pathlib.Path(sys.argv[3]),
)

def spec_examples(path):
	"""The markdown side of the spec's fenced examples: content up to the
	line holding only the separator period."""
	text = path.read_text(encoding="utf-8", errors="replace")
	return [
		m.group(1)
		for m in re.finditer(r"`{20} example\n(.*?)\n\.\n.*?`{20}", text, re.S)
	]

parse_dir = out / "parse"
parse_dir.mkdir(parents=True, exist_ok=True)
count = 0
for i, example in enumerate(spec_examples(tmp / "cmark-spec.txt")):
	(parse_dir / f"cmark-{i:04}.md").write_text(example + "\n")
	count += 1
for i, example in enumerate(spec_examples(tmp / "gfm-spec.txt")):
	(parse_dir / f"gfm-{i:04}.md").write_text(example + "\n")
	count += 1

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
print(f"seeds: parse={sum(1 for _ in parse_dir.iterdir())} math={sum(1 for _ in math_dir.iterdir())} mvss={count} webpage={webpage}")
PY
