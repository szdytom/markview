#!/usr/bin/env python3
"""Compare `mvfuzz`'s `doc_memory` allocator samples from two builds."""
import argparse
import json
from pathlib import Path
from statistics import median


def summarize(report):
    corpora = sorted({s["corpus"] for s in report["samples"]})
    return {
        corpus: {
            key: {"median": median(values), "min": min(values), "max": max(values)}
            for key in report["samples"][0]
            if key not in ("corpus", "iteration")
            and (values := [s[key] for s in report["samples"] if s["corpus"] == corpus])
            and all(isinstance(v, (int, float)) for v in values)
        }
        for corpus in corpora
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--plot", action="store_true")
    args = parser.parse_args()
    old_env = json.loads((args.before / "environment.json").read_text())
    new_env = json.loads((args.after / "environment.json").read_text())
    if old_env != new_env:
        differing = sorted(key for key in old_env.keys() | new_env.keys() if old_env.get(key) != new_env.get(key))
        parser.error("measurement environments differ: " + ", ".join(differing))
    before = json.loads((args.before / "memory.json").read_text())
    after = json.loads((args.after / "memory.json").read_text())
    a, b = summarize(before), summarize(after)
    assert a.keys() == b.keys(), "measurement corpora differ"
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "comparison.json").write_text(json.dumps({"before": a, "after": b}, indent=2) + "\n")
    lines = ["# Document memory measurements", "", "Five samples per build and corpus; medians shown. Heap bytes are allocator-requested live bytes, excluding mapped font files and allocator bookkeeping. Cold live includes the parsed source/map, document, engine caches and snapshot. Resize retains the previous snapshot. Repeated templates and existing long fixtures are reported separately.", "", "| Corpus | Cold live before (KiB) | After (KiB) | Reduction | Resize coexist before (KiB) | After (KiB) |", "| --- | ---: | ---: | ---: | ---: | ---: |"]
    for corpus in a:
        old, new = a[corpus]["cold_live_bytes"]["median"], b[corpus]["cold_live_bytes"]["median"]
        resize = [d[corpus]["resize_coexist_bytes"]["median"] / 1024 for d in (a, b)]
        lines.append(f"| {corpus} | {old / 1024:.1f} | {new / 1024:.1f} | {100 * (1 - new / old):.1f}% | {resize[0]:.1f} | {resize[1]:.1f} |")
    lines += ["", "| Corpus | Cold peak before (KiB) | After (KiB) | Resize peak before (KiB) | After (KiB) |", "| --- | ---: | ---: | ---: | ---: |"]
    for corpus in a:
        peaks = [d[corpus][key]["median"] / 1024 for key in ("cold_peak_bytes", "resize_peak_bytes") for d in (a, b)]
        lines.append(f"| {corpus} | " + " | ".join(f"{v:.1f}" for v in peaks) + " |")
    lines += ["", "| Corpus | Semantic tree/index/text before (KiB) | After (KiB) | Source map before/after (KiB) | Original source before/after (KiB) |", "| --- | ---: | ---: | --- | --- |"]
    for corpus in a:
        semantic = [d[corpus]["semantic_tree_index_text_bytes"]["median"] / 1024 for d in (a, b)]
        source_map = [d[corpus]["source_map_bytes"]["median"] / 1024 for d in (a, b)]
        source = [d[corpus]["original_source_bytes"]["median"] / 1024 for d in (a, b)]
        lines.append(f"| {corpus} | {semantic[0]:.1f} | {semantic[1]:.1f} | {source_map[0]:.1f} / {source_map[1]:.1f} | {source[0]:.1f} / {source[1]:.1f} |")
    lines += ["", "| Corpus | Cold allocation calls before | After | Live allocations after warm before | After | Released residual median [max], before/after (B) |", "| --- | ---: | ---: | ---: | ---: | --- |"]
    for corpus in a:
        old, new = a[corpus], b[corpus]
        lines.append(f"| {corpus} | {old['cold_allocations']['median']} | {new['cold_allocations']['median']} | {old['live_allocations']['median']} | {new['live_allocations']['median']} | {old['after_release_bytes']['median']} [{old['after_release_bytes']['max']}] / {new['after_release_bytes']['median']} [{new['after_release_bytes']['max']}] |")
    lines += ["", "| Type | Before (B) | After (B) |", "| --- | ---: | ---: |"]
    lines += [f"| {key} | {before['sizes'][key]} | {after['sizes'][key]} |" for key in before["sizes"]]
    lines += ["", "Timing distributions (microseconds, median [min, max]); these are observations, not CI thresholds.", "", "| Corpus | Operation | Before | After |", "| --- | --- | ---: | ---: |"]
    for corpus in a:
        for key in ("parse_us", "first_screen_us", "cold_us", "warm_us", "resize_us"):
            if key in a[corpus] and key in b[corpus]:
                old, new = a[corpus][key], b[corpus][key]
                lines.append(f"| {corpus} | {key} | {old['median']} [{old['min']}, {old['max']}] | {new['median']} [{new['min']}, {new['max']}] |")
    (args.output / "report.md").write_text("\n".join(lines) + "\n")
    if args.plot:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        names = list(a)
        fig, ax = plt.subplots(figsize=(10, 5))
        positions = list(range(len(names)))
        for offset, data, label, color in [(-0.2, a, "Before", "#9ca3af"), (0.2, b, "After", "#087e8b")]:
            ax.barh([i + offset for i in positions], [data[n]["cold_live_bytes"]["median"] / 1024 for n in names], height=0.36, label=label, color=color)
        ax.set_yticks(positions, names)
        ax.invert_yaxis()
        ax.set_xlabel("Live heap after complete layout (KiB; median of 5)")
        ax.set_title("Document and retained layout memory")
        ax.spines[["top", "right"]].set_visible(False)
        ax.legend(frameon=False)
        fig.tight_layout()
        fig.savefig(args.output / "memory.png", dpi=180)
        fig.savefig(args.output / "memory.svg")
        plt.close(fig)
    print(args.output / "report.md")


if __name__ == "__main__":
    main()
