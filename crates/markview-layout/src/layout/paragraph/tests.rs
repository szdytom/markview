use super::*;
use crate::{
	document::{LineBreak, TextStyle},
	fonts::FontConfig,
	layout::LayoutEngine,
};
use proptest::prelude::*;

fn inline(kind: InlineKind) -> Inline {
	Inline {
		kind,
		link: None,
		style: TextStyle::default(),
	}
}

fn options() -> LayoutOptions {
	LayoutOptions {
		width: 160.,
		paragraph_indent: 1.,
		fonts: FontConfig {
			ignore_system_fonts: true,
			directories: vec![
				std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
					.join("tests/fonts"),
			],
			..Default::default()
		},
		..Default::default()
	}
}

fn with_context<T>(
	opts: &LayoutOptions,
	run: impl FnOnce(&mut BlockContext<'_>) -> T,
) -> T {
	let mut engine = LayoutEngine::new();
	engine.shaper.set_fonts(&opts.fonts);
	engine.shaper.set_stylesheet(opts.stylesheet.clone());
	let images = Default::default();
	let mut context = BlockContext {
		nested: None,
		cancelled: &|| false,
		fields: Default::default(),
		shaper: &mut engine.shaper,
		math: &mut engine.math,
		images: &images,
		highlight_cache: engine.highlights.results(),
		marker_depth: 0,
		enum_depth: 0,
		footnote_column: 1,
	};
	run(&mut context)
}

fn layout(
	rich: &[Inline],
	opts: &LayoutOptions,
	max_lines: Option<usize>,
) -> (f32, BlockLayout) {
	with_context(opts, |context| {
		let mut out = BlockLayout::default();
		let height = context.paragraph_bounded(
			rich,
			11.,
			13.,
			opts.width,
			18.,
			false,
			CellAlign::Left,
			opts.justify,
			true,
			max_lines,
			opts,
			&mut out,
		);
		(height, out)
	})
}

#[test]
fn leading_empty_line_keeps_the_indent_and_anchors_follow_their_line() {
	let rich = [
		inline(InlineKind::Anchor("empty".into())),
		inline(InlineKind::LineBreak(LineBreak::Hard)),
		inline(InlineKind::Anchor("text".into())),
		inline(InlineKind::Text(
			"Opening words wrap across several lines in this narrow paragraph."
				.into(),
		)),
		inline(InlineKind::Anchor("end".into())),
	];
	for greedy in [false, true] {
		let opts = LayoutOptions {
			greedy,
			..options()
		};
		let (_, out) = layout(&rich, &opts, None);
		let clusters = &out.text[0].clusters;
		let first = clusters.at(0).rect;
		assert_eq!(first.x, 11. + opts.indent(18., opts.width));
		assert!(first.y > 13.);
		let second = clusters
			.iter()
			.find(|c| c.rect.y > first.y + 1.)
			.unwrap()
			.rect;
		assert_eq!(second.x, 11.);
		assert_eq!(out.anchors[0].y, 13.);
		assert_eq!(out.anchors[1].y, first.y);
		assert_eq!(out.anchors[2].y, clusters.iter().last().unwrap().rect.y);
	}
}

#[test]
fn bounded_elision_counts_empty_lines_and_preserves_source_endpoints() {
	let text = "Cannot open image: /a/very/long/中文/directory/missing.png";
	for greedy in [false, true] {
		for leading_empty in [false, true] {
			let mut rich = Vec::new();
			if leading_empty {
				rich.push(inline(InlineKind::LineBreak(LineBreak::Hard)));
			}
			rich.push(inline(InlineKind::Text(text.into())));
			rich.push(inline(InlineKind::Anchor("end".into())));
			let opts = LayoutOptions {
				greedy,
				..options()
			};
			let (full_height, _) = layout(&rich, &opts, None);
			let (height, out) =
				layout(&rich, &opts, Some(1 + usize::from(leading_empty)));
			assert!(height < full_height);
			let node = &out.text[0];
			let first = node.clusters.at(0);
			assert!(node.clusters.iter().all(|c| c.rect.y == first.rect.y));
			assert_eq!(first.range.start, usize::from(leading_empty));
			assert_eq!(
				node.clusters.iter().last().unwrap().range.end,
				node.text.len()
			);
			assert_eq!(out.anchors[0].y, first.rect.y);
			assert!(node.clusters.iter().all(|c| c.command < out.draws.len()));
		}
	}
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(24))]
	#[test]
	fn an_unrestrictive_bound_preserves_paragraph_geometry(
		words in prop::collection::vec(prop::sample::select(vec!["office", "extraordinary", "中文", "（排版），", "link", " "]), 1..30),
		width in 60u16..400,
		greedy in any::<bool>(),
		justify in any::<bool>(),
		indent in 0u8..3,
	) {
		let rich = [inline(InlineKind::Text(words.join(" ")))];
		let opts = LayoutOptions { width: f32::from(width), greedy, justify, paragraph_indent: f32::from(indent), ..options() };
		let (height, full) = layout(&rich, &opts, None);
		let (bounded_height, bounded) = layout(&rich, &opts, Some(1000));
		prop_assert_eq!(height, bounded_height);
		prop_assert_eq!(format!("{full:?}"), format!("{bounded:?}"));
	}
}

#[test]
fn boundary_reshaping_replaces_the_tail_using_the_full_measure() {
	for greedy in [false, true] {
		let opts = LayoutOptions {
			greedy,
			..options()
		};
		with_context(&opts, |context| {
			let rich = [inline(InlineKind::Text("office words need several lines after reshaping their boundaries and inserting hyphens".into()))];
			let p = context.prepare(&rich, 18., &mut BlockLayout::default());
			let units = context.units(
				&p,
				18.,
				false,
				true,
				opts.width,
				opts.typography(),
			);
			let paragraph = ParagraphLayout {
				prepared: &p,
				size: 18.,
				sans: false,
				width: opts.width,
				justify: true,
				opts: &opts,
			};
			// Model a break search that underestimated the boundary-shaped advances.
			let mut line = linebreak::Line {
				units: 0..units.len(),
				width: 0.,
				ratio: 0.,
				hyphen: false,
				last: true,
			};
			let mut tail = VecDeque::new();
			let first_width = opts.width - 54.;
			let mut eager_line = line.clone();
			let mut eager_tail = tail.clone();
			let (eager, corrections) = eager_reshape_to_fit(
				&paragraph,
				context,
				&units,
				&mut eager_line,
				&mut eager_tail,
				first_width,
			);
			assert!(corrections > 1);
			let shaped = paragraph.reshape_to_fit(
				context,
				&units,
				&mut line,
				&mut tail,
				first_width,
			);
			assert_eq!(
				format!("{line:?} {tail:?}"),
				format!("{eager_line:?} {eager_tail:?}")
			);
			assert_same_shaping(&shaped, &eager);
			assert!(line.units.end < units.len());
			assert!(!line.last);
			let shrink: f32 = shaped.fits.iter().map(|f| f.shrink()).sum();
			assert!(
				shaped.natural - shrink <= first_width + shaped.overhang + 0.1
			);
			assert!(!tail.is_empty());
			// A discarded break space belongs between the current line and the tail.
			let end = line.units.end;
			let consumed = end + usize::from(units[end].discard);
			let expected = if greedy {
				linebreak::greedy(&units[consumed..], opts.width, &opts.limits)
			} else {
				linebreak::break_lines(
					&units[consumed..],
					opts.width,
					18.,
					true,
					&opts.limits,
				)
			};
			for (actual, expected) in tail.iter().zip(&expected.lines) {
				assert_eq!(
					actual.units,
					expected.units.start + consumed
						..expected.units.end + consumed
				);
				assert_eq!(actual.hyphen, expected.hyphen);
				assert_eq!(actual.last, expected.last);
			}
			assert_eq!(tail.len(), expected.lines.len());
		});
	}
}

#[test]
fn an_empty_correction_keeps_the_last_successful_tail() {
	for greedy in [false, true] {
		let opts = LayoutOptions {
			greedy,
			justify: false,
			..options()
		};
		for (text, corrected) in [("  words", false), ("  office words", true)]
		{
			with_context(&opts, |context| {
				let rich = [inline(InlineKind::Text(text.into()))];
				let p =
					context.prepare(&rich, 18., &mut BlockLayout::default());
				let units = context.units(
					&p,
					18.,
					false,
					false,
					opts.width,
					opts.typography(),
				);
				let paragraph = ParagraphLayout {
					prepared: &p,
					size: 18.,
					sans: false,
					width: opts.width,
					justify: false,
					opts: &opts,
				};
				let mut line = linebreak::Line {
					units: 0..units.len(),
					width: 0.,
					ratio: 0.,
					hyphen: false,
					last: true,
				};
				let mut eager_line = line.clone();
				let mut tail = VecDeque::from([line.clone()]);
				let mut eager_tail = tail.clone();
				let (expected, corrections) = eager_reshape_to_fit(
					&paragraph,
					context,
					&units,
					&mut eager_line,
					&mut eager_tail,
					-1.,
				);
				assert_eq!(corrections > 0, corrected);
				assert!(eager_line.units.is_empty());
				let actual = paragraph
					.reshape_to_fit(context, &units, &mut line, &mut tail, -1.);
				assert_eq!(
					format!("{line:?} {tail:?}"),
					format!("{eager_line:?} {eager_tail:?}")
				);
				assert_same_shaping(&actual, &expected);
			});
		}
	}
}

fn assert_same_shaping(actual: &ShapedLine, expected: &ShapedLine) {
	assert_eq!(actual.natural.to_bits(), expected.natural.to_bits());
	assert_eq!(actual.overhang.to_bits(), expected.overhang.to_bits());
	assert_eq!(actual.fits, expected.fits);
	assert_eq!(actual.clusters.len(), expected.clusters.len());
	for (a, b) in actual.clusters.iter().zip(&expected.clusters) {
		assert_eq!((&a.range, a.rtl, a.mixed), (&b.range, b.rtl, b.mixed));
		let metrics = |c: &Cluster| {
			[
				c.width,
				c.ascent,
				c.descent,
				c.background.0,
				c.background.1,
				c.baseline_shift,
			]
			.map(f32::to_bits)
		};
		assert_eq!(metrics(a), metrics(b));
		assert_eq!(a.glyphs.len(), b.glyphs.len());
		for (a, b) in a.glyphs.iter().zip(&b.glyphs) {
			assert_eq!(a.id, b.id);
			assert_eq!(
				[a.x, a.y, a.size].map(f32::to_bits),
				[b.x, b.y, b.size].map(f32::to_bits)
			);
			assert_eq!(a.font.index, b.font.index);
			assert_eq!(a.font.data.data(), b.font.data.data());
			assert_eq!(a.coords, b.coords);
			assert_eq!(a.synthetic_italic, b.synthetic_italic);
			assert_eq!(a.synthetic_bold, b.synthetic_bold);
			assert_eq!(a.paint, b.paint);
		}
	}
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(48))]
	#[test]
	fn deferred_tail_matches_eager_boundary_corrections(
		words in prop::collection::vec(prop::sample::select(vec!["office", "extraordinary", "中文", "（排版），", "soft\u{ad}hyphen", "\u{202e}ab cd\u{202c}", " ", "\n"]), 1..24),
		width in 20u16..400,
		greedy in any::<bool>(),
		justify in any::<bool>(),
		budget in prop_oneof![Just(0usize), Just(1), Just(10000)],
	) {
		let opts = LayoutOptions { width: f32::from(width), greedy, justify, limits: crate::limits::Limits { linebreak_evaluations: budget, ..Default::default() }, ..options() };
		with_context(&opts, |context| {
			let rich = [inline(InlineKind::Text(words.join(" ")))];
			let p = context.prepare(&rich, 18., &mut BlockLayout::default());
			let units = context.units(&p, 18., false, true, opts.width, opts.typography());
			let paragraph = ParagraphLayout { prepared: &p, size: 18., sans: false, width: opts.width, justify, opts: &opts };
			let mut line = linebreak::Line { units: 0..units.len(), width: 0., ratio: 0., hyphen: false, last: true };
			let mut eager_line = line.clone();
			// A sentinel detects accidental tail replacement when no correction succeeds.
			let mut tail = VecDeque::from([line.clone()]);
			let mut eager_tail = tail.clone();
			let first_width = opts.width * 0.75;
			let (expected, _) = eager_reshape_to_fit(&paragraph, context, &units, &mut eager_line, &mut eager_tail, first_width);
			let actual = paragraph.reshape_to_fit(context, &units, &mut line, &mut tail, first_width);
			assert_eq!(format!("{line:?} {tail:?}"), format!("{eager_line:?} {eager_tail:?}"));
			assert_same_shaping(&actual, &expected);
		});
	}
}

// Reference policy: every successful boundary correction solves the tail.
fn eager_reshape_to_fit(
	paragraph: &ParagraphLayout<'_>,
	context: &mut BlockContext<'_>,
	units: &[linebreak::Unit],
	line: &mut linebreak::Line,
	lines: &mut VecDeque<linebreak::Line>,
	line_width: f32,
) -> (ShapedLine, usize) {
	let mut corrections = 0;
	let mut shaped = paragraph.shape(context, units, line);
	loop {
		let target = line_width + shaped.overhang;
		let shrink: f32 =
			if shaped.natural > target || (paragraph.justify && !line.last) {
				shaped.fits.iter().map(|f| f.shrink()).sum()
			} else {
				0.
			};
		if shaped.natural - shrink <= target + 0.1 {
			break;
		}
		let Some(end) = (line.units.start + 1..line.units.end)
			.rev()
			.find(|&end| units[end - 1].after.is_some())
		else {
			break;
		};
		let br = units[end - 1].after.unwrap();
		line.units.end = end;
		while line.units.end > line.units.start
			&& units[line.units.end - 1].discard
		{
			line.units.end -= 1;
		}
		if line.units.is_empty() {
			break;
		}
		line.hyphen = br.hyphen_width > 0.;
		line.last = br.forced;
		shaped = paragraph.shape(context, units, line);
		corrections += 1;
		let tail = if paragraph.opts.greedy {
			linebreak::greedy(
				&units[end..],
				paragraph.width,
				&paragraph.opts.limits,
			)
		} else {
			linebreak::break_lines(
				&units[end..],
				paragraph.width,
				paragraph.size,
				paragraph.justify,
				&paragraph.opts.limits,
			)
		};
		*lines = tail
			.lines
			.into_iter()
			.map(|mut l| {
				l.units.start += end;
				l.units.end += end;
				l
			})
			.collect();
	}
	(shaped, corrections)
}
