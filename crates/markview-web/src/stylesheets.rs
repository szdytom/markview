//! Instance-owned parsed MVSS rules and atomic reader selection.
#[cfg(test)]
mod proptests;

use anyhow::{Context, Result, ensure};
use markview_layout::style::{StyleTarget, Stylesheet, StylesheetSource};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub(crate) struct Stylesheets {
	registered: HashMap<String, Arc<StylesheetSource>>,
	pub(crate) explicit: bool,
}

impl Stylesheets {
	pub(crate) fn register(&mut self, id: &str, source: &str) -> Result<()> {
		ensure!(!id.trim().is_empty(), "stylesheet ID must not be blank");
		ensure!(
			!id.starts_with("bundled:"),
			"stylesheet ID {id:?}: bundled: is reserved"
		);
		let sheet = StylesheetSource::parse(source)
			.with_context(|| format!("stylesheet {id:?}"))?;
		Self::validate_target(id, &sheet)?;
		self.registered.insert(id.into(), Arc::new(sheet));
		Ok(())
	}

	fn resolve(&self, id: &str) -> Result<Arc<StylesheetSource>> {
		let sheet = match id.strip_prefix("bundled:") {
			Some(name) => StylesheetSource::named_rules(name),
			None => self.registered.get(id).cloned(),
		}
		.with_context(|| format!("unknown stylesheet ID {id:?}"))?;
		Self::validate_target(id, &sheet)?;
		Ok(sheet)
	}

	fn validate_target(id: &str, sheet: &StylesheetSource) -> Result<()> {
		ensure!(
			sheet.targets.contains(&StyleTarget::Ui),
			"stylesheet {id:?}: targets do not include ui"
		);
		Ok(())
	}

	pub(crate) fn select(
		&mut self,
		ids: &[String],
		default: Arc<Stylesheet>,
	) -> Result<Arc<Stylesheet>> {
		let selected = if ids.is_empty() {
			default
		} else {
			let mut sheet = (*StylesheetSource::builtin()).clone();
			for id in ids.iter().rev() {
				sheet.merge(&*self.resolve(id)?);
			}
			Arc::new(Arc::new(sheet).resolve(default.media()))
		};
		self.explicit = !ids.is_empty();
		Ok(selected)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use markview_layout::style::{Color, Condition, MediaContext};

	const FIRST: &str = "format_version = 3\nversion = 1\n[[rule]]\nwhen = ['body']\ncolor = '#112233'";
	const SECOND: &str = "format_version = 3\nversion = 1\n[[rule]]\nwhen = ['body']\ncolor = '#445566'";

	#[test]
	fn registration_caches_rules_and_replaces_only_after_validation() {
		let mut styles = Stylesheets::default();
		styles.register("light", FIRST).unwrap();
		let cached = styles.resolve("light").unwrap();
		assert!(Arc::ptr_eq(&cached, &styles.resolve("light").unwrap()));
		assert!(!styles.explicit);
		for (id, source) in [
			("light", "invalid TOML"),
			(
				"light",
				"format_version = 3\nversion = 1\ntargets = ['pdf']",
			),
			("bundled:light", FIRST),
			("bundled:custom", FIRST),
			(" ", FIRST),
		] {
			assert!(styles.register(id, source).is_err());
		}
		assert!(Arc::ptr_eq(&cached, &styles.resolve("light").unwrap()));
		styles.register("light", SECOND).unwrap();
		assert!(!Arc::ptr_eq(&cached, &styles.resolve("light").unwrap()));
		assert_eq!(
			cached
				.resolve(MediaContext::default())
				.rule(Condition::Body)
				.color,
			Some(Color(0x112233ff))
		);
		assert!(Stylesheets::default().resolve("light").is_err());
	}

	#[test]
	fn selection_layers_leftmost_first_and_rejects_invalid_ids_atomically() {
		let mut styles = Stylesheets::default();
		styles.register("first", FIRST).unwrap();
		styles.register("second", SECOND).unwrap();
		let default = Stylesheet::bundled(true);
		for id in ["missing", "bundled:print", "bundled:builtin"] {
			assert!(styles.select(&[id.into()], default.clone()).is_err());
			assert!(!styles.explicit);
		}
		let sheet = styles
			.select(
				&["first".into(), "second".into(), "bundled:dark".into()],
				default.clone(),
			)
			.unwrap();
		assert_eq!(sheet.rule(Condition::Body).color, Some(Color(0x112233ff)));
		assert!(styles.explicit);
		assert!(styles.select(&["missing".into()], default.clone()).is_err());
		assert!(styles.explicit);
		assert!(Arc::ptr_eq(
			&default,
			&styles.select(&[], default.clone()).unwrap()
		));
		assert!(!styles.explicit);
	}
}
