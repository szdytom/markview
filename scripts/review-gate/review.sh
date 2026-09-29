#!/usr/bin/env bash
# Gate one change behind a review by gpt-6-astra at medium reasoning effort.
#
# The prompt is read from stdin and should name the requirement IDs under
# review, from `docs/plugin-requirements.md`, and end by asking for a final
# `VERDICT: approve` or `VERDICT: reject` line.
#
# The review is the CLI's own review agent (`codex review`), so
# it inspects the staged, unstaged and untracked changes itself and answers
# findings-first rather than with prose. It is expected to leave the tree as it
# found it; anything it writes is a finding in its own right.
#
# Two details make it work here, and both are macOS-specific hazards:
#
# - `CODEX_HOME` is relocated into the repository, because the CLI otherwise
#   needs write access to `~/.codex` for its IPC socket, logs, and caches. The
#   relocated home holds a copy of the CLI credentials, so it is added to
#   `.git/info/exclude`, which is per-clone and never committed.
# - The CLI keeps its own sandbox off. Its `sandbox-exec` policy cannot be
#   applied inside another sandbox, so nesting one makes every file read fail
#   with `sandbox_apply: Operation not permitted`. The boundary is therefore
#   the caller's: run this from an environment already confined to the
#   workspace, and the reviewer inherits exactly that scope.
#
# Exit status: 0 the review approved, 1 it rejected, 2 the review did not run
# or produced no usable verdict. On 1 the findings are printed and the work
# goes back to the implementer.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
home_dir="$repo/.codex-home"

mkdir -p "$home_dir"
if [ ! -f "$home_dir/auth.json" ]; then
	cp "$HOME/.codex/auth.json" "$home_dir/auth.json"
	chmod 600 "$home_dir/auth.json"
fi
# The exclude file is per-clone, so a fresh checkout has to be told again.
exclude="$repo/.git/info/exclude"
if ! grep -qxF '.codex-home/' "$exclude" 2>/dev/null; then
	printf '.codex-home/\n' >>"$exclude"
fi

prompt="$(cat)"
if [ -z "$prompt" ]; then
	echo "review gate: no prompt on stdin" >&2
	exit 2
fi

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# No scope flag: the review agent reviews the working tree, and a scope flag
# cannot be combined with the instructions that say what to decide.
if ! printf '%s\n' "$prompt" | CODEX_HOME="$home_dir" codex review \
	-c 'model="gpt-6-astra"' \
	-c 'model_reasoning_effort="medium"' \
	-c 'sandbox_mode="danger-full-access"' \
	- >"$scratch/log.txt" 2>&1; then
	cat "$scratch/log.txt" >&2
	echo "review gate: the reviewer did not run" >&2
	exit 2
fi

python3 - "$scratch/log.txt" <<'PY'
import re
import sys

try:
	with open(sys.argv[1]) as handle:
		review = handle.read()
except OSError as error:
	print(f"review gate: unusable review: {error}", file=sys.stderr)
	sys.exit(2)

print(review.rstrip())

# The review agent is asked for a verdict line. Without one its findings are
# what decides, and findings are written as `[P0]`..`[P3]`.
# The verdict is asked for as the reviewer's last word, and a reviewer may end
# its summary with it rather than start a line with it. Only the tail is read:
# the log opens with this script's own prompt, which names both verdicts, and
# the review itself may quote an earlier round's.
tail = "\n".join(review.splitlines()[-40:])
marked = re.findall(r"VERDICT:\s*(approve|reject)\b", tail, re.IGNORECASE)
if marked:
	approve = marked[-1].lower() == "approve"
elif re.search(r"^\s*No findings\.?\s*$", review, re.IGNORECASE | re.MULTILINE):
	approve = True
elif re.search(r"\[P[0-3]\]", review):
	approve = False
else:
	print("review gate: no verdict and no findings", file=sys.stderr)
	sys.exit(2)

print()
print(f"review gate: {'approved' if approve else 'rejected'}")
sys.exit(0 if approve else 1)
PY
