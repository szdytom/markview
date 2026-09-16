//! Every recursion depth and work allowance the pipeline draws on.
//!
//! The defaults are chosen so ordinary documents never reach them: a limit is
//! a guard against pathological input, not a policy about how much a user may
//! read. See `docs/security.md` for the threat model.

/// Depth, iteration, and byte budgets for parsing and layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
	/// Maximum inline AST recursion. Comrak builds the AST iteratively, so
	/// this bounds only Markview's own recursion, which needs about 0.5 KB of
	/// stack per level.
	pub inline_depth: usize,
	/// Maximum block nesting retained as structure; deeper content is kept as
	/// source text.
	pub block_depth: usize,
	/// Total candidate line breaks the knapsack pass and its greedy fallback
	/// may evaluate for one paragraph.
	pub linebreak_evaluations: usize,
	/// A single source line longer than this is rendered without syntax
	/// colors instead of being handed to a regex highlighter.
	pub highlight_line_bytes: usize,
	/// Total code bytes highlighted per layout pass; the remainder is shown
	/// uncolored.
	pub highlight_bytes: usize,
	/// A single formula larger than this is reported as a math error.
	pub math_formula_bytes: usize,
	/// Total formula bytes laid out per layout pass.
	pub math_bytes: usize,
	/// Columns retained from a document table.
	pub table_columns: usize,
	/// Rows retained from a document table.
	pub table_rows: usize,
	/// Cells retained from a document table, which caps a wide and long table
	/// at once.
	pub table_cells: usize,
}

impl Default for Limits {
	fn default() -> Self {
		Self {
			inline_depth: 256,
			block_depth: 256,
			linebreak_evaluations: 2_000_000,
			highlight_line_bytes: 64 * 1024,
			highlight_bytes: 16 * 1024 * 1024,
			math_formula_bytes: 256 * 1024,
			math_bytes: 8 * 1024 * 1024,
			table_columns: 256,
			table_rows: 16_384,
			table_cells: 131_072,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn every_budget_is_nonzero_and_generous() {
		let l = Limits::default();
		assert_eq!(l.inline_depth, 256);
		assert_eq!(l.block_depth, 256);
		assert!(l.linebreak_evaluations >= 1_000_000);
		// A 10K-character formula, even in three-byte characters, must fit.
		assert!(l.math_formula_bytes >= 10_000 * 3);
		assert!(l.math_bytes > l.math_formula_bytes);
		assert!(l.table_columns >= 64 && l.table_rows >= 1024);
		assert!(l.table_cells >= l.table_columns);
	}
}
