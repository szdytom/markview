use anyhow::{Result, bail};
use dom_smoothie::{Config, Readability, TextMode, dom_query};

pub struct Extracted {
	pub markdown: String,
	pub title: String,
}

pub fn extract(html: &str, url: &str) -> Result<Extracted> {
	let config = Config {
		text_mode: TextMode::Markdown,
		max_elements_to_parse: 100_000,
		..Default::default()
	};
	let document = dom_query::Document::from(html);
	let preserved = preserve_markup(&document);
	let article =
		Readability::with_document(document, Some(url), Some(config))?
			.parse()?;
	if article.text_content.trim().is_empty() {
		bail!("No readable article found on this page");
	}
	// A single plain heading names the temporary tab; source links stay absolute.
	let title = article
		.title
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	let title = if title.is_empty() { url } else { &title };
	let mut heading = String::with_capacity(title.len());
	for c in title.chars() {
		if c.is_ascii_punctuation() {
			use std::fmt::Write;
			write!(heading, "&#{};", c as u32).unwrap();
		} else {
			heading.push(c);
		}
	}
	let mut content = literal_delimiters(&article.text_content);
	for (token, replacement) in preserved {
		content = content.replace(&token, &replacement);
	}
	let markdown = format!("# {heading}\n\nSource: <{url}>\n\n{}\n", content);
	Ok(Extracted {
		markdown,
		title: title.to_owned(),
	})
}

fn preserve_markup(document: &dom_query::Document) -> Vec<(String, String)> {
	let text = document.root().text();
	let mut prefix = "MARKVIEWPRESERVED".to_string();
	while text.contains(&prefix) {
		prefix.push('X');
	}
	let mut preserved = Vec::new();
	for node in document
		.select(".math.inline, .math.display, script[type^='math/tex']")
		.nodes()
	{
		if node.ancestors_it(None).any(|parent| parent.is("pre, code")) {
			continue;
		}
		let text = node.text();
		let mut latex = text.trim();
		for (open, close) in
			[(r"\(", r"\)"), (r"\[", r"\]"), ("$$", "$$"), ("$", "$")]
		{
			if let Some(inner) =
				latex.strip_prefix(open).and_then(|s| s.strip_suffix(close))
			{
				latex = inner;
				break;
			}
		}
		let in_table =
			node.ancestors_it(None).any(|parent| parent.is("td, th"));
		// TeX delimiters keep literal pipes from splitting Markdown table cells.
		let latex = if in_table {
			latex.replace(r"\|", r"\Vert ").replace('|', r"\vert ")
		} else {
			latex.to_string()
		};
		let display = node.has_class("display")
			|| node
				.attr("type")
				.is_some_and(|kind| kind.contains("mode=display"));
		let replacement = if display && !in_table {
			format!("\n\n\\[{latex}\\]\n\n")
		} else {
			format!("\\({latex}\\)")
		};
		let token = format!("{prefix}{}END", preserved.len());
		if node.is("script") {
			node.rename("span");
		}
		node.set_text(token.as_str());
		preserved.push((token, replacement));
	}
	// Empty headings otherwise produce an empty, invalid table separator cell.
	for node in document.select("th").nodes() {
		if node.inner_html().trim().is_empty() {
			let token = format!("{prefix}{}END", preserved.len());
			node.set_text(token.as_str());
			preserved.push((token, String::new()));
		}
	}
	preserved
}

fn literal_delimiters(markdown: &str) -> String {
	let arena = comrak::Arena::new();
	let root =
		comrak::parse_document(&arena, markdown, &comrak::Options::default());
	let mut lines = vec![0];
	lines.extend(markdown.match_indices('\n').map(|(offset, _)| offset + 1));
	let mut code = root
		.descendants()
		.filter_map(|node| {
			let data = node.data.borrow();
			matches!(
				data.value,
				comrak::nodes::NodeValue::Code(_)
					| comrak::nodes::NodeValue::CodeBlock(_)
			)
			.then(|| {
				let position = data.sourcepos;
				let start =
					lines[position.start.line - 1] + position.start.column - 1;
				let end = (lines[position.end.line - 1] + position.end.column)
					.min(markdown.len());
				start..end
			})
		})
		.peekable();
	let mut output = String::with_capacity(markdown.len());
	let mut offset = 0;
	while offset < markdown.len() {
		if let Some(range) = code.peek()
			&& offset == range.start
		{
			output.push_str(&markdown[range.clone()]);
			offset = range.end;
			code.next();
			continue;
		}
		let tail = &markdown[offset..];
		let mut chars = tail.chars();
		let c = chars.next().unwrap();
		if c == '\\'
			&& let Some(next) = chars.next()
		{
			match next {
				'(' => output.push_str("&#40;"),
				')' => output.push_str("&#41;"),
				'[' => output.push_str("&#91;"),
				']' => output.push_str("&#93;"),
				_ => {
					output.push(c);
					output.push(next);
				}
			}
			offset += c.len_utf8() + next.len_utf8();
		} else {
			output.push(c);
			offset += c.len_utf8();
		}
	}
	output
}
