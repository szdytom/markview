use super::*;
use crate::html::BlockEvent;

/// Records what the stack asks the parser to render; the tests assert on
/// stack state, so leaves only need to be counted.
#[derive(Default)]
struct Host {
	opaque: Vec<Range<usize>>,
	ordinal: u32,
	leaves: Vec<String>,
}

impl ContainerHost for Host {
	fn leaf(
		&mut self,
		text: &str,
		_range: Range<usize>,
		_opaque_until: usize,
	) -> Option<Block> {
		self.leaves.push(text.to_owned());
		None
	}

	fn summary(&mut self, _text: &str, _source: &Range<usize>) -> RichText {
		RichText::new()
	}

	fn opaque_until(&self, at: usize) -> usize {
		self.opaque
			.iter()
			.find(|range| range.start <= at && at < range.end)
			.map_or(0, |range| range.end)
	}

	fn next_details_ordinal(&mut self) -> u32 {
		let ordinal = self.ordinal;
		self.ordinal += 1;
		ordinal
	}
}

fn open(stack: &mut ContainerStack, name: &str) {
	let raw = format!("<{name}>");
	stack.open(name, false, 0, Vec::new(), raw, 0);
}

fn no_container_events(events: &[BlockEvent]) -> bool {
	!events.iter().any(|event| {
		matches!(event, BlockEvent::Open { .. } | BlockEvent::Close { .. })
	})
}

/// Run the stack over `source` as one raw HTML block, with no container
/// prefixes to map.
fn consume(source: &str) -> (ContainerStack, Host) {
	let mut stack = ContainerStack::default();
	let mut host = Host::default();
	let mut out = Vec::new();
	let events = html::block_events(source);
	stack.consume(
		&events,
		source,
		|range| range,
		0,
		0,
		usize::MAX,
		"",
		&mut host,
		&mut out,
	);
	(stack, host)
}

#[test]
fn an_unclosed_container_stays_open_until_the_walk_ends() {
	let mut stack = ContainerStack::default();
	open(&mut stack, "div");
	assert_eq!(stack.len(), 1);
	assert!(matches!(
		&stack.frames[0].kind,
		FrameKind::Transparent { name } if name == "div"
	));
	assert_eq!(opaque_len("<div>x</div>"), None);
	let mut host = Host::default();
	let mut out = Vec::new();
	stack.pop_unclosed(&mut out, &mut host);
	assert_eq!(stack.len(), 0);
	assert_eq!(host.leaves, ["<div>"]);
}

#[test]
fn an_orphan_close_matches_nothing() {
	let mut stack = ContainerStack::default();
	let mut host = Host::default();
	let mut out = Vec::new();
	assert!(!stack.close("div", 10, "", &mut out, &mut host));
	open(&mut stack, "section");
	assert!(!stack.close("details", 20, "", &mut out, &mut host));
	assert!(!stack.close("p", 20, "", &mut out, &mut host));
	assert_eq!(stack.len(), 1);
	assert!(stack.frames[0].matches("section"));
	assert_eq!(opaque_len("<section>x</section>"), None);
}

#[test]
fn a_close_ends_the_nearest_match_and_everything_inside_it() {
	let mut stack = ContainerStack::default();
	open(&mut stack, "div");
	open(&mut stack, "section");
	open(&mut stack, "p");
	assert_eq!(stack.len(), 3);
	let names: Vec<&str> = stack
		.frames
		.iter()
		.filter_map(|frame| match &frame.kind {
			FrameKind::Transparent { name } => Some(name.as_str()),
			FrameKind::Details { .. } => None,
		})
		.collect();
	assert_eq!(names, ["div", "section", "p"]);
	assert_eq!(opaque_len("<div><section><p>x</p></section></div>"), None);
	let mut host = Host::default();
	let mut out = Vec::new();
	assert!(stack.close("div", 30, "", &mut out, &mut host));
	assert_eq!(stack.len(), 0);
}

#[test]
fn raw_text_elements_are_opaque_to_container_matching() {
	for name in ["script", "style", "textarea"] {
		let source = format!("<{name}></div></{name}>");
		assert_eq!(opaque_len(&source), Some(source.len()), "{name}");
		assert!(no_container_events(&html::block_events(&source)), "{name}");
		assert_eq!(consume(&source).0.len(), 0, "{name}");
	}
}

#[test]
fn pre_and_foreign_content_stay_opaque() {
	for source in ["<pre><div>x</div></pre>", "<svg><div></div></svg>"] {
		assert_eq!(opaque_len(source), Some(source.len()), "{source}");
		assert!(no_container_events(&html::block_events(source)), "{source}");
	}
}

#[test]
fn grouping_tags_are_transparent_containers() {
	let mut stack = ContainerStack::default();
	for name in ["div", "section", "p"] {
		open(&mut stack, name);
		let frame = stack.frames.last().expect("an open frame");
		assert!(matches!(
			&frame.kind,
			FrameKind::Transparent { name: open } if open == name
		));
		assert!(frame.matches(name));
		assert!(!frame.matches("details"));
		assert_eq!(opaque_len(&format!("<{name}>x</{name}>")), None);
	}
	assert_eq!(stack.len(), 3);
	let mut host = Host::default();
	let mut out = Vec::new();
	assert!(stack.close("div", 40, "", &mut out, &mut host));
	assert_eq!(stack.len(), 0);
}
