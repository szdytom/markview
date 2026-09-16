//! Window and deterministic diagnostic entry points.
use crate::cli::{Mode, arguments};
use crate::{
	benchmark, document,
	file::read_document,
	layout::LayoutEngine,
	render::{Renderer, Theme, View},
};
use anyhow::{Result, bail};
use std::collections::HashMap;
use winit::event_loop::{ControlFlow, EventLoop};

use super::{App, Event};

pub(super) fn run() -> Result<()> {
	let Some(mut args) = arguments()? else {
		return Ok(());
	};
	if let Some((source, force)) = &args.install {
		let dir = crate::stylesheet::directory()
			.ok_or_else(|| anyhow::anyhow!("No user stylesheet directory"))?;
		let (id, path) = crate::stylesheet::install(source, &dir, *force)?;
		println!("Installed {id}: {}", path.display());
		return Ok(());
	}
	let ids = args.style.clone().or_else(|| {
		args.theme.map(|t| {
			vec![if t == Theme::Dark { "dark" } else { "light" }.into()]
		})
	});
	if let Some(ids) = &ids {
		args.options.stylesheet = crate::stylesheet::load(
			ids,
			crate::stylesheet::directory().as_deref(),
		)?;
	}
	if args.mode == Mode::Render || args.mode == Mode::Bench {
		args.options.width = args
			.options
			.width
			.min(args.width as f32 / args.scale - 32.0)
			.max(80.0);
		let path = args.path.as_ref().unwrap();
		if args.mode == Mode::Bench {
			return benchmark::run(
				path,
				args.output.as_deref(),
				args.width,
				args.height,
				args.scale,
				args.theme.unwrap_or_default(),
				args.iterations,
				args.options,
				args.offline,
			);
		}
		let mut renderer = pollster::block_on(Renderer::new(None))?;
		renderer.set_stylesheet(args.options.stylesheet.clone());
		let mut engine = LayoutEngine::new();
		engine.validate_stylesheet(&args.options.stylesheet)?;
		let doc = document::parse(read_document(path)?);
		let mut images = crate::images::Images::new(args.offline);
		images.prepare(&doc, path, 1, false);
		images.wait();
		let mut snapshot =
			engine.layout_with_images(&doc, &args.options, &images.snapshot);
		for info in images.snapshot.entries.values() {
			if let Some(error) = &info.error {
				eprintln!("Image: {error}");
			}
		}
		let target = renderer.offscreen(args.width, args.height);
		let horizontal = HashMap::new();
		let view = View {
			selection: None,
			hovered_link: None,
			held_overflow: None,
			hovered_overflow: None,
			scroll: args.scroll,
			horizontal: &horizontal,
			revision: 0,
			width: args.width,
			height: args.height,
			scale: args.scale,
			left: ((args.width as f32 / args.scale - args.options.width) / 2.0)
				.max(16.0),
			top: 24.0,
			bottom: 24.0,
			theme: args.theme.unwrap_or_default(),
		};
		let index = renderer.render(
			&snapshot,
			&view,
			&[],
			&target.create_view(&Default::default()),
		)?;
		renderer.wait(Some(index))?;
		// The first frame publishes the actual physical sizes, including DPI
		// and explicit HTML dimensions. Export the settled vector raster.
		images.wait();
		if snapshot.images.entries != images.snapshot.entries {
			snapshot = engine.layout_with_images(
				&doc,
				&args.options,
				&images.snapshot,
			);
			let index = renderer.render(
				&snapshot,
				&view,
				&[],
				&target.create_view(&Default::default()),
			)?;
			renderer.wait(Some(index))?;
		}
		let output = args.output.as_ref().unwrap();
		if let Some(parent) =
			output.parent().filter(|p| !p.as_os_str().is_empty())
		{
			std::fs::create_dir_all(parent)?;
		}
		renderer.save_png(&target, output)?;
		eprintln!(
			"Rendered {} blocks, {:.0}px tall, {} degraded paragraphs, {} formula errors; {}",
			snapshot.blocks.len(),
			snapshot.height,
			snapshot.degraded,
			snapshot.math_errors,
			renderer.adapter_name
		);
		return Ok(());
	}
	let event_loop = EventLoop::<Event>::with_user_event().build()?;
	event_loop.set_control_flow(ControlFlow::Wait);
	let mut app = App::new(args, event_loop.create_proxy());
	event_loop.run_app(&mut app)?;
	app.flush_settings();
	if let Some(warning) = &app.preferences.settings_warning {
		eprintln!("{warning}");
	}
	if let Some(error) = app.fatal {
		bail!("{error}");
	}
	Ok(())
}
