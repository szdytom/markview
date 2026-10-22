//! File and experimental web entry points for the empty reader.
use super::{
	Button, Command, Draw, Lang, Paint, Rect, TextShaper, components, controls,
	ui_appearance,
};
use crate::state::TextField;
use markview_layout::style::{ColorField as C, Condition};

struct Layout {
	rect: Rect,
	short: bool,
	sections: [(Rect, Vec<String>); 2],
	hint: Vec<String>,
	hint_y: f32,
	buttons: Vec<Button>,
}

fn layout(ui: &mut TextShaper, width: f32, height: f32, lang: Lang) -> Layout {
	ui.appearance = ui_appearance(ui);
	let w = (width - 48.0).clamp(0.0, 580.0);
	let x = (width - w) / 2.0;
	let columns = w >= 480.0;
	let section_w = if columns { (w - 40.0) / 2.0 } else { w };
	let mut detail = [
		if markview_platform::touch_layout() {
			lang.empty_open_detail_android()
		} else {
			lang.empty_open_detail()
		},
		lang.empty_web_detail(),
	]
	.map(|text| controls::wrap(ui, text, 13.0, section_w));
	let mut heights = detail
		.each_ref()
		.map(|lines| 54.0 + lines.len() as f32 * 20.0);
	if columns {
		heights = [heights[0].max(heights[1]); 2];
	}
	let compact = height < 500.0;
	let mut section_y = if compact { 56.0 } else { 72.0 };
	let mut web_y = if columns {
		section_y
	} else {
		section_y + heights[0] + 20.0
	};
	let mut hint = controls::wrap(
		ui,
		&lang.empty_paste_hint(if markview_platform::command_key() {
			"⌘V"
		} else {
			"Ctrl+V"
		}),
		13.0,
		w,
	);
	let mut hint_y = web_y + heights[1] + if compact { 28.0 } else { 40.0 };
	let mut total = hint_y + hint.len() as f32 * 20.0 - 16.0;
	let available = height - super::TOP - super::BOTTOM;
	let short = !columns && total + 32.0 > available;
	if short {
		// Keep the URL row visible when the keyboard reduces the viewport.
		detail = [Vec::new(), Vec::new()];
		heights = [36.0, 54.0];
		section_y = 0.0;
		web_y = heights[0]
			+ (available - heights[0] - heights[1]).clamp(16.0, 24.0);
		hint_y = web_y + heights[1] + 28.0;
		total = hint_y + hint.len() as f32 * 20.0 - 16.0;
		if total > available {
			hint.clear();
			total = web_y + heights[1];
		}
	}
	let slack = available - total;
	let y = super::TOP
		+ if short {
			slack.min(slack / 2.0)
		} else {
			(slack / 2.0).max(16.0)
		};
	let [local_detail, web_detail] = detail;
	let sections = [
		(
			Rect {
				x,
				y: y + section_y,
				w: section_w,
				h: heights[0],
			},
			local_detail,
		),
		(
			Rect {
				x: if columns { x + section_w + 40.0 } else { x },
				y: y + web_y,
				w: section_w,
				h: heights[1],
			},
			web_detail,
		),
	];
	let local = sections[0].0;
	let web = sections[1].0;
	let mut open = components::button(
		lang.empty_open_file(),
		Command::Open,
		Rect {
			x,
			y: local.y + local.h - 36.0,
			w: 128.0_f32.min(section_w),
			h: 36.0,
		},
	);
	open.kind = components::ButtonKind::Primary;
	open.id = markview_ui_core::WidgetId::new(2, 1);
	let button_w = (ui.text_width(lang.empty_open_url(), 13.0) + 24.0)
		.max(64.0)
		.min(section_w);
	let input_y = web.y + web.h - 36.0;
	let buttons = vec![
		open,
		components::button(
			lang.empty_url_placeholder(),
			Command::FocusInput(TextField::Url),
			Rect {
				x: web.x,
				y: input_y,
				w: (section_w - button_w - 8.0).max(0.0),
				h: 36.0,
			},
		),
		components::button(
			lang.empty_open_url(),
			Command::OpenUrl,
			Rect {
				x: web.x + section_w - button_w,
				y: input_y,
				w: button_w,
				h: 36.0,
			},
		),
	];
	Layout {
		rect: Rect { x, y, w, h: total },
		short,
		sections,
		hint,
		hint_y: y + hint_y,
		buttons,
	}
}

pub(super) fn buttons(
	ui: &mut TextShaper,
	width: f32,
	height: f32,
	lang: Lang,
) -> Vec<Button> {
	layout(ui, width, height, lang).buttons
}

pub(super) fn draw(
	ui: &mut TextShaper,
	interaction: &crate::app::widgets::Drawn<'_>,
	width: f32,
	height: f32,
	lang: Lang,
) -> Vec<Draw> {
	let l = layout(ui, width, height, lang);
	ui.appearance.weight = 600;
	let heading = ui.appearance.clone();
	let title = ui.fit(lang.empty_open_title(), 25.0, l.rect.w);
	let mut out = if l.short {
		Vec::new()
	} else {
		ui.label_with(
			&title,
			25.0,
			l.rect.x,
			l.rect.y + 26.0,
			&heading,
			Paint::Styled(Condition::Ui, C::Color),
			None,
		)
		.0
	};
	for (i, (rect, detail)) in l.sections.iter().enumerate() {
		if l.short && i == 0 {
			continue;
		}
		ui.appearance = heading.clone();
		let experimental_w = if i == 1 {
			ui.text_width(lang.empty_web_experimental(), 11.0) + 12.0
		} else {
			0.0
		};
		let title = ui.fit(
			if i == 0 {
				lang.empty_local_title()
			} else {
				lang.empty_web_title()
			},
			15.0,
			rect.w - experimental_w,
		);
		let (draws, title_w) = ui.label_with(
			&title,
			15.0,
			rect.x,
			rect.y,
			&heading,
			Paint::Styled(Condition::Ui, C::Color),
			None,
		);
		out.extend(draws);
		ui.appearance = ui_appearance(ui);
		if i == 1 {
			out.extend(ui.label(
				lang.empty_web_experimental(),
				11.0,
				rect.x + title_w + 12.0,
				rect.y,
				Paint::Styled(Condition::Ui, C::Muted),
			));
		}
		for (j, line) in detail.iter().enumerate() {
			out.extend(ui.label(
				line,
				13.0,
				rect.x,
				rect.y + 26.0 + j as f32 * 20.0,
				Paint::Styled(Condition::Ui, C::Muted),
			));
		}
	}
	for (i, line) in l.hint.iter().enumerate() {
		out.extend(ui.label(
			line,
			13.0,
			l.rect.x,
			l.hint_y + i as f32 * 20.0,
			Paint::Styled(Condition::Ui, C::Muted),
		));
	}
	if !markview_platform::touch_layout() {
		let open = l.buttons[0].rect;
		out.extend(ui.centered_label(
			if markview_platform::command_key() {
				"⌘O"
			} else {
				"Ctrl+O"
			},
			12.0,
			open.x + open.w + 16.0,
			open.y + open.h / 2.0,
			Paint::Styled(Condition::Ui, C::Muted),
		));
	}
	for button in l.buttons {
		if !matches!(button.action, Command::FocusInput(_)) {
			out.extend(components::draw_button(ui, interaction, &button, true));
		}
	}
	vec![Draw::Clipped {
		rect: Rect {
			x: 0.0,
			y: super::TOP,
			w: width,
			h: (height - super::TOP - super::BOTTOM).max(0.0),
		},
		draws: out,
	}]
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::state::InteractionState;

	fn drawn(interaction: &InteractionState) -> crate::app::widgets::Drawn<'_> {
		crate::app::widgets::Drawn::new(interaction, None)
	}
	use crate::render::{Renderer, Theme, View};

	#[test]
	fn empty_page_entry_points_fit_and_reflow_without_overlapping() {
		let mut ui = crate::test_support::shaper();
		for lang in [Lang::En, Lang::ZhHans, Lang::ZhHant, Lang::Ja] {
			for (width, height) in [
				(800.0, 600.0),
				(536.0, 300.0),
				(360.0, 480.0),
				(400.0, 300.0),
				(360.0, 240.0),
				(320.0, 200.0),
				(400.0, 180.0),
			] {
				let controls = buttons(&mut ui, width, height, lang);
				assert_eq!(
					controls.iter().map(|b| b.action).collect::<Vec<_>>(),
					[
						Command::Open,
						Command::FocusInput(TextField::Url),
						Command::OpenUrl,
					]
				);
				for (i, b) in controls.iter().enumerate() {
					assert!(
						b.rect.x >= 24.0 && b.rect.x + b.rect.w <= width - 24.0
					);
					assert!(
						b.rect.y >= super::super::TOP
							&& b.rect.y + b.rect.h
								<= height - super::super::BOTTOM
					);
					assert!(b.rect.w > 0.0);
					assert!(
						controls[i + 1..].iter().all(|other| b
							.rect
							.intersect(other.rect)
							.is_none())
					);
				}
				let [file, input, web] = controls.as_slice() else {
					unreachable!()
				};
				assert_eq!(input.rect.y, web.rect.y);
				if width >= 536.0 {
					assert_eq!(file.rect.y, input.rect.y);
				} else {
					assert!(file.rect.y + file.rect.h < input.rect.y);
					assert_eq!(file.rect.x, input.rect.x);
				}
				let draws = draw(
					&mut ui,
					&drawn(&InteractionState::default()),
					width,
					height,
					lang,
				);
				let [Draw::Clipped { rect, draws }] = draws.as_slice() else {
					unreachable!()
				};
				for draw in draws {
					if let Draw::Glyph(g) = draw {
						let face = ttf_parser::Face::parse(
							g.font.data.data(),
							g.font.index,
						)
						.unwrap();
						if let Some(bounds) =
							face.glyph_bounding_box(ttf_parser::GlyphId(g.id))
						{
							let scale = g.size / f32::from(face.units_per_em());
							assert!(
								g.x + f32::from(bounds.x_min) * scale >= rect.x
							);
							assert!(
								g.x + f32::from(bounds.x_max) * scale
									<= rect.x + rect.w
							);
							assert!(
								g.y - f32::from(bounds.y_max) * scale >= rect.y
							);
							assert!(
								g.y - f32::from(bounds.y_min) * scale
									<= rect.y + rect.h
							);
						}
					}
					assert!(
						!matches!(draw, Draw::Box { radius, .. } if *radius != 0.0)
					);
				}
			}
		}
	}

	#[test]
	fn empty_page_frames() -> anyhow::Result<()> {
		let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("artifacts/empty-page");
		std::fs::create_dir_all(&output)?;
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		for dark in [false, true] {
			let sheet = markview_layout::style::Stylesheet::bundled(dark);
			renderer.set_stylesheet(sheet.clone());
			let mut ui = crate::test_support::shaper();
			ui.set_stylesheet(sheet);
			for lang in [Lang::En, Lang::ZhHans, Lang::ZhHant, Lang::Ja] {
				for (width, height) in [
					(800.0, 600.0),
					(360.0, 480.0),
					(400.0, 300.0),
					(360.0, 240.0),
					(320.0, 200.0),
				] {
					let focused = height < 480.0;
					let interaction = InteractionState {
						widgets: markview_ui_core::Interaction {
							focus: focused.then_some(
								Command::FocusInput(TextField::Url).widget_id(),
							),
							..Default::default()
						},
						..Default::default()
					};
					let mut overlay = draw(
						&mut ui,
						&drawn(&interaction),
						width,
						height,
						lang,
					);
					let buttons = buttons(&mut ui, width, height, lang);
					for b in &buttons {
						assert!(
							b.rect.y + b.rect.h
								<= height - super::super::BOTTOM
						);
					}
					let mut input =
						markview_layout::text_input::TextInput::default();
					if focused {
						input.set_text(&mut ui, "https://example.com/article");
					}
					overlay.extend(input.draw(
						&mut ui,
						buttons[1].rect,
						focused,
						focused,
						lang.empty_url_placeholder(),
					));
					let horizontal = std::collections::HashMap::new();
					let view = View {
						width: width as u32,
						height: height as u32,
						scale: 1.0,
						left: 0.0,
						top: 0.0,
						bottom: 0.0,
						scroll: 0.0,
						theme: if dark { Theme::Dark } else { Theme::Light },
						horizontal: &horizontal,
						selection: None,
						revision: 0,
						hovered_link: None,
						hovered_overflow: None,
						held_overflow: None,
					};
					let target = renderer.offscreen(view.width, view.height);
					let submission = renderer.render(
						&Default::default(),
						&view,
						&overlay,
						&target.create_view(&Default::default()),
					)?;
					renderer.wait(Some(submission))?;
					renderer.save_png(
						&target,
						&output.join(format!(
							"{lang:?}-{dark}-{width}-{height}.png"
						)),
					)?;
				}
			}
		}
		Ok(())
	}
}
