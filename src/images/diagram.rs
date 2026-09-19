//! Mermaid diagrams, rendered to SVG on the image workers.
//!
//! `mermaid-rs-renderer` discovers system fonts lazily on its first text
//! measurement, so importing it costs nothing until a diagram is rendered.
use anyhow::Result;
use std::{
	collections::{HashMap, VecDeque},
	sync::{Arc, Mutex, OnceLock},
};

/// Diagrams kept rendered per process. Diagram source is small, so a count cap
/// bounds the SVG memory while a reflow or a reopened file never re-renders.
const CACHE_CAPACITY: usize = 32;

/// Node, edge and subgraph budget for one diagram. The layout recurses once
/// per node along a path (its strongly-connected-component DFS, for example),
/// so a chain of `N` nodes costs about `N` frames. Measured in a debug build
/// on a default 2 MiB stack, a 2000-node chain fit in 512 KiB but overflowed
/// 256 KiB, putting one frame between 131 and 262 bytes and the exhaustion
/// point near 10,000 nodes. A node and an edge count equally here, so a simple
/// path spends half the budget on nodes and recurses at most 256 times, under
/// 70 KiB. Subgraphs share the budget because nesting is walked recursively
/// too.
pub(super) const MAX_GRAPH_ELEMENTS: usize = 512;

/// Source cap checked before parsing, so an oversized fence costs nothing and
/// a pathological one cannot abort a worker. It is also the ultimate bound on
/// the dependency's recursion: a recursive descent consumes at least one byte
/// per frame to terminate, so no traversal can be deeper than the source is
/// long. [`RENDER_STACK_BYTES`] is sized for that whole range rather than for
/// one grammar's nesting cost.
pub(super) const MAX_SOURCE_BYTES: usize = 8 * 1024;

/// Brace-nesting cap for label markup. The text normalizer recurses once per
/// `{...}` group it rewrites, so this bounds that recursion directly and turns
/// a pathological label into a readable error before any layout runs. Real
/// diagrams nest a handful of groups; the byte cap would otherwise allow
/// thousands.
pub(super) const MAX_LABEL_NESTING: usize = 64;

/// Stack for the thread that runs the layout and render stages.
///
/// The source cap is the ultimate bound on every recursive descent in the
/// dependency: a frame consumes at least one byte before it can return, so no
/// traversal is deeper than the 8192 bytes the cap allows. A debug build (the
/// largest frames) was measured at about 1.8 KiB per nesting level, so the
/// worst case a one-byte-per-frame recursion could reach is under 15 MiB;
/// 32 MiB covers that with room for frames up to 4 KiB. The graph traversals
/// are separately bounded by [`MAX_GRAPH_ELEMENTS`] at a few hundred bytes per
/// frame, and [`MAX_LABEL_NESTING`] rejects deep brace markup before this
/// backstop is needed at all.
const RENDER_STACK_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct Cache {
	svgs: HashMap<String, Arc<str>>,
	order: VecDeque<String>,
}

fn cache() -> &'static Mutex<Cache> {
	static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
	CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Whether a parsed diagram of `elements` nodes, edges and subgraphs is small
/// enough for the layout's recursive traversals.
pub(super) fn within_graph_budget(elements: usize) -> bool {
	elements <= MAX_GRAPH_ELEMENTS
}

/// Whether `code`'s `{...}` nesting stays inside the normalizer's budget. The
/// scan is over the raw source, so it also catches markup assembled inside a
/// quoted label; `}` is allowed to close more than it opened so a stray one
/// never underflows.
pub(super) fn within_nesting_budget(code: &str) -> bool {
	let mut depth = 0usize;
	let mut deepest = 0usize;
	for byte in code.bytes() {
		match byte {
			b'{' => {
				depth += 1;
				deepest = deepest.max(depth);
			}
			b'}' => depth = depth.saturating_sub(1),
			_ => {}
		}
	}
	deepest <= MAX_LABEL_NESTING
}

/// Renders through the dependency's three stages instead of its `render`
/// wrapper, so the parsed graph can be bounded before layout walks it. A
/// stack overflow cannot be caught, so the bounds run first and the recursion
/// runs on a thread whose stack covers the worst case the byte cap allows.
/// The thread is joined before this returns, so none is left behind.
fn render_bounded(code: &str) -> Result<String> {
	if code.len() > MAX_SOURCE_BYTES {
		anyhow::bail!(
			"Mermaid: diagram source exceeds {} KiB",
			MAX_SOURCE_BYTES / 1024
		);
	}
	if !within_nesting_budget(code) {
		anyhow::bail!(
			"Mermaid: label markup nests deeper than {MAX_LABEL_NESTING}"
		);
	}
	std::thread::scope(|scope| {
		std::thread::Builder::new()
			.name("markview-mermaid".into())
			.stack_size(RENDER_STACK_BYTES)
			.spawn_scoped(scope, || -> Result<String> {
				let parsed = mermaid_rs_renderer::parse_mermaid_strict(code)
					.map_err(|e| anyhow::anyhow!("Mermaid: {e}"))?;
				let graph = &parsed.graph;
				let elements = graph.nodes.len()
					+ graph.edges.len()
					+ graph.subgraphs.len();
				if !within_graph_budget(elements) {
					anyhow::bail!(
						"Mermaid: diagram has {elements} graph elements, \
						 the limit is {MAX_GRAPH_ELEMENTS}"
					);
				}
				let options = mermaid_rs_renderer::RenderOptions::default();
				let layout = mermaid_rs_renderer::compute_layout(
					graph,
					&options.theme,
					&options.layout,
				);
				Ok(mermaid_rs_renderer::render_svg(
					&layout,
					&options.theme,
					&options.layout,
				))
			})
			.expect("start Mermaid render")
			.join()
			.unwrap_or_else(|_| {
				Err(anyhow::anyhow!("Mermaid: renderer panicked"))
			})
	})
}

/// The SVG for a diagram source, rendering it on first use. The render runs
/// outside the lock, so independent diagrams do not serialize on the cache.
pub(super) fn svg(code: &str) -> Result<Arc<str>> {
	if let Some(svg) = cache().lock().unwrap().svgs.get(code) {
		return Ok(svg.clone());
	}
	let svg: Arc<str> = render_bounded(code)?.into();
	let mut cache = cache().lock().unwrap();
	if let Some(existing) = cache.svgs.get(code) {
		return Ok(existing.clone());
	}
	while cache.order.len() >= CACHE_CAPACITY {
		if let Some(oldest) = cache.order.pop_front() {
			cache.svgs.remove(&oldest);
		}
	}
	cache.order.push_back(code.to_owned());
	cache.svgs.insert(code.to_owned(), svg.clone());
	Ok(svg)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn renders_each_source_once() {
		let code = "graph TD\n X[one]-->Y[two]\n";
		let first = svg(code).unwrap();
		assert!(first.contains("<svg"));
		assert!(Arc::ptr_eq(&first, &svg(code).unwrap()));
	}

	#[test]
	fn malformed_source_is_an_error() {
		assert!(svg("flowchart LR\n--> B\n").is_err());
	}

	#[test]
	fn over_budget_graph_is_rejected_before_layout() {
		// 300 edges with 301 nodes is over the element budget but well under
		// the source cap, so this exercises the graph check on its own.
		let mut code = String::from("flowchart TD\n");
		for i in 0..300 {
			code.push_str(&format!("N{i}-->N{}\n", i + 1));
		}
		let error = svg(&code).unwrap_err().to_string();
		assert!(error.contains("graph elements"), "{error}");
	}
}
