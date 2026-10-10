//! Session search, asynchronous matching, navigation and bottom chrome.
use super::{App, Button, SendEvent};
use crate::{
	layout::{Draw, Paint, Rect},
	state::{Command, PanelPage, TextField},
};
use markview_core::{
	document::Document,
	search::{SearchIndex, SearchMatch, SearchOptions},
	text_input::TextInput,
};
use std::{
	path::PathBuf,
	sync::{Arc, Condvar, Mutex, atomic::Ordering},
};

pub(super) const HEIGHT: f32 = 44.0;
pub(super) fn bottom(open: bool) -> f32 {
	if open { HEIGHT } else { super::BOTTOM }
}
#[derive(Default, Clone)]
pub(crate) struct SearchState {
	pub input: TextInput,
	pub document: Option<Arc<Document>>,
	pub open: bool,
	pub options: SearchOptions,
	pub matches: Arc<[SearchMatch]>,
	pub current: Option<usize>,
	pub retained: Option<SearchMatch>,
	pub pending_navigation: bool,
	pub queued_navigation: Option<bool>,
	pub dirty: bool,
	pub sequence: u64,
	pub content: Option<u64>,
	pub query: String,
	pub preparing: bool,
}
pub(super) struct Result {
	path: PathBuf,
	content: u64,
	sequence: u64,
	matches: Arc<[SearchMatch]>,
}
struct Request {
	path: PathBuf,
	content: u64,
	sequence: u64,
	document: Arc<Document>,
	query: String,
	options: SearchOptions,
}
type Inbox = crate::mailbox::Inbox<Request, Option<(PathBuf, u64)>>;

pub(super) struct Worker {
	inbox: Arc<(Mutex<Inbox>, Condvar)>,
	control: Arc<crate::mailbox::Control>,
	handle: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
	pub(super) fn new(done: impl Fn(Result) + Send + 'static) -> Self {
		let inbox = Arc::new((Mutex::new(Inbox::default()), Condvar::new()));
		let control = Arc::new(crate::mailbox::Control::default());
		let index_serial = control.clone();
		let shared = inbox.clone();
		let serial = control.clone();
		let handle = std::thread::Builder::new()
			.name("markview-search".into())
			.spawn(move || {
				let mut cache: Option<(PathBuf, u64, SearchIndex)> = None;
				loop {
					let (request, index_version) = {
						let (lock, wake) = &*shared;
						let mut inbox = Inbox::lock(lock);
						while inbox.pending.is_none()
							&& !inbox.stopped && !inbox.release
						{
							inbox = Inbox::recover(lock, wake.wait(inbox));
						}
						if inbox.stopped {
							break;
						}
						if std::mem::take(&mut inbox.release) {
							cache = None;
							if inbox.pending.is_none() {
								continue;
							}
						}
						(
							inbox.pending.take().unwrap(),
							index_serial.generation.load(Ordering::Relaxed),
						)
					};
					let cancelled = || {
						serial.sequence.load(Ordering::Relaxed)
							!= request.sequence
					};
					if cancelled() {
						continue;
					}
					if cache.as_ref().is_none_or(|(p, v, _)| {
						p != &request.path || *v != request.content
					}) {
						cache = Some((
							request.path.clone(),
							request.content,
							match SearchIndex::new_cancellable(
								&request.document,
								|| {
									index_serial
										.generation
										.load(Ordering::Relaxed) != index_version
								},
							) {
								Some(index) => index,
								None => continue,
							},
						));
					}
					if let Some(matches) =
						cache.as_ref().unwrap().2.find_cancellable(
							&request.query,
							request.options,
							cancelled,
						) && !cancelled()
					{
						done(Result {
							path: request.path,
							content: request.content,
							sequence: request.sequence,
							matches: matches.into(),
						});
					}
				}
			})
			.unwrap();
		Self {
			inbox,
			control,
			handle: Some(handle),
		}
	}
	pub(super) fn cancel(&self) -> u64 {
		self.control.sequence.fetch_add(1, Ordering::Relaxed) + 1
	}
	pub(super) fn release(&self) {
		self.cancel();
		self.control.generation.fetch_add(1, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		let mut inbox = Inbox::lock(lock);
		inbox.pending = None;
		inbox.control = None;
		inbox.release = true;
		wake.notify_one();
	}

	fn submit(&self, request: Request) {
		let (lock, wake) = &*self.inbox;
		let mut inbox = Inbox::lock(lock);
		if inbox.control.as_ref().is_none_or(|(path, content)| {
			path != &request.path || *content != request.content
		}) {
			inbox.control = Some((request.path.clone(), request.content));
			self.control.generation.fetch_add(1, Ordering::Relaxed);
		}
		if inbox.stopped {
			return;
		}
		inbox.pending = Some(request);
		wake.notify_one();
	}
}
impl Worker {
	pub(super) fn shutdown(&mut self) {
		self.cancel();
		self.control.generation.fetch_add(1, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		{
			let mut inbox = Inbox::lock(lock);
			inbox.stopped = true;
			inbox.pending = None;
		}
		wake.notify_one();
		if let Some(handle) = self.handle.take() {
			crate::shutdown::join_within(handle, "Search worker");
		}
	}
}
impl Drop for Worker {
	fn drop(&mut self) {
		self.shutdown();
	}
}
impl<P: SendEvent> App<P> {
	pub(super) fn bottom(&self) -> f32 {
		bottom(self.readers.session.search.open)
	}
	pub(super) fn open_search(&mut self) {
		self.open_search_with(false);
	}
	pub(super) fn open_new_search(&mut self) {
		self.open_search_with(true);
	}
	fn open_search_with(&mut self, fresh: bool) {
		if self.interaction.modal.is_some()
			|| self.readers.session.path.is_none()
		{
			return;
		}
		self.clear_input_focus();
		self.interaction.show_panel(PanelPage::Closed);
		self.readers.session.search.open = true;
		if fresh {
			let search = &mut self.readers.session.search;
			search.input.set_text(&mut self.ui, "");
			search.query.clear();
			search.matches = Arc::default();
			search.current = None;
			search.retained = None;
			self.search_changed();
		} else if let Some(selection) = self.interaction.selection {
			let session = &mut self.readers.session;
			let text = session
				.snapshot
				.extract_text(selection, session.accepted_revision);
			if !text.is_empty() {
				session.search.input.set_text(&mut self.ui, &text);
				self.search_changed();
			}
		}
		self.interaction.focus = Some(Command::FocusInput(TextField::Search));
		self.readers.session.search.input.select_all(&mut self.ui);
		self.sync_input();
		self.search_tick();
		self.redraw();
	}
	pub(super) fn close_search(&mut self) {
		self.clear_input_focus();
		self.readers.session.search.open = false;
		self.readers.session.search.pending_navigation = false;
		self.readers.session.search.queued_navigation = None;
		self.redraw();
	}
	pub(super) fn search_changed(&mut self) {
		let sequence = self.search_worker.cancel();
		let search = &mut self.readers.session.search;
		search.sequence = sequence;
		search.preparing = true;
		search.dirty = true;
		search.pending_navigation = false;
		search.queued_navigation = None;
		self.search_tick();
		self.redraw();
	}
	pub(super) fn search_tick(&mut self) {
		if !self.readers.session.search.open {
			return;
		}
		let session = &self.readers.session;
		if session.search.input.is_composing() {
			return;
		}
		if session.search.content != Some(session.content_version)
			&& !session.search.dirty
		{
			self.readers.session.search.matches = Arc::default();
			self.search_changed();
			return;
		}
		let session = &mut self.readers.session;
		if !session.search.dirty || !session.parse_complete {
			return;
		}
		let (Some(document), Some(path)) = (
			session
				.search
				.document
				.clone()
				.or_else(|| session.document.clone()),
			session.path.clone(),
		) else {
			return;
		};
		let search = &mut session.search;
		search.dirty = false;
		search.content = Some(session.content_version);
		search.query = search.input.text().to_owned();
		self.search_worker.submit(Request {
			path,
			content: session.content_version,
			sequence: search.sequence,
			document,
			query: search.query.clone(),
			options: search.options,
		});
	}
	pub(super) fn search_ready(&mut self, result: Result) {
		let session = &mut self.readers.session;
		if session.path.as_ref() != Some(&result.path)
			|| session.content_version != result.content
			|| session.search.sequence != result.sequence
		{
			return;
		}
		let search = &mut session.search;
		let previous = search
			.current
			.and_then(|i| search.matches.get(i))
			.cloned()
			.or_else(|| search.retained.take());
		search.current = previous.and_then(|old| {
			result
				.matches
				.binary_search_by_key(
					&(old.block, old.field, old.range.start),
					|hit| (hit.block, hit.field, hit.range.start),
				)
				.ok()
				.filter(|&i| result.matches[i] == old)
		});
		search.matches = result.matches;
		search.preparing = false;
		if let Some(backwards) = search.queued_navigation.take() {
			self.navigate_search(backwards);
		}
		self.redraw();
	}
	pub(super) fn navigate_search(&mut self, backwards: bool) {
		self.search_tick();
		let session = &mut self.readers.session;
		let search = &mut session.search;
		if search.open && search.preparing {
			session.saved_reading = None;
			search.queued_navigation = Some(backwards);
			return;
		}
		if !search.open || search.matches.is_empty() {
			return;
		}
		session.saved_reading = None;
		let count = search.matches.len();
		let index = search.current.map_or_else(
			|| {
				let y = |m: &SearchMatch| {
					session
						.snapshot
						.search_selection(m, session.accepted_revision)
						.and_then(|s| {
							let b = &session.snapshot.blocks[s.anchor.block];
							b.layout.text[s.anchor.node]
								.clusters
								.iter()
								.find(|c| c.range.contains(&s.anchor.offset))
								.and_then(|c| {
									b.rect(c.command, c.rect)
										.map(|rect| b.y + rect.y)
								})
						})
						.or_else(|| {
							session.snapshot.blocks.get(m.block).map(|b| b.y)
						})
						.unwrap_or(f32::INFINITY)
				};
				if backwards {
					search
						.matches
						.iter()
						.rposition(|m| y(m) <= session.scrolling.offset)
						.unwrap_or(count - 1)
				} else {
					search
						.matches
						.iter()
						.position(|m| y(m) >= session.scrolling.offset)
						.unwrap_or(0)
				}
			},
			|i| {
				if backwards {
					(i + count - 1) % count
				} else {
					(i + 1) % count
				}
			},
		);
		search.current = Some(index);
		search.pending_navigation = true;
		let hit = &search.matches[index];
		let mut changed = false;
		for &id in hit.enclosing.iter() {
			let declared = search
				.document
				.as_ref()
				.or(session.document.as_ref())
				.unwrap()
				.details_declared(id)
				.unwrap();
			if !session.details_open.get(&id).copied().unwrap_or(declared) {
				Arc::make_mut(&mut session.details_open).insert(id, true);
				changed = true;
			}
		}
		if changed {
			self.present_disclosures();
		}
		self.apply_search_navigation();
		self.redraw();
	}
	pub(super) fn apply_search_navigation(&mut self) {
		let viewport = self.viewport();
		let session = &mut self.readers.session;
		let search = &mut session.search;
		if !search.open || !search.pending_navigation {
			return;
		}
		session.saved_reading = None;
		if session.accepted_revision != session.content_version {
			return;
		}
		let Some(hit) = search.current.and_then(|i| search.matches.get(i))
		else {
			return;
		};
		let Some(selection) = session
			.snapshot
			.search_selection(hit, session.accepted_revision)
		else {
			self.worker.prioritize(f32::INFINITY);
			return;
		};
		let block = &session.snapshot.blocks[selection.anchor.block];
		let node = &block.layout.text[selection.anchor.node];
		let Some(cluster) = node
			.clusters
			.iter()
			.find(|c| c.range.contains(&selection.anchor.offset))
		else {
			search.pending_navigation = false;
			return;
		};
		for (oi, overflow) in block.layout.overflow.iter().enumerate() {
			if overflow.commands.contains(&cluster.command) {
				let rect = block.overflow_rect(oi).unwrap();
				let origin = block.command_offset(cluster.command).unwrap()[0];
				let offset =
					session.horizontal.entry((hit.block, oi)).or_default();
				let x = cluster.rect.x + origin;
				let end = if selection.anchor.node == selection.focus.node {
					node.clusters
						.iter()
						.filter(|c| {
							c.range.start < selection.focus.offset
								&& c.range.end > selection.anchor.offset
								&& (c.rect.y - cluster.rect.y).abs() < 0.5
						})
						.map(|c| c.rect.x + c.rect.w + origin)
						.fold(x + cluster.rect.w, f32::max)
				} else {
					x + cluster.rect.w
				};
				if x < rect.x + *offset
					|| end > rect.x + overflow.rect.w + *offset
				{
					let target =
						if end - x > overflow.rect.w || x < rect.x + *offset {
							x - rect.x
						} else {
							end - rect.x - overflow.rect.w
						};
					*offset = target.clamp(
						0.0,
						(overflow.content_width - overflow.rect.w).max(0.0),
					);
				}
			}
		}
		let y = block.y + block.rect(cluster.command, cluster.rect).unwrap().y;
		let to = if y < session.scrolling.offset
			|| y + cluster.rect.h > session.scrolling.offset + viewport
		{
			y - viewport * 0.25
		} else {
			session.scrolling.offset
		};
		session.cancel_scroll_animation();
		session.scrolling.offset = to.clamp(
			0.0,
			crate::state::scroll_limit(session.snapshot.height, viewport),
		);
		session.search.pending_navigation = false;
		self.worker.prioritize(session.coverage(viewport));
	}
	pub(super) fn search_input_rect(&self) -> Rect {
		let (width, height, _) = self.dimensions();
		Rect {
			x: 12.0,
			y: height - HEIGHT + 6.0,
			w: (width - 344.0).max(100.0),
			h: 32.0,
		}
	}
	pub(super) fn search_buttons(&self) -> Vec<Button> {
		if !self.readers.session.search.open
			|| self.interaction.modal.is_some()
			|| self.interaction.panel_open()
		{
			return vec![];
		}
		let (width, height, _) = self.dimensions();
		let lang = self.preferences.values.lang();
		let search = &self.readers.session.search;
		let actions = [
			("Aa", Command::SearchCase, search.options.case_sensitive),
			(
				lang.search_word(),
				Command::SearchWord,
				search.options.whole_word,
			),
			(lang.search_previous(), Command::SearchPrevious, false),
			(lang.search_next(), Command::SearchNext, false),
			(lang.panel_close(), Command::SearchClose, false),
		];
		std::iter::once(Button {
			kind: super::chrome::components::ButtonKind::Quiet,
			enabled: true,
			rect: self.search_input_rect(),
			label: (lang.search_placeholder()).into(),
			icon: None,
			marker: None,
			active: false,
			action: Command::FocusInput(TextField::Search),
		})
		.chain(actions.into_iter().enumerate().map(
			|(i, (label, action, active))| Button {
				kind: super::chrome::components::ButtonKind::Quiet,
				enabled: true,
				rect: Rect {
					x: width - 216.0 + [0.0, 44.0, 100.0, 136.0, 172.0][i],
					y: height - HEIGHT + 6.0,
					w: [40.0, 52.0, 32.0, 32.0, 32.0][i],
					h: 32.0,
				},
				label: label.into(),
				icon: match action {
					Command::SearchPrevious => Some(super::chrome::icons::UP),
					Command::SearchNext => Some(super::chrome::icons::DOWN),
					Command::SearchClose => Some(super::chrome::icons::CLOSE),
					_ => None,
				},
				marker: None,
				active,
				action,
			},
		))
		.collect()
	}
	pub(super) fn draw_search(&mut self) -> Vec<Draw> {
		if !self.readers.session.search.open {
			return vec![];
		}
		use markview_core::style::{ColorField as C, Condition};
		let (width, height, _) = self.dimensions();
		let search = &self.readers.session.search;
		let lang = self.preferences.values.lang();
		let count = if search.preparing {
			lang.search_preparing().to_owned()
		} else if let Some(i) = search.current {
			format!("{} / {}", i + 1, search.matches.len())
		} else {
			search.matches.len().to_string()
		};
		let mut out = vec![
			Draw::Rect(
				Rect {
					x: 0.0,
					y: height - HEIGHT,
					w: width,
					h: HEIGHT,
				},
				Paint::Styled(Condition::Statusbar, C::Background),
			),
			Draw::Rect(
				Rect {
					x: 0.0,
					y: height - HEIGHT,
					w: width,
					h: 1.0,
				},
				Paint::Styled(Condition::Statusbar, C::BorderColor),
			),
		];
		self.ui.appearance = self.ui.stylesheet.text(
			&markview_core::style::TextAppearance::default(),
			Condition::Ui,
		);
		let rect = self.search_input_rect();
		let count =
			self.ui
				.fit(&count, 12.0, width - 216.0 - rect.x - rect.w - 24.0);
		out.extend(self.ui.label(
			&count,
			12.0,
			rect.x + rect.w + 12.0,
			height - 16.0,
			Paint::Styled(Condition::Statusbar, C::Color),
		));
		for button in self.search_buttons() {
			if matches!(button.action, Command::FocusInput(_)) {
				continue;
			}
			out.extend(super::chrome::components::draw_button(
				&mut self.ui,
				&self.interaction,
				&button,
				true,
			));
		}
		out
	}
	pub(super) fn draw_search_highlights(&self) -> Vec<Draw> {
		use markview_core::style::{ColorField as C, Condition};
		let session = &self.readers.session;
		let search = &session.search;
		if !search.open
			|| search.matches.is_empty()
			|| search.content != Some(session.content_version)
			|| session.accepted_revision != session.content_version
		{
			return vec![];
		}
		let geometry = self.view_geometry();
		let visible = session.scrolling.offset
			..session.scrolling.offset + self.viewport();
		let mut out = Vec::new();
		session.snapshot.visit_search_clusters(
			&session.horizontal,
			visible,
			|bi, field, range, mut rect| {
				let key = (bi, field);
				let first = search.matches.partition_point(|hit| {
					(hit.block, hit.field) < key
						|| ((hit.block, hit.field) == key
							&& hit.range.end <= range.start)
				});
				let mut active = None;
				for (i, hit) in search.matches.iter().enumerate().skip(first) {
					if (hit.block, hit.field) != key
						|| hit.range.start >= range.end
					{
						break;
					}
					active = Some(
						active.unwrap_or(false) || search.current == Some(i),
					);
				}
				if let Some(active) = active {
					rect.x += geometry.left;
					rect.y += geometry.top - geometry.scroll;
					out.push(Draw::Rect(
						rect,
						Paint::Styled(
							if active {
								Condition::SearchCurrent
							} else {
								Condition::Search
							},
							C::Background,
						),
					));
				}
			},
		);
		vec![Draw::Clipped {
			rect: geometry.clip(),
			draws: out,
		}]
	}
}

#[cfg(test)]
mod tests;
