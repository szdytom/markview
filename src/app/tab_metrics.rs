//! Cache filename measurements so pointer motion never shapes offscreen tabs.
use crate::{layout::TextShaper, state::ReaderTab};
use markview_layout::style::{Condition, Stylesheet, TextAppearance};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;

/// The `x` inset from a tab's left edge to its label.
pub(super) const LABEL_START: f32 = 12.0;
/// The offset from a tab's right edge to its close control's leading edge.
pub(super) const CLOSE_INSET: f32 = 22.0;
/// The distance from a tab's right edge at which the close hover target
/// starts.
pub(super) const CLOSE_HOVER: f32 = 24.0;
/// The width a tab spends beside its label, shared with the chrome that lays
/// those parts out, so retuning an inset retunes the minimum width and label
/// budget with it.
pub(super) const TAB_CHROME: f32 = LABEL_START + CLOSE_INSET + 2.0;

#[derive(Default)]
pub(super) struct TabMetrics {
	span: super::tab_strip::TabSpan,
	sheet: Option<Arc<Stylesheet>>,
	order: Vec<PathBuf>,
	measured: HashMap<PathBuf, (f32, f32)>,
	pub widths: Vec<(f32, f32)>,
}
impl TabMetrics {
	pub fn end(&self, viewport: crate::layout::Rect, scroll: f32) -> f32 {
		self.span.end(viewport, scroll)
	}
	pub fn sync(&mut self, ui: &mut TextShaper, tabs: &[ReaderTab]) {
		if self
			.sheet
			.as_ref()
			.is_none_or(|sheet| !Arc::ptr_eq(sheet, &ui.stylesheet))
		{
			self.order.clear();
			self.widths.clear();
			self.span = Default::default();
			self.measured.clear();
			self.sheet = Some(ui.stylesheet.clone());
		}
		if self.order.len() == tabs.len()
			&& self
				.order
				.iter()
				.zip(tabs)
				.all(|(path, tab)| *path == tab.path)
		{
			return;
		}
		self.order = tabs.iter().map(|tab| tab.path.clone()).collect();
		let paths: std::collections::HashSet<_> =
			tabs.iter().map(|tab| &tab.path).collect();
		self.measured.retain(|path, _| paths.contains(path));
		let old = ui.appearance.clone();
		ui.appearance = tab_appearance(ui);
		self.widths.clear();
		for tab in tabs {
			let widths =
				*self.measured.entry(tab.path.clone()).or_insert_with(|| {
					let name = tab
						.path
						.file_name()
						.unwrap_or(tab.path.as_os_str())
						.to_string_lossy();
					let prefix: String = name.graphemes(true).take(2).collect();
					let minimum = ui
						.text_width("汉字", 12.0)
						.max(ui.text_width(&prefix, 12.0))
						+ TAB_CHROME;
					let natural = (ui.text_width(&name, 12.0) + TAB_CHROME)
						.clamp(92.0, 240.0)
						.max(minimum);
					(natural, minimum)
				});
			self.widths.push(widths);
		}
		ui.appearance = old;
		self.span = super::tab_strip::TabSpan::new(&self.widths);
	}
}
pub(super) fn tab_appearance(ui: &TextShaper) -> TextAppearance {
	ui.stylesheet.text(
		&ui.stylesheet
			.text(&TextAppearance::default(), Condition::Ui),
		Condition::Toolbar,
	)
}
