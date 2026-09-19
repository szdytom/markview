//! Compile-time SVG icons for the reader's chrome.
//!
//! [`icon!`] reads an SVG file next to the calling crate's manifest, parses it
//! at compile time, and expands to a `&'static [markview_core::scene::IconPath]`
//! normalized to a unit box. The reader keeps its icons as editable SVG files
//! without parsing, allocating or retaining any SVG runtime.
//!
//! The emitted constant also embeds the file through `include_bytes!`, so
//! editing an icon rebuilds the crate that uses it.
use proc_macro::TokenStream;
use quote::quote;

mod svg;

/// Expands to the vector figures of an SVG file, in a unit box.
///
/// The path is relative to the manifest directory of the crate that invokes
/// the macro. Only geometry is extracted: colors come from the theme, stroked
/// figures are drawn round-capped, and a square viewport is required.
#[proc_macro]
pub fn icon(input: TokenStream) -> TokenStream {
	let path = syn::parse_macro_input!(input as syn::LitStr);
	let relative = path.value();
	let root = std::env::var("CARGO_MANIFEST_DIR")
		.expect("cargo sets CARGO_MANIFEST_DIR for the crate being compiled");
	let source = match std::fs::read_to_string(
		std::path::Path::new(&root).join(&relative),
	) {
		Ok(source) => source,
		Err(error) => {
			return fail(&path, &format!("cannot read {relative}: {error}"));
		}
	};
	let figures = match svg::parse(&source) {
		Ok(figures) => figures,
		Err(error) => return fail(&path, &format!("{relative}: {error}")),
	};
	let figures = figures.iter().map(|figure| {
		let commands = figure.commands.iter().map(|command| match command {
			svg::Command::MoveTo { x, y } => {
				quote!(::markview_core::scene::PathCommand::MoveTo { x: #x, y: #y })
			}
			svg::Command::LineTo { x, y } => {
				quote!(::markview_core::scene::PathCommand::LineTo { x: #x, y: #y })
			}
			svg::Command::QuadTo { x1, y1, x, y } => quote!(
				::markview_core::scene::PathCommand::QuadTo {
					x1: #x1, y1: #y1, x: #x, y: #y,
				}
			),
			svg::Command::CubicTo {
				x1,
				y1,
				x2,
				y2,
				x,
				y,
			} => quote!(
				::markview_core::scene::PathCommand::CubicTo {
					x1: #x1, y1: #y1, x2: #x2, y2: #y2, x: #x, y: #y,
				}
			),
			svg::Command::Close => {
				quote!(::markview_core::scene::PathCommand::Close)
			}
		});
		let (fill, stroke_width) = (figure.fill, figure.stroke_width);
		quote!(
			::markview_core::scene::IconPath {
				commands: &[#(#commands),*],
				fill: #fill,
				stroke_width: #stroke_width,
			}
		)
	});
	quote!({
		const _: &[u8] =
			include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/", #relative));
		&[#(#figures),*]
	})
	.into()
}

fn fail(path: &syn::LitStr, message: &str) -> TokenStream {
	syn::Error::new(path.span(), message)
		.to_compile_error()
		.into()
}
