use dom_smoothie::dom_query::{Document, NodeRef};
use std::rc::Rc;

/// Resolves language inheritance before Readability removes wrappers and classes.
pub(super) fn preserve(document: &Document) {
	let mut pending = vec![(document.root(), None::<Rc<str>>)];
	while let Some((node, inherited)) = pending.pop() {
		let language = declared(&node).map(Rc::from).or(inherited);
		if node.node_name().is_some_and(|name| {
			name.eq_ignore_ascii_case("pre")
				|| name.eq_ignore_ascii_case("code")
		}) && let Some(language) = &language
		{
			node.set_attr("data-mv-language", language);
		}
		pending.extend(
			node.children_it(false)
				.filter(NodeRef::is_element)
				.map(|child| (child, language.clone())),
		);
	}
}

fn declared(node: &NodeRef<'_>) -> Option<String> {
	let classes = node.attr("class").unwrap_or_default();
	if classes.split_ascii_whitespace().any(|class| {
		class.eq_ignore_ascii_case("nohighlight")
			|| class.eq_ignore_ascii_case("no-highlight")
	}) {
		return Some("text".into());
	}
	for attribute in ["data-language", "data-lang"] {
		if let Some(value) = node.attr(attribute)
			&& let Some(language) = normalize(&value)
		{
			return Some(language);
		}
	}
	classes
		.split_ascii_whitespace()
		.find_map(|class| {
			class
				.strip_prefix("language-")
				.or_else(|| class.strip_prefix("lang-"))
				.or_else(|| class.strip_prefix("highlight-source-"))
				.and_then(normalize)
		})
		.or_else(|| {
			// Legacy highlight.js bare names are only meaningful on code blocks.
			(node.has_name("pre") || node.has_name("code"))
				.then(|| {
					classes.split_ascii_whitespace().find_map(|class| {
						match class {
							"javascript" | "typescript" | "python" | "ruby"
							| "rust" | "bash" | "sh" | "java" | "cpp" | "c"
							| "csharp" | "cs" | "css" | "html" | "xml"
							| "json" | "yaml" | "sql" | "go" | "js" | "ts"
							| "py" | "rb" | "plaintext" => normalize(class),
							_ => None,
						}
					})
				})
				.flatten()
		})
}

fn normalize(value: &str) -> Option<String> {
	let value = value.trim().to_ascii_lowercase();
	if value.is_empty() {
		return None;
	}
	Some(
		match value.as_str() {
			"none" | "plain" | "plaintext" | "txt" => "text",
			"javascript" => "js",
			"typescript" => "ts",
			"python" | "python3" => "py",
			"ruby" => "rb",
			"shell" | "bash" | "zsh" => "sh",
			"c++" | "c--" => "cpp",
			"csharp" | "c#" => "cs",
			"markup" => "html",
			"yml" => "yaml",
			_ => &value,
		}
		.to_owned(),
	)
}

/// Only interpreter declarations qualify as content-based language hints.
pub(super) fn shebang(text: &str) -> Option<&'static str> {
	let mut words = text
		.lines()
		.next()?
		.strip_prefix("#!")?
		.split_ascii_whitespace();
	let executable = words.next()?.rsplit('/').next()?;
	let interpreter = if executable == "env" {
		match words.next()? {
			"-S" => words.next()?,
			word => word,
		}
	} else {
		executable
	};
	match interpreter {
		"python" | "python2" | "python3" => Some("py"),
		"sh" | "bash" | "zsh" | "dash" => Some("sh"),
		"ruby" => Some("rb"),
		"node" | "nodejs" => Some("js"),
		"perl" => Some("pl"),
		_ => None,
	}
}
