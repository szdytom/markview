use markview_geometry::Rect;

/// Stable semantic identity, independent of a widget's message or geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId {
	pub scope: u64,
	pub key: u64,
}
impl WidgetId {
	pub const fn new(scope: u64, key: u64) -> Self {
		Self { scope, key }
	}
}

/// The layout stage supplies geometry once; painting and input borrow it.
#[derive(Clone, Debug)]
pub struct Widget<M> {
	pub id: WidgetId,
	pub rect: Rect,
	pub clip: Option<Rect>,
	pub enabled: bool,
	pub focusable: bool,
	pub message: M,
}
impl<M> Widget<M> {
	pub fn hit_rect(&self) -> Option<Rect> {
		match self.clip {
			Some(clip) => self.rect.intersect(clip),
			None => Some(self.rect),
		}
	}
}

/// Frontmost widgets come first. A disabled widget still blocks hits behind it.
#[derive(Clone, Debug)]
/// Each layout must supply unique widget identities. Identity persists across
/// movement and is independent of the activation message.
pub struct Layout<M> {
	pub widgets: Vec<Widget<M>>,
}
impl<M> Default for Layout<M> {
	fn default() -> Self {
		Self {
			widgets: Vec::new(),
		}
	}
}
impl<M> Layout<M> {
	pub fn hit(&self, point: (f32, f32)) -> Option<&Widget<M>> {
		self.widgets.iter().find(|w| {
			w.hit_rect().is_some_and(|r| r.contains(point.0, point.1))
		})
	}
	pub fn get(&self, id: WidgetId) -> Option<&Widget<M>> {
		self.widgets.iter().find(|w| w.id == id)
	}
	/// Prefer exact hits, then the nearest expanded touch target within its clip.
	pub fn hit_touch(
		&self,
		point: (f32, f32),
		minimum: f32,
	) -> Option<&Widget<M>> {
		self.hit(point).or_else(|| {
			self.widgets
				.iter()
				.filter(|widget| {
					let rect = widget.rect;
					let w = rect.w.max(minimum);
					let h = rect.h.max(minimum);
					let expanded = Rect {
						x: rect.x + (rect.w - w) / 2.0,
						y: rect.y + (rect.h - h) / 2.0,
						w,
						h,
					};
					let hit = widget.clip.map_or(Some(expanded), |clip| {
						expanded.intersect(clip)
					});
					widget.hit_rect().is_some_and(|r| r.w > 0.0 && r.h > 0.0)
						&& hit.is_some_and(|r| r.contains(point.0, point.1))
				})
				.min_by(|a, b| {
					let distance = |rect: Rect| {
						(point.0 - rect.x - rect.w / 2.0)
							.hypot(point.1 - rect.y - rect.h / 2.0)
					};
					distance(a.rect).total_cmp(&distance(b.rect))
				})
		})
	}
	pub fn next_focus(
		&self,
		current: Option<WidgetId>,
		backward: bool,
	) -> Option<WidgetId> {
		let ids: Vec<_> = self
			.widgets
			.iter()
			.filter(|w| w.enabled && w.focusable)
			.map(|w| w.id)
			.collect();
		let index = match ids.iter().position(|id| Some(*id) == current) {
			Some(i) => {
				(i + if backward { ids.len() - 1 } else { 1 }) % ids.len()
			}
			None if backward => ids.len().checked_sub(1)?,
			None => 0,
		};
		ids.get(index).copied()
	}
}

#[derive(Clone, Copy, Debug)]
pub enum Input {
	Move((f32, f32)),
	Press((f32, f32)),
	Release((f32, f32)),
	Navigate { backward: bool },
	Activate,
	Cancel,
	Blur,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Interaction {
	pub hovered: Option<WidgetId>,
	pub focus: Option<WidgetId>,
	pub captured: Option<WidgetId>,
	pub focus_visible: bool,
}
impl Interaction {
	/// Removed or disabled controls cannot retain a capture or keyboard focus.
	pub fn reconcile<M>(&mut self, layout: &Layout<M>) {
		self.focus = self.focus.filter(|id| {
			layout.get(*id).is_some_and(|w| w.enabled && w.focusable)
		});
		self.captured = self
			.captured
			.filter(|id| layout.get(*id).is_some_and(|w| w.enabled));
		self.hovered = self
			.hovered
			.filter(|id| layout.get(*id).is_some_and(|w| w.enabled));
	}
	pub fn handle<M: Clone>(
		&mut self,
		layout: &Layout<M>,
		input: Input,
	) -> Option<M> {
		self.reconcile(layout);
		match input {
			Input::Move(point)
			| Input::Press(point)
			| Input::Release(point) => {
				let hit = layout.hit(point).filter(|w| w.enabled);
				self.hovered = hit.map(|w| w.id);
				match input {
					Input::Press(_) => {
						self.captured = self.hovered;
						self.focus = hit.filter(|w| w.focusable).map(|w| w.id);
						self.focus_visible = false;
					}
					Input::Release(_) => {
						let captured = self.captured.take();
						return hit
							.filter(|w| Some(w.id) == captured)
							.map(|w| w.message.clone());
					}
					_ => {}
				}
			}
			Input::Navigate { backward } => {
				self.focus = layout.next_focus(self.focus, backward);
				self.focus_visible = true;
			}
			Input::Activate => {
				return self
					.focus
					.and_then(|id| layout.get(id))
					.map(|w| w.message.clone());
			}
			Input::Cancel => self.captured = None,
			Input::Blur => *self = Self::default(),
		}
		None
	}
}

#[cfg(test)]
mod proptests;
