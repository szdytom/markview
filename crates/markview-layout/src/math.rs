use crate::{limits::Limits, style::TextAppearance};
use ratex_types::{DisplayList, MathStyle};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug)]
pub struct MathBox {
	pub width: f32,
	pub ascent: f32,
	pub descent: f32,
	pub size: f32,
	pub display: DisplayList,
	/// The resolved `[math]` appearance for a missing KaTeX face.
	pub appearance: Arc<TextAppearance>,
}

type CacheKey = (String, bool, u32, u64);

#[derive(Default)]
pub struct MathEngine {
	appearances: Vec<(u64, Arc<TextAppearance>)>,
	cache: HashMap<CacheKey, Result<Arc<MathBox>, String>>,
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

	fn shared_appearance(
		&mut self,
		appearance: &TextAppearance,
	) -> Arc<TextAppearance> {
		let key = appearance.key();
		if let Some((_, shared)) = self
			.appearances
			.iter()
			.find(|(id, shared)| *id == key && shared.as_ref() == appearance)
		{
			return shared.clone();
		}
		if self.appearances.len() >= 256 {
			self.appearances.clear();
		}
		let shared = Arc::new(appearance.clone());
		self.appearances.push((key, shared.clone()));
		shared
	}

	pub fn layout(
		&mut self,
		latex: &str,
		display: bool,
		size: f32,
		appearance: &TextAppearance,
	) -> Result<Arc<MathBox>, String> {
		let key =
			(latex.to_string(), display, size.to_bits(), appearance.key());
		if let Some(value) = self.cache.get(&key) {
			return value.clone();
		}
		// Bound caches and hostile/accidentally enormous AI-generated formulas.
		if self.cache.len() >= 256 {
			self.cache.clear();
			self.appearances.clear();
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
			let shared_appearance = self.shared_appearance(appearance);
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
				let mut commands = ratex_layout::to_display_list(&layout);
				commands.items.shrink_to_fit();
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
					appearance: shared_appearance.clone(),
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
		let m = e
			.layout(
				r"\frac{x_1}{\sqrt{y}}",
				false,
				18.0,
				&TextAppearance::default(),
			)
			.unwrap();
		assert!(m.width > 0.0 && m.ascent > 0.0 && m.descent > 0.0);
		assert!(!m.display.items.is_empty());
		assert!(
			e.layout(r"\frac{", false, 18.0, &TextAppearance::default())
				.is_err()
		);
		assert!(Arc::ptr_eq(
			&m,
			&e.layout(
				r"\frac{x_1}{\sqrt{y}}",
				false,
				18.0,
				&TextAppearance::default()
			)
			.unwrap()
		));
	}
	#[test]
	fn large_formulas_fit_the_default_budget() {
		let mut e = MathEngine::default();
		let latex =
			format!(r"\frac{{x_1}}{{\sqrt{{y}}}}{}", " + z".repeat(2_500));
		assert!(latex.len() > 10_000);
		assert!(
			e.layout(&latex, false, 18.0, &TextAppearance::default())
				.is_ok()
		);
	}
	#[test]
	fn formula_budgets_reject_without_sticking_in_the_cache() {
		let mut e = MathEngine::default();
		e.set_limits(Limits {
			math_formula_bytes: 2,
			..Default::default()
		});
		assert!(
			e.layout("xxxx", false, 18.0, &TextAppearance::default())
				.is_err()
		);
		e.set_limits(Limits {
			math_bytes: 0,
			..Default::default()
		});
		assert!(
			e.layout("x", false, 18.0, &TextAppearance::default())
				.is_err()
		);
		assert!(e.appearances.is_empty());
		// A new pass gets a fresh allowance, so a resize can still succeed.
		e.set_limits(Limits::default());
		assert!(
			e.layout("x", false, 18.0, &TextAppearance::default())
				.is_ok()
		);
		assert!(
			e.layout("xxxx", false, 18.0, &TextAppearance::default())
				.is_ok()
		);
	}
	/// An oversized `\\char` literal must return an error.
	#[test]
	fn overlong_char_literal_is_an_error_not_a_crash() {
		let mut e = MathEngine::default();
		assert!(
			e.layout(
				r"\char9999999999999999999",
				false,
				18.0,
				&TextAppearance::default()
			)
			.is_err()
		);
		assert!(
			e.layout(
				r#"\char"FFFFFFFFFFFFFFFF"#,
				false,
				18.0,
				&TextAppearance::default()
			)
			.is_err()
		);
	}
	#[test]
	fn formulas_share_equal_appearance_and_seal_commands() {
		let mut engine = MathEngine::default();
		let appearance = TextAppearance::default();
		let x = engine.layout("x", false, 16., &appearance).unwrap();
		let y = engine.layout("y", false, 16., &appearance).unwrap();
		assert!(Arc::ptr_eq(&x.appearance, &y.appearance));
		assert_eq!(x.display.items.len(), x.display.items.capacity());
		let changed = TextAppearance {
			weight: 700,
			..appearance
		};
		let z = engine.layout("z", false, 16., &changed).unwrap();
		assert!(!Arc::ptr_eq(&x.appearance, &z.appearance));
		assert_eq!(x.appearance.weight, 400);
	}
	#[test]
	fn appearance_pool_is_bounded_and_old_formulas_remain_valid() {
		let mut engine = MathEngine::default();
		let first = engine
			.layout("x", false, 16., &TextAppearance::default())
			.unwrap();
		let weak = Arc::downgrade(&first.appearance);
		for weight in 1..=600 {
			let appearance = TextAppearance {
				weight,
				..Default::default()
			};
			engine.shared_appearance(&appearance);
			assert!(engine.appearances.len() <= 256);
		}
		assert_eq!(first.appearance.weight, 400);
		drop(engine);
		assert!(weak.upgrade().is_some());
		drop(first);
		assert!(weak.upgrade().is_none());
	}

	#[test]
	fn sharing_keeps_distinct_float_bits_and_compares_hash_matches() {
		let mut engine = MathEngine::default();
		let appearance = TextAppearance::default();
		let first = engine.shared_appearance(&appearance);
		let changed = TextAppearance {
			letter_spacing: -0.,
			..appearance.clone()
		};
		let other = engine.shared_appearance(&changed);
		assert_eq!(other.letter_spacing.to_bits(), (-0_f32).to_bits());
		assert!(!Arc::ptr_eq(&first, &other));
		let changed = TextAppearance {
			weight: 700,
			..appearance
		};
		engine.appearances = vec![(changed.key(), first.clone())];
		assert_eq!(engine.shared_appearance(&changed).weight, 700);
		assert_eq!(first.weight, 400);
	}
}
