//! Versioned reader-output records, independent of allocation and cache identities.
use markview_layout::{
	scene::*,
	text::{TextCluster, TextNode},
};
use serde_json::{Value, json};

fn rect(r: Rect) -> Value {
	json!([r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()])
}
fn paint(p: Paint) -> Value {
	match p {
		Paint::Color(c) => json!(["color", c.0]),
		Paint::Styled(c, f) => json!(["styled", c as u32, f as u32]),
		Paint::Cascade(c, f) => json!(["cascade", c.to_string(), f as u32]),
		Paint::Scoped(c, s, f) => {
			json!(["scoped", c.to_string(), s as u32, f as u32])
		}
		Paint::Text => json!("text"),
		Paint::Muted => json!("muted"),
		Paint::Accent => json!("accent"),
		Paint::Panel => json!("panel"),
		Paint::Glass => json!("glass"),
		Paint::Scrim => json!("scrim"),
		Paint::Shadow => json!("shadow"),
		Paint::Border => json!("border"),
		Paint::Background => json!("background"),
		Paint::Error => json!("error"),
	}
}
pub fn draw(d: &Draw) -> Value {
	match d {
		Draw::Glyph(g) => {
			json!({"kind":"glyph", "font": font_identity(g), "face":g.font.index, "coords":&*g.coords, "id":g.id, "size":g.size.to_bits(), "x":g.x.to_bits(), "y":g.y.to_bits(), "italic":g.synthetic_italic, "bold":g.synthetic_bold, "paint":paint(g.paint)})
		}
		Draw::Clipped { rect: r, draws } => {
			json!({"kind":"clip","rect":rect(*r),"draws":draws.iter().map(draw).collect::<Vec<_>>()})
		}
		Draw::Image {
			src,
			version,
			rect: r,
			title,
		} => {
			json!({"kind":"image","src":src,"version":version,"rect":rect(*r),"title":title})
		}
		Draw::Rect(r, p) => {
			json!({"kind":"rect","rect":rect(*r),"paint":paint(*p)})
		}
		Draw::FadedRect {
			rect: r,
			paint: p,
			opacity,
		} => {
			json!({"kind":"faded","rect":rect(*r),"paint":paint(*p),"opacity":opacity.to_bits()})
		}
		Draw::Box {
			rect: r,
			chain,
			condition,
			radius,
			border,
			left_only,
			decoration,
		} => {
			json!({"kind":"box","rect":rect(*r),"chain":chain.to_string(),"condition":*condition as u32,"radius":radius.to_bits(),"border":border.to_bits(),"left":left_only,"decoration":decoration.as_ref().map(|d| (d.edges.map(f32::to_bits),d.corners.map(f32::to_bits)))})
		}
		Draw::Math {
			math,
			paint: p,
			x,
			y,
		} => {
			json!({"kind":"math","paint":paint(*p),"x":x.to_bits(),"y":y.to_bits(),"dimensions":[math.width.to_bits(),math.ascent.to_bits(),math.descent.to_bits(),math.size.to_bits()],"display":math.display})
		}
		Draw::Polygon {
			center,
			points,
			paint: p,
		} => {
			json!({"kind":"polygon","center":center.map(f32::to_bits),"points":points.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>(),"paint":paint(*p)})
		}
		Draw::Icon {
			paths,
			paint: p,
			x,
			y,
			size,
		} => {
			json!({"kind":"icon","paths":paths.iter().map(|p|json!([p.commands,p.fill,p.stroke_width.to_bits()])).collect::<Vec<_>>(),"paint":paint(*p),"x":x.to_bits(),"y":y.to_bits(),"size":size.to_bits()})
		}
	}
}
fn target(t: &LinkTarget) -> Value {
	match t {
		LinkTarget::Url(s) => json!(["url", &**s]),
		LinkTarget::FootnoteDefinition(s) => json!(["definition", s.as_str()]),
		LinkTarget::FootnoteReturn(s) => json!(["return", s.as_str()]),
		LinkTarget::Disclosure(id) => json!(["disclosure", id.get()]),
	}
}
fn cluster(c: &TextCluster, draws: &[Draw]) -> Value {
	json!({"range":[c.range.start,c.range.end],"rect":rect(c.rect),"rtl":c.rtl,"atomic":c.atomic,"spacing":[c.mixed_spacing.0.to_bits(),c.mixed_spacing.1.to_bits()],"binding":draws.get(c.command).map(draw)})
}
// Only contiguous, nonempty unit-slope mappings have identical boundary behavior.
fn canonical_ranges(
	ranges: impl Iterator<Item = [usize; 4]>,
) -> Vec<[usize; 4]> {
	let mut result: Vec<[usize; 4]> = Vec::new();
	for r in ranges {
		if let Some(p) = result.last_mut()
			&& p[0] < p[1]
			&& r[0] < r[1]
			&& p[1] - p[0] == p[3] - p[2]
			&& r[1] - r[0] == r[3] - r[2]
			&& p[1] == r[0]
			&& p[3] == r[2]
		{
			p[1] = r[1];
			p[3] = r[3];
		} else {
			result.push(r);
		}
	}
	result
}
fn binding(draws: &[Draw], range: &std::ops::Range<usize>) -> Value {
	match draws.get(range.clone()) {
		Some(draws) => json!(draws.iter().map(draw).collect::<Vec<_>>()),
		None => json!({"invalid_range":[range.start,range.end]}),
	}
}
fn node(n: &TextNode, draws: &[Draw]) -> Value {
	let (images, group, boundaries) = n.reading_metadata();
	json!({"field":n.search_field.map(|f|f.get()),"ranges":canonical_ranges(n.search_ranges.iter().map(|(a,b)|[a.start,a.end,b.start,b.end])),"images":images.iter().map(|(i,c)|json!([i,cluster(c,draws)])).collect::<Vec<_>>(),"group":group,"boundaries":boundaries,"text":n.text.as_str(),"separator":n.separator,"clusters":n.clusters.iter().map(|c|cluster(&c,draws)).collect::<Vec<_>>()})
}
// The exporter also compiles against the old borrowed drawing/link views.
#[allow(clippy::map_clone, clippy::needless_borrow)]
pub fn block(b: &BlockLayout) -> Value {
	let draws: Vec<Draw> = b.draws.iter().map(|d| d.clone()).collect();
	json!({"height":b.height.to_bits(),"width":b.width.to_bits(),"degraded":b.degraded,"math_errors":b.math_errors,
 "constraints":b.page_constraints.iter().map(|p|json!([p.top.to_bits(),p.bottom.to_bits(),p.orphans,p.widows,p.keep_together])).collect::<Vec<_>>(),
 "text":b.text.iter().map(|n|node(n,&draws)).collect::<Vec<_>>(),"draws":draws.iter().map(draw).collect::<Vec<_>>(),
 "overflow":b.overflow.iter().map(|o|json!({"rect":rect(o.rect),"width":o.content_width.to_bits(),"gutter":o.gutter.to_bits(),"binding":binding(&draws,&o.commands)})).collect::<Vec<_>>(),
 "links":b.links.iter().map(|l|json!({"id":match l.id {LinkId::Inline(i)=>json!(["inline",i.get()]),LinkId::Special(i)=>json!(["special",i])},"rect":rect(l.rect),"url":l.url.as_ref(),"target":target(&l.target),"binding":binding(&draws,&l.commands)})).collect::<Vec<_>>(),
 "anchors":b.anchors.iter().map(|a|json!([anchor(&a.anchor),a.y.to_bits()])).collect::<Vec<_>>(),
 "decorations":b.inline_decorations.iter().map(|(i,r)|json!([draws.get(*i).map(draw),r.start.to_bits(),r.end.to_bits()])).collect::<Vec<_>>()})
}
fn anchor(a: &markview_document::AnchorId) -> Value {
	use markview_document::AnchorId::*;
	match a {
		Fragment(s) => json!(["fragment", s]),
		FootnoteDefinition(s) => json!(["definition", s.as_str()]),
		FootnoteReference {
			definition,
			occurrence,
		} => json!(["reference", definition.as_str(), occurrence.get()]),
	}
}
pub fn snapshot(s: &LayoutSnapshot) -> Value {
	let flat = s.flattened();
	json!({"format":1,"width":s.width.to_bits(),"height":s.height.to_bits(),"centered":s.centered,"reading":s.full_reading_text(),"document_box":s.document_box.as_ref().map(draw),"blocks":flat.blocks.iter().map(|b|json!({"id":b.id.get(),"y":b.y.to_bits(),"layout":block(&b.layout)})).collect::<Vec<_>>(),"retained":s.blocks.iter().map(|b|block(&b.layout)).collect::<Vec<_>>()})
}
/// First field difference, with an address usable in failure artifacts.
pub fn difference(a: &Value, b: &Value, path: &str) -> Option<String> {
	if a == b {
		return None;
	}
	match (a, b) {
		(Value::Object(a), Value::Object(b)) if a.keys().eq(b.keys()) => a
			.iter()
			.find_map(|(k, v)| difference(v, &b[k], &format!("{path}.{k}"))),
		(Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
			a.iter().zip(b).enumerate().find_map(|(i, (a, b))| {
				difference(a, b, &format!("{path}[{i}]"))
			})
		}
		_ => Some(format!("{path}: expected {a}, actual {b}")),
	}
}

fn font_identity(g: &Glyph) -> u64 {
	thread_local! { static LAST: std::cell::Cell<Option<(u64,u64)>> = const { std::cell::Cell::new(None) }; }
	LAST.with(|last| {
		if let Some((id, hash)) = last.get()
			&& id == g.font.data.id()
		{
			return hash;
		}
		let hash = markview_document::stable_fingerprint(g.font.data.data());
		last.set(Some((g.font.data.id(), hash)));
		hash
	})
}

fn attributes(kind: &markview_document::BlockKind) -> Value {
	use markview_document::BlockKind;
	let ids = |blocks: &[markview_document::Block]| {
		blocks.iter().map(|b| b.id().get()).collect::<Vec<_>>()
	};
	match kind {
  BlockKind::Paragraph(_)|BlockKind::Rule=>Value::Null,
  BlockKind::Heading{level,anchor,..}=>json!({"level":level,"anchor":anchor}),
  BlockKind::Code{language,..}=>json!({"language":language}),
  BlockKind::Quote{callout,blocks}=>json!({"callout":callout.as_ref().map(|c|json!({"kind":c.kind as u32,"name":c.name,"fold":c.fold,"has_title":c.title.is_some()})),"children":ids(blocks)}),
  BlockKind::List{start,reversed,tight,items}=>json!({"start":start,"reversed":reversed,"tight":tight,"items":items.iter().map(|i|json!({"checked":i.checked,"number":i.number,"children":ids(&i.blocks)})).collect::<Vec<_>>()}),
  BlockKind::Table(t)=>json!({"has_caption":t.caption.is_some(),"columns":t.columns.iter().map(|c|c.align.map(|a|a as u32)).collect::<Vec<_>>(),"groups":t.groups.iter().map(|g|json!({"kind":g.kind as u32,"rows":g.rows.iter().map(|r|r.cells.iter().map(|c|json!({"kind":c.kind as u32,"rowspan":c.rowspan,"colspan":c.colspan,"align":c.align.map(|a|a as u32),"children":ids(&c.blocks)})).collect::<Vec<_>>()).collect::<Vec<_>>()})).collect::<Vec<_>>()}),
  BlockKind::Footnote{id,number,blocks}=>json!({"definition":id.as_str(),"number":number,"children":ids(blocks)}),
  BlockKind::Details{default_open,blocks,..}|BlockKind::FrontMatter{default_open,blocks}=>json!({"default_open":default_open,"children":ids(blocks)}),
  BlockKind::DefinitionList(items)=>json!(items.iter().map(|i|json!({"terms":i.terms.len(),"definitions":i.definitions.iter().map(|b|ids(b)).collect::<Vec<_>>()})).collect::<Vec<_>>()),
  BlockKind::Figure{blocks,caption}=>json!({"children":ids(blocks),"has_caption":caption.is_some()}),
 }
}

/// Semantic identities, paths and source bindings, independent of storage slots.
pub fn document(p: &markview_commonmark::MarkdownParse) -> Value {
	use markview_document::*;
	let mut blocks = Vec::new();
	let mut fields = Vec::new();
	for root in p.document.blocks() {
		visit_blocks(root, &mut |b| {
			let kind = match b.kind() {
				BlockKind::Paragraph(_) => "paragraph",
				BlockKind::Heading { .. } => "heading",
				BlockKind::Code { .. } => "code",
				BlockKind::Quote { .. } => "quote",
				BlockKind::List { .. } => "list",
				BlockKind::Table(_) => "table",
				BlockKind::Footnote { .. } => "footnote",
				BlockKind::Details { .. } => "details",
				BlockKind::Rule => "rule",
				BlockKind::FrontMatter { .. } => "frontmatter",
				BlockKind::DefinitionList(_) => "definitions",
				BlockKind::Figure { .. } => "figure",
			};
			blocks.push(json!({"id":b.id().get(),"path":p.document.path_of(b.id()),"kind":kind,"attributes":attributes(b.kind()),"anchors":b.anchors(),"open":p.document.details_declared(b.id())}));
		});
		visit_fields(
			root,
			&mut Vec::new(),
			&|| false,
			&mut |owner, field, text, enclosing| {
				let content=match text {
    FieldText::Code(s)=>json!({"code":s}),
    FieldText::Rich(r)=>json!(r.iter().map(|i|{
     let kind=match &i.kind {
      InlineKind::Text(s)=>json!(["text",s]),InlineKind::Anchor(s)=>json!(["anchor",s]),
      InlineKind::Math{latex,display}=>json!(["math",latex,display]),
      InlineKind::LineBreak(b)=>json!(["break",*b as u32]),
      InlineKind::FootnoteRef(f)=>json!(["note",f.definition.as_str(),f.number,f.occurrence.get()]),
      InlineKind::Image(img)=>json!(["image",match &img.source {ImageSource::Resource(r)=>json!(["resource",r.address,r.base as u32]),ImageSource::Diagram(d)=>json!(["diagram",d.language,d.source])},img.alt,img.title,img.width,img.height]),
     };
     let s=&i.style;
     json!({"kind":kind,"style":[s.bold,s.italic,s.strike,s.mark,s.code,s.superscript,s.subscript],"color":s.color.map(|c|c.0),"link":i.link.as_ref().map(|l|(l.as_str(),l.occurrence().get()))})
    }).collect::<Vec<_>>()),
   };
				fields.push(json!({"owner":owner.get(),"field":field.get(),"enclosing":enclosing.iter().map(|id|id.get()).collect::<Vec<_>>(),"content":content}));
			},
		);
	}
	fn span(s: &SourceSpan) -> Value {
		json!([
			s.semantic.start,
			s.semantic.end,
			s.source.start,
			s.source.end,
			s.exact
		])
	}
	json!({"blocks":blocks,"fields":fields,"outline":p.document.outline_entries().iter().map(|o|json!([o.block.get(),o.level,o.text,o.anchor])).collect::<Vec<_>>(),"sources":p.map.blocks().iter().map(|b|json!({"block":b.block.get(),"source":b.source.as_ref().map(|r|[r.start,r.end]),"fields":b.fields.iter().map(|f|json!({"field":f.field.get(),"spans":f.spans.iter().map(span).collect::<Vec<_>>(),"covers":f.covers.iter().map(span).collect::<Vec<_>>()})).collect::<Vec<_>>() })).collect::<Vec<_>>()})
}

/// Fixed measurement corpus shared with the frozen-output regression test.
pub fn corpora() -> Vec<(&'static str, String)> {
	let corpora=[
 ("ordinary_10k", include_str!("../../../tests/fixtures/ordinary-10k.md").to_owned()),
 ("long_code_10k", include_str!("../../../tests/fixtures/long-code-10k.md").to_owned()),
 ("details_10k", include_str!("../../../tests/fixtures/details-10k.md").to_owned()),
 ("math_10k", include_str!("../../../tests/fixtures/math-10k.md").to_owned()),
 ("prose", "# Reading\n\nThe reader renders a long paragraph with **bold**, *italic*, [a link](https://example.com) and 中文文字. Cafe\u{301} 👩‍💻 office.\n\n".repeat(120)),
 ("inline", "**bold** *italic* `code` [one](u)[two](u) plain ~~strike~~ text.\n\n".repeat(200)),
 ("images", "![a](a.png \"title\") ![b](b.png) [link](u)\n\n".repeat(120)),
 ("containers", "<details open>\n<summary>Summary</summary>\n\n> A quotation with [link](u).\n\n- First\n- Second\n\n</details>\n\n".repeat(50)),
 ("code_math_table", "```rust\nfn main() { println!(\"hello\"); }\n```\n\n$x^2+y^2$\n\n| a | b |\n| - | - |\n| c | d |\n\n".repeat(40)),
 ];
	corpora.into_iter().collect()
}

/// Durable print behavior, including page furniture in the actual PDF operators.
pub fn print(source: &str) -> Value {
	let (bytes, _, snapshot, pagination, geometry) =
		crate::pipeline::export_pdf(source);
	let pdf = lopdf::Document::load_mem(&bytes).unwrap();
	let pages: Vec<_> = pdf.get_pages().into_values().collect();
	fn object(
		o: &lopdf::Object,
		pdf: &lopdf::Document,
		pages: &[lopdf::ObjectId],
	) -> Value {
		use lopdf::Object;
		match o {
			Object::Null => Value::Null,
			Object::Boolean(b) => json!(b),
			Object::Integer(i) => json!(["int", i]),
			Object::Real(f) => json!(["real", f.to_bits()]),
			Object::Name(n) => json!(["name", n]),
			Object::String(s, _) => json!(["string", s]),
			Object::Array(a) => json!(
				a.iter().map(|o| object(o, pdf, pages)).collect::<Vec<_>>()
			),
			Object::Dictionary(d) => {
				let mut values: Vec<_> = d
					.iter()
					.map(|(k, v)| (k.clone(), object(v, pdf, pages)))
					.collect();
				values.sort_by(|a, b| a.0.cmp(&b.0));
				json!(values)
			}
			Object::Reference(id) => {
				if let Some(i) = pages.iter().position(|p| p == id) {
					json!(["page", i])
				} else {
					object(pdf.get_object(*id).unwrap(), pdf, pages)
				}
			}
			Object::Stream(s) => json!([
				object(&Object::Dictionary(s.dict.clone()), pdf, pages),
				s.content
			]),
		}
	}
	let page_records:Vec<_>=pages.iter().enumerate().map(|(i,id)| {
  let dict=pdf.get_dictionary(*id).unwrap();
  let content=lopdf::content::Content::decode(&pdf.get_page_content(*id)).unwrap();
  let operators:Vec<_>=content.operations.iter().map(|o|json!([o.operator,o.operands.iter().map(|v|object(v,&pdf,&pages)).collect::<Vec<_>>()])).collect();
  json!({"text":pdf.extract_text(&[u32::try_from(i+1).unwrap()]).unwrap(),"media_box":dict.get(b"MediaBox").ok().map(|o|object(o,&pdf,&pages)),"annotations":dict.get(b"Annots").ok().map(|o|object(o,&pdf,&pages)),"operators":operators})
 }).collect();
	let mut anchors: Vec<_> = pagination
		.anchors
		.iter()
		.map(|(a, (p, y))| json!([a, p, y.to_bits()]))
		.collect();
	anchors.sort_by_key(Value::to_string);
	let mut targets: Vec<_> = pagination
		.targets
		.iter()
		.map(|(a, (p, y))| json!([anchor(a), p, y.to_bits()]))
		.collect();
	targets.sort_by_key(Value::to_string);
	json!({"format":1,"geometry":[geometry.width_pt.to_bits(),geometry.height_pt.to_bits(),geometry.margin_pt[0].to_bits(),geometry.margin_pt[1].to_bits(),geometry.margin_pt[2].to_bits(),geometry.margin_pt[3].to_bits()],"layout":self::snapshot(&snapshot),"pagination":pagination.pages.iter().map(|page|page.iter().map(|i|json!([i.block,i.bands.start,i.bands.end,i.scale.to_bits(),i.y.to_bits(),i.top.to_bits(),i.bottom.to_bits(),i.first,i.last])).collect::<Vec<_>>()).collect::<Vec<_>>(),"anchors":anchors,"targets":targets,"pdf":page_records})
}

/// Full risk probes supplement the short excerpts used by the memory fixtures.
pub fn regression_sources() -> Vec<(&'static str, String)> {
	vec![
  ("unicode", "# Unicode\n\n[one](same)[two](same) office affine Cafe\u{301} 👩\u{200d}🚀 中文 العربية אבג A\u{ad}B\r\nnext ^raised^ $x^2$ ![alt](image.png)\n\n[^n] and [^n].\n\n[^n]: a note\n".into()),
  ("nested-flow", include_str!("../../../crates/markview-render/tests/fixtures/details-flow.md").to_owned()),
  ("complex-html", "<details open><summary>Outer</summary><dl><dt>Term</dt><dd>Definition <a href=\"same\">one</a></dd></dl><details><summary>Inner</summary><figure><p>Body <sup>2</sup> $x^2$</p><figcaption>Caption</figcaption></figure></details><table><tr><th rowspan=\"2\">A</th><th colspan=\"2\">B</th></tr><tr><td>C</td><td>D</td></tr></table></details>".into()),
  ("nested", include_str!("../../../crates/markview-render/tests/fixtures/table-forms.md").to_owned()),
 ]
}

/// The same export entry is installed in the independent old checkout.
pub fn records() -> impl Iterator<Item = (String, Value)> {
	use markview_document::{
		BlockKind, DocumentBuilder, DocumentRevision, Inline, InlineKind, Link,
		LinkOccurrenceId, TextStyle,
	};
	let mut sheet =
		(*markview_layout::style::Stylesheet::bundled(false)).clone();
	sheet.set_cjk_type(markview_layout::style::CjkType::Sc);
	let options = markview_layout::layout::LayoutOptions {
		width: 720.,
		fonts: crate::pipeline::pinned_fonts(),
		stylesheet: std::sync::Arc::new(sheet),
		..Default::default()
	};
	let mut sources = Vec::new();
	for (name, source) in corpora() {
		let end = source
			.char_indices()
			.nth(600)
			.map_or(source.len(), |(i, _)| i);
		sources.push((name.to_owned(), source[..end].to_owned(), true));
		sources.push((format!("{name}-full"), source, false));
	}
	sources.extend(
		regression_sources()
			.into_iter()
			.map(|(n, s)| (n.to_owned(), s, true)),
	);
	let reader_options = options.clone();
	let readers=sources.into_iter().map(move |(name,source,with_print)| {
        let doc=markview_commonmark::parse(source.as_str());
        let mut engine=crate::pipeline::differential_engine();
        let first=engine.layout(&doc,&reader_options);
        let value=if with_print {
            json!({"format":2,"document":document(&doc),"layout":snapshot(&first),"print":print(&source)})
        } else {
            let resized=engine.layout(&doc,&markview_layout::layout::LayoutOptions {width:660.,..reader_options.clone()});
            json!({"format":2,"document":document(&doc),"layout":snapshot(&first),"resize":snapshot(&resized)})
        };
        (name,value)
    });

	let mut builder = DocumentBuilder::new(DocumentRevision::new(1));
	let block = builder.block(
		vec!["direct".into()],
		BlockKind::Paragraph(
			["one", "two", " Cafe\u{301} 👩\u{200d}🚀 العربية 中文"]
				.into_iter()
				.enumerate()
				.map(|(i, s)| Inline {
					kind: InlineKind::Text(s.to_owned()),
					style: TextStyle::default(),
					link: (i < 2).then(|| {
						Link::new(
							"https://example.test/same",
							LinkOccurrenceId::new(
								u32::try_from(i + 1).unwrap(),
							)
							.unwrap(),
						)
					}),
				})
				.collect(),
		),
	);
	let direct = builder.finish(vec![block]);
	let web=markview_webpage::extract("<html><body><article><h1>Web heading</h1><p>Native article café 中文 with <a href='/same'>one</a><a href='/same'>two</a>.</p><details><summary>Hidden</summary><p>Search this retained body.</p></details><table><tr><th rowspan='2'>A</th><td>B</td></tr><tr><td>C</td></tr></table><figure><p>Figure body</p><figcaption>Caption</figcaption></figure></article></body></html>","https://example.test/article").unwrap().document;
	let source_free = [("direct", direct), ("webpage", web)].into_iter().map(
		move |(name, doc)| {
			let layout =
				crate::pipeline::differential_engine().layout(&doc, &options);
			let parsed = markview_commonmark::MarkdownParse {
				document: doc,
				source: "".into(),
				map: Default::default(),
				dialect: Default::default(),
			};
			(
				name.to_owned(),
				json!({"format":2,"document":document(&parsed),"layout":snapshot(&layout)}),
			)
		},
	);
	readers.chain(source_free)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn records_detect_paint_binding_and_clip_changes() {
		let doc = markview_commonmark::parse("[one](u)[two](u) **bold**");
		let mut engine = crate::pipeline::differential_engine();
		let options = crate::pipeline::options_for("sensitivity");
		let snapshot = engine.layout(&doc, &options);
		let original = &snapshot.blocks[0].layout;
		let baseline = block(original);
		for change in 0..5 {
			let mut changed = (**original).clone();
			match change {
				0 | 1 => {
					let g = changed
						.draws
						.iter_mut()
						.find_map(|d| {
							if let Draw::Glyph(g) = d {
								Some(g)
							} else {
								None
							}
						})
						.unwrap();
					if change == 0 {
						g.x += 1.;
					} else {
						g.synthetic_bold = !g.synthetic_bold;
					}
				}
				2 => {
					changed.links.get_mut(0).unwrap().target =
						LinkTarget::Disclosure(
							markview_document::BlockId::new(77).unwrap(),
						)
				}
				4 => {
					changed.links.get_mut(0).unwrap().commands =
						usize::MAX..usize::MAX
				}
				_ => changed.draws.push(Draw::Clipped {
					rect: Rect {
						x: 1.,
						y: 2.,
						w: 3.,
						h: 4.,
					},
					draws: vec![],
				}),
			}
			assert!(difference(&baseline, &block(&changed), "block").is_some());
		}
	}
}
