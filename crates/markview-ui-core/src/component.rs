use std::collections::VecDeque;

/// Layout invalidation implies paint invalidation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Invalidation {
	#[default]
	None,
	Paint,
	Layout,
}

/// Ordered effects and the highest requested invalidation. Merging is associative
/// with an empty `None` update as identity; effect order makes it noncommutative.
#[derive(Debug)]
pub struct Update<E> {
	pub invalidation: Invalidation,
	pub effects: Vec<E>,
}

impl<E> Update<E> {
	pub fn new(invalidation: Invalidation) -> Self {
		Self {
			invalidation,
			effects: Vec::new(),
		}
	}
	pub fn effect(mut self, effect: E) -> Self {
		self.effects.push(effect);
		self
	}
	pub fn merge(&mut self, other: Self) {
		self.invalidation = self.invalidation.max(other.invalidation);
		self.effects.extend(other.effects);
	}
	/// Maps effects in order without changing invalidation.
	pub fn map<F>(self, f: impl FnMut(E) -> F) -> Update<F> {
		Update {
			invalidation: self.invalidation,
			effects: self.effects.into_iter().map(f).collect(),
		}
	}
}

pub trait Component {
	type Message;
	type Effect;
	type View<'a>
	where
		Self: 'a;

	fn update(&mut self, message: Self::Message) -> Update<Self::Effect>;
	fn view(&self) -> Self::View<'_>;
}

/// FIFO scheduling without reentrant updates or effect execution.
pub struct Runtime<C: Component> {
	pub component: C,
	queue: VecDeque<C::Message>,
}

impl<C: Component> Runtime<C> {
	pub fn new(component: C) -> Self {
		Self {
			component,
			queue: VecDeque::new(),
		}
	}
	pub fn send(&mut self, message: C::Message) {
		self.queue.push_back(message);
	}
	/// Processes queued messages once in FIFO order and merges their updates.
	/// An empty flush returns no effects and no invalidation.
	pub fn flush(&mut self) -> Update<C::Effect> {
		let mut update = Update::new(Invalidation::None);
		while let Some(message) = self.queue.pop_front() {
			update.merge(self.component.update(message));
		}
		update
	}
	pub fn view(&self) -> C::View<'_> {
		self.component.view()
	}
}

#[cfg(test)]
mod proptests;
