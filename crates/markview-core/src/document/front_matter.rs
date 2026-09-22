//! YAML front matter: a flat mapping becomes a table, the rest stays source.
//!
//! Only the shape is decided here, never the values: a mapping whose every
//! value is a scalar, or a flat sequence of scalars, reads as two columns,
//! while anything deeper keeps its YAML — the honest rendering of a shape a
//! two-column table cannot carry. A key is bold, so a theme can reach the
//! column through `strong`.

use super::{Inline, InlineKind, RichText, TextStyle};
use std::ops::Range;
use yaml_rust2::{Yaml, YamlLoader};

/// The language a front-matter source shape is drawn and highlighted as.
pub(crate) const LANGUAGE: &str = "yaml";

/// What a front-matter block holds.
pub(super) enum Content {
	/// No entries, so there is nothing to draw.
	Empty,
	/// A key cell and a value cell per entry, with the YAML they came from.
	Table(Vec<Vec<RichText>>, String),
	/// The YAML nests, so it is drawn as `yaml` source instead.
	Source(String),
}

/// Read the payload comrak hands over, delimiters included.
pub(super) fn parse(source: &str, range: &Range<usize>) -> Content {
	let yaml = strip(source);
	if yaml.trim().is_empty() {
		return Content::Empty;
	}
	let Ok(documents) = YamlLoader::load_from_str(yaml) else {
		return Content::Source(yaml.to_owned());
	};
	let Some(Yaml::Hash(entries)) = documents.first() else {
		return Content::Source(yaml.to_owned());
	};
	// A key or a value that will not fit a cell sends the whole block to
	// source: half a table beside half a listing reads worse than either.
	let mut rows = Vec::with_capacity(entries.len());
	for (key, value) in entries {
		let (Some(key), Some(value)) = (scalar(key), value_of(value)) else {
			return Content::Source(yaml.to_owned());
		};
		rows.push(vec![cell(key, true, range), cell(value, false, range)]);
	}
	if rows.is_empty() {
		Content::Empty
	} else {
		Content::Table(rows, yaml.to_owned())
	}
}

/// The YAML between the delimiters. Comrak keeps both delimiter lines and
/// closes on the first `---` line, so the body is what precedes it. A line
/// that merely opens with `---` is content — `---extra` closes nothing — so
/// the delimiter is matched the way comrak matched it, including a closing
/// `---` that ends the file with no line ending after it.
fn strip(source: &str) -> &str {
	let body = source.split_once('\n').map_or("", |(_, rest)| rest);
	body.match_indices("\n---")
		.find(|(index, _)| {
			// `None` is the end of the file, where comrak also closes.
			matches!(body.as_bytes().get(index + 4), None | Some(b'\n' | b'\r'))
		})
		.map_or("", |(index, _)| &body[..index + 1])
}

/// A scalar as text, or `None` for anything a cell cannot hold. A literal
/// block scalar keeps its newlines, which a table row would swallow.
fn scalar(value: &Yaml) -> Option<String> {
	match value {
		Yaml::String(text) => (!text.contains('\n')).then(|| text.clone()),
		Yaml::Integer(number) => Some(number.to_string()),
		Yaml::Real(number) => Some(number.clone()),
		Yaml::Boolean(flag) => Some(flag.to_string()),
		Yaml::Null => Some(String::new()),
		_ => None,
	}
}

/// A tabulated value: a scalar, or a flat sequence of scalars joined.
fn value_of(value: &Yaml) -> Option<String> {
	let Yaml::Array(items) = value else {
		return scalar(value);
	};
	let mut out = String::new();
	for item in items {
		if !out.is_empty() {
			out.push_str(", ");
		}
		out.push_str(&scalar(item)?);
	}
	Some(out)
}

/// One cell: a single run, bold for a key, spanning the block's own source.
fn cell(text: String, bold: bool, source: &Range<usize>) -> RichText {
	vec![Inline {
		kind: InlineKind::Text(text),
		style: TextStyle {
			bold,
			..TextStyle::default()
		},
		source: source.clone(),
	}]
}
