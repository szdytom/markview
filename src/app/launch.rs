//! Window and deterministic diagnostic entry points.
use crate::cli::{Mode, arguments};
use crate::{
	benchmark, document,
	file::read_document,
	layout::LayoutEngine,
	render::{Renderer, Theme, View},
};
use anyhow::{Result, bail};
use log::{info, warn};
use markview_core::style::CjkType;
use std::collections::HashMap;
use winit::event_loop::{ControlFlow, EventLoop};

use super::{App, Event};

pub(super) fn run() -> Result<()> {
	// Only a run with a command line reports on the console it came from. The
	// reader window keeps no console, so closing a shell cannot end it.
	if std::env::args_os().len() > 1 {
		crate::platform::attach_parent_console();
	}
	let Some(mut args) = arguments()? else {
		return Ok(());
	};
	crate::logging::init(&args.mode);
	if let Some((source, force)) = &args.install {
		let dir = crate::stylesheet::directory()
			.ok_or_else(|| anyhow::anyhow!("No user stylesheet directory"))?;
		let (id, path) = crate::stylesheet::install(source, &dir, *force)?;
		crate::logging::report(format_args!(
			"Installed {id}: {}",
			path.display()
		));
		return Ok(());
	}
	let ids = args.style.clone().or_else(|| {
		args.theme.map(|t| {
			vec![if t == Theme::Dark { "dark" } else { "light" }.into()]
		})
	});
	// The diagnostic entry points lay out from the command line, so they have to
	// pick the CJK variant up themselves; otherwise CJK text is drawn in a
	// system fallback face rather than the one the reader configured.
	// This path lays out from the command line rather than from a running
	// window, so it takes the CJK variant from the flag and defaults to the
	// mainland convention. A variant always has to be selected: without one the
	// stylesheet has no `[cjk]` font definition at all, and every CJK cluster
	// would be drawn in whatever face the system happens to offer.
	let cjk_type = args.cjk_type.unwrap_or(CjkType::Sc);
	// A rendered page cannot be scrolled sideways, so image exports wrap code
	// blocks by default; the interactive reader keeps its saved preference.
	if args.mode.exports_image() {
		args.options.codeblock_wrap = true;
	}
	args.options.stylesheet = crate::stylesheet::load_for_run(
		ids.as_deref(),
		crate::stylesheet::directory().as_deref(),
		cjk_type,
	)?;
	if args.mode == Mode::Render
		|| args.mode == Mode::Bench
		|| args.mode == Mode::Latency
	{
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
		if args.mode == Mode::Latency {
			return crate::latency::run(
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
		for entry in images.snapshot.entries.values() {
			if let Some(error) = &entry.error {
				warn!("Image: {error}");
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
		info!(
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
		warn!("{warning}");
	}
	if let Some(error) = app.fatal {
		bail!("{error}");
	}
	Ok(())
}
