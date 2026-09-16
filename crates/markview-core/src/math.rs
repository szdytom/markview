use crate::limits::Limits;
use ratex_types::{DisplayList, MathStyle};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug)]
pub struct MathBox {
	pub width: f32,
	pub ascent: f32,
	pub descent: f32,
	pub size: f32,
	pub display: DisplayList,
}

#[derive(Default)]
pub struct MathEngine {
	cache: HashMap<(String, bool, u32), Result<Arc<MathBox>, String>>,
	limits: Limits,
	/// Formula bytes laid out since `set_limits`, which resets per pass.
	used: usize,
}

impl MathEngine {
	/// Applies the pass budgets. A new pass starts with a fresh allowance, so
	/// a resize can still lay out formulas the previous pass refused.
	pub fn set_limits(&mut self, limits: Limits) {
		self.limits = limits;
		self.used = 0;
	}

	pub fn layout(
		&mut self,
		latex: &str,
		display: bool,
		size: f32,
	) -> Result<Arc<MathBox>, String> {
		let key = (latex.to_string(), display, size.to_bits());
		if let Some(value) = self.cache.get(&key) {
			return value.clone();
		}
		// Bound caches and hostile/accidentally enormous AI-generated formulas.
		if self.cache.len() >= 256 {
			self.cache.clear();
		}
		let over_budget =
			self.used.saturating_add(latex.len()) > self.limits.math_bytes;
		let over_limit =
			over_budget || latex.len() > self.limits.math_formula_bytes;
		let result = if latex.len() > self.limits.math_formula_bytes {
			Err("Formula exceeds the size budget".into())
		} else if over_budget {
			Err("Document formula budget exceeded".into())
		} else {
			std::panic::catch_unwind(|| {
				let ast =
					ratex_parser::parse(latex).map_err(|e| e.to_string())?;
				let opts = ratex_layout::LayoutOptions {
					style: if display {
						MathStyle::Display
					} else {
						MathStyle::Text
					},
					..Default::default()
				};
				let layout = ratex_layout::layout(&ast, &opts);
				let commands = ratex_layout::to_display_list(&layout);
				let width = commands.width as f32 * size;
				let ascent = commands.height as f32 * size;
				let descent = commands.depth as f32 * size;
				if ![width, ascent, descent]
					.iter()
					.all(|x| x.is_finite() && *x >= 0.0 && *x < 1e6)
				{
					return Err("Formula has invalid dimensions".into());
				}
				Ok(Arc::new(MathBox {
					width,
					ascent,
					descent,
					size,
					display: commands,
				}))
			})
			.unwrap_or_else(|_| Err("Formula could not be laid out".into()))
		};
		match &result {
			Ok(_) => {
				self.used = self.used.saturating_add(latex.len());
				self.cache.insert(key, result.clone());
			}
			// A pass budget must not stick in the cache: the next pass gets a
			// fresh allowance.
			Err(_) if over_limit => {}
			Err(_) => {
				self.cache.insert(key, result.clone());
			}
		}
		result
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn baseline_and_errors() {
		let mut e = MathEngine::default();
		let m = e.layout(r"\frac{x_1}{\sqrt{y}}", false, 18.0).unwrap();
		assert!(m.width > 0.0 && m.ascent > 0.0 && m.descent > 0.0);
		assert!(!m.display.items.is_empty());
		assert!(e.layout(r"\frac{", false, 18.0).is_err());
		assert!(Arc::ptr_eq(
			&m,
			&e.layout(r"\frac{x_1}{\sqrt{y}}", false, 18.0).unwrap()
		));
	}
	#[test]
	fn large_formulas_fit_the_default_budget() {
		let mut e = MathEngine::default();
		let latex =
			format!(r"\frac{{x_1}}{{\sqrt{{y}}}}{}", " + z".repeat(2_500));
		assert!(latex.len() > 10_000);
		assert!(e.layout(&latex, false, 18.0).is_ok());
	}
	#[test]
	fn formula_budgets_reject_without_sticking_in_the_cache() {
		let mut e = MathEngine::default();
		e.set_limits(Limits {
			math_formula_bytes: 2,
			..Default::default()
		});
		assert!(e.layout("xxxx", false, 18.0).is_err());
		e.set_limits(Limits {
			math_bytes: 0,
			..Default::default()
		});
		assert!(e.layout("x", false, 18.0).is_err());
		// A new pass gets a fresh allowance, so a resize can still succeed.
		e.set_limits(Limits::default());
		assert!(e.layout("x", false, 18.0).is_ok());
		assert!(e.layout("xxxx", false, 18.0).is_ok());
	}
}
