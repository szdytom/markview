//! Transfers a confirmed preview activation to its owning reader session.
use super::*;
use markview_document::{
	BlockId, ContentKey, FieldId, FieldText, LinkOccurrenceId,
	inline_link_origins,
};
use markview_layout::scene::{LinkId, LinkTarget};

#[derive(Clone)]
enum Location {
	Inline {
		root: usize,
		field: FieldId,
		occurrence: usize,
	},
	Disclosure(Vec<usize>),
	Return,
}

#[derive(Default)]
pub(super) struct Origins {
	inline: HashMap<LinkOccurrenceId, Location>,
	disclosures: HashMap<BlockId, Vec<usize>>,
}
impl Origins {
	pub fn new(
		document: &markview_document::Document,
		snapshot: &LayoutSnapshot,
	) -> Self {
		let mut origins = Self::default();
		let mut wanted = HashSet::new();
		for (link, _) in snapshot.blocks.iter().flat_map(|block| block.links())
		{
			if let LinkId::Inline(id) = link.id {
				wanted.insert(id);
			}
			if let LinkTarget::Disclosure(id) = link.target {
				origins
					.disclosures
					.insert(*id, document.path_of(*id).unwrap().to_vec());
			}
		}
		for (root, block) in document.blocks().iter().enumerate() {
			markview_document::visit_fields(
				block,
				&mut Vec::new(),
				&|| false,
				&mut |_, field, text, _| {
					if let FieldText::Rich(text) = text {
						for (occurrence, id) in
							inline_link_origins(text).into_iter().enumerate()
						{
							if wanted.contains(&id) {
								origins.inline.entry(id).or_insert(
									Location::Inline {
										root,
										field,
										occurrence,
									},
								);
							}
						}
					}
				},
			);
		}
		origins
	}
	fn location(&self, link: LinkId, target: &LinkTarget) -> Location {
		match target {
			LinkTarget::Disclosure(id) => {
				Location::Disclosure(self.disclosures[id].clone())
			}
			LinkTarget::FootnoteReturn(_) => Location::Return,
			_ => {
				let LinkId::Inline(id) = link else {
					unreachable!("inline links have occurrence identities");
				};
				self.inline[&id].clone()
			}
		}
	}
}
impl Location {
	fn resolve(
		&self,
		document: &markview_document::Document,
		target: &mut LinkTarget,
	) -> Option<LinkId> {
		match self {
			Self::Disclosure(path) => {
				*target = LinkTarget::Disclosure(
					document.block_at_path(path).unwrap().id(),
				);
				None
			}
			Self::Return => None,
			Self::Inline {
				root,
				field,
				occurrence,
			} => {
				let mut resolved = None;
				markview_document::visit_fields(
					&document.blocks()[*root],
					&mut Vec::new(),
					&|| false,
					&mut |_, id, text, _| {
						if id == *field
							&& let FieldText::Rich(text) = text
						{
							resolved = Some(LinkId::Inline(
								inline_link_origins(text)[*occurrence],
							));
						}
					},
				);
				Some(
					resolved
						.expect("matching content retains the clicked field"),
				)
			}
		}
	}
}

#[derive(Clone)]
pub(crate) struct PendingAction {
	pub content: ContentKey,
	pub revision: u64,
	location: Location,
	pub target: LinkTarget,
	pub expanded: Option<bool>,
}

impl<P: super::super::SendEvent> App<P> {
	pub(super) fn activate_file_preview(&mut self, target: LinkTarget) {
		let open = self.preview.open().unwrap();
		let path = open.path.clone().unwrap();
		let floating = open.floating.as_ref().unwrap();
		let link = floating.state.hover.link.as_ref().unwrap().id;
		let content = floating.snapshot.content_id;
		let location = open.origins.location(link, &target);
		let expanded = if let LinkTarget::Disclosure(id) = target {
			floating
				.snapshot
				.blocks
				.iter()
				.find_map(|block| block.disclosure_open(id))
				.map(|open| !open)
		} else {
			None
		};
		let security = crate::security::Security::local(
			self.readers.session.security.origin.trust(),
		);
		if self.readers.session.security.check_file(&path).is_err() {
			self.dismiss_preview();
			return;
		}
		self.dismiss_preview();
		self.open_document(path, security);
		self.readers.session.saved_reading = None;
		self.readers.session.pending_anchor = None;
		self.readers.session.pending_preview_action = Some(PendingAction {
			content,
			revision: self.readers.session.content_version,
			location,
			target,
			expanded,
		});
		self.apply_anchor();
	}

	pub(in crate::app) fn apply_preview_action(&mut self) {
		let session = &self.readers.session;
		let Some(pending) = &session.pending_preview_action else {
			return;
		};
		if pending.revision != session.content_version
			|| session.load_error.is_some()
		{
			self.readers.session.pending_preview_action = None;
			return;
		}
		if session.accepted_revision != session.content_version
			|| !session.snapshot_complete
		{
			return;
		}
		let mut pending =
			self.readers.session.pending_preview_action.take().unwrap();
		if self.readers.session.accepted_content_id != pending.content.get() {
			return;
		}
		let document = self.readers.session.document.as_ref().unwrap();
		let link = pending.location.resolve(document, &mut pending.target);
		let enclosing = enclosing(document, link, &pending.target);
		for id in enclosing {
			std::sync::Arc::make_mut(&mut self.readers.session.details_open)
				.insert(id, true);
		}
		self.present_disclosures();
		let y = self
			.readers
			.session
			.display
			.snapshot
			.blocks
			.iter()
			.find_map(|block| {
				block
					.links()
					.find(|(fragment, _)| {
						link.is_none_or(|id| fragment.id == id)
							&& *fragment.target == pending.target
					})
					.map(|(_, rect)| block.y + rect.y)
			});
		let Some(y) = y else {
			return;
		};
		self.readers.session.cancel_scroll_animation();
		self.readers.session.scrolling.offset = y.clamp(
			0.0,
			self.readers
				.session
				.display
				.snapshot
				.scroll_limit(self.viewport()),
		);
		if let LinkTarget::Disclosure(id) = pending.target {
			std::sync::Arc::make_mut(&mut self.readers.session.details_open)
				.insert(id, pending.expanded.unwrap());
			self.present_disclosures();
		} else {
			self.open_link_target(&pending.target, false);
		}
		self.after_scroll();
	}
}

fn enclosing(
	document: &markview_document::Document,
	link: Option<LinkId>,
	target: &LinkTarget,
) -> Vec<BlockId> {
	match target {
		LinkTarget::Disclosure(id) => {
			let path = document.path_of(*id).unwrap();
			(1..path.len())
				.filter_map(|end| document.block_at_path(&path[..end]))
				.filter(|block| document.details_declared(block.id()).is_some())
				.map(|block| block.id())
				.collect()
		}
		LinkTarget::FootnoteReturn(id) => document.details_enclosing(
			markview_document::AnchorId::FootnoteDefinition(id.clone()),
		),
		_ => {
			let Some(LinkId::Inline(occurrence)) = link else {
				return Vec::new();
			};
			let mut found = Vec::new();
			for block in document.blocks() {
				markview_document::visit_fields(
					block,
					&mut Vec::new(),
					&|| false,
					&mut |_, _, text, parents| {
						if let FieldText::Rich(text) = text
							&& inline_link_origins(text).contains(&occurrence)
						{
							found = parents.to_vec();
						}
					},
				);
			}
			found
		}
	}
}
