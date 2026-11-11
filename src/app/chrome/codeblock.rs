//! Code copy controls share their geometry between paint and input.
use super::{Button, Chrome, components, icons};
use crate::layout::{Draw, Rect};
use crate::state::{Command, DocumentCommand};

impl Chrome<'_> {
	pub(super) fn codeblock_buttons(&self) -> Vec<Button> {
		if !self.settings.model.codeblock_copy
			|| self.session.doc.layout_pending
			|| self.interaction.panel_open()
			|| self.interaction.overlays.modal_open()
			|| self.interaction.overlays.viewer_open()
			|| self.interaction.overlays.quick_open.open()
			|| self.interaction.overlays.outline.open()
		{
			return Vec::new();
		}
		let clip = self.reading_clip();
		let snapshot = &self.session.doc.display.snapshot;
		let inset = if super::folder::docked(
			self.frame.width,
			self.session.strip.phone,
		) {
			clip.x
		} else {
			0.
		};
		let left =
			inset + ((self.frame.width - inset - snapshot.width) / 2.).max(20.);
		let scroll = self.session.doc.scrolling.offset;
		let mut buttons = Vec::new();
		let start = snapshot
			.blocks
			.partition_point(|block| block.y + block.height() < scroll);
		for (bi, block) in snapshot
			.blocks
			.iter()
			.enumerate()
			.skip(start)
			.take_while(|(_, block)| block.y <= scroll + clip.h)
		{
			for ni in 0..block.layout.text.len() {
				let Some(bounds) = block.codeblock_rect(ni) else {
					continue;
				};
				let rect = Rect {
					x: left + bounds.x + bounds.w - 28.,
					y: clip.y + block.y + bounds.y - scroll,
					w: 28.,
					h: 28.,
				};
				if rect.x < clip.x
					|| rect.y < clip.y
					|| rect.x + rect.w > clip.x + clip.w
					|| rect.y + rect.h > clip.y + clip.h
				{
					continue;
				}
				let mut button = components::button(
					"",
					Command::Document(DocumentCommand::CopyCode {
						revision: snapshot.revision.get(),
						block: bi,
						node: ni,
					}),
					rect,
				);
				button.icon = Some(icons::COPY);
				button.clip = Some(clip);
				buttons.push(button);
			}
		}
		buttons
	}

	pub(super) fn draw_codeblock_buttons(&mut self, out: &mut Vec<Draw>) {
		for button in self.codeblock_buttons() {
			out.extend(components::draw_icon_button(
				self.ui,
				&self.interaction,
				&button,
				16.,
			));
		}
	}
}
