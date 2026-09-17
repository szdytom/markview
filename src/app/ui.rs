//! Adapt application state to borrowed chrome inputs.
use super::{App, Button, chrome::Chrome};
use crate::layout::Draw;
impl App {
	/// The remote-image deferral count while its banner is worth showing.
	pub(super) fn remote_notice(&self) -> Option<usize> {
		self.readers.session.remote_notice()
	}

	/// Top of the document area, below the toolbar and any notice strip.
	pub(super) fn content_top(&self) -> f32 {
		super::chrome::content_top(self.remote_notice().is_some())
	}

	fn chrome(&mut self) -> Chrome<'_> {
		let (width, height, _) = self.dimensions();
		let scrollbar = self.document_scrollbar();
		let remote_notice = self.remote_notice();
		// An internal footnote jump has no external target to name, so the
		// footer stays empty while the pointer is over one.
		let hover_hint =
			self.interaction.hover_image.as_deref().or_else(|| {
				self.interaction
					.hover
					.as_deref()
					.filter(|url| !super::anchor::footnote_link(url))
			});
		Chrome {
			ui: &mut self.ui,
			session: &self.readers.session,
			tabs: self.readers.entries(),
			active_tab: self.readers.active(),
			tab_strip: &self.tab_strip,
			tab_widths: &self.tab_metrics.widths,
			settings: &self.preferences.values,
			interaction: &self.interaction,
			style_entries: &self.preferences.style_entries,
			style_page: self.preferences.style_page,
			width,
			height,
			scrollbar,
			warning: self
				.preferences
				.style_warning
				.as_deref()
				.or(self.preferences.settings_warning.as_deref()),
			status: &self.status,
			status_until: self.status_until,
			error: self.error,
			hover_hint,
			remote_notice,
		}
	}
	pub(super) fn buttons(&mut self) -> Vec<Button> {
		self.chrome().buttons()
	}
	pub(super) fn overlay(&mut self) -> Vec<Draw> {
		self.normalize_tab_scroll();
		let session = &self.readers.session;
		let selection = self.interaction.selection.filter(|s| {
			!s.is_empty()
				&& s.anchor.revision == session.accepted_revision
				&& s.focus.revision == session.accepted_revision
		});
		if self.interaction.selection_counts.map(|(s, _)| s) != selection {
			self.interaction.selection_counts = selection.map(|s| {
				(
					s,
					markview_core::text::TextCounts::of(
						&session
							.snapshot
							.extract_text(s, session.accepted_revision),
					),
				)
			});
		}
		self.chrome().overlay()
	}
	pub(super) fn tab_layout(&mut self) -> super::tab_strip::TabLayout {
		self.tab_metrics.sync(&mut self.ui, self.readers.entries());
		self.chrome().tab_bar().layout()
	}
}
