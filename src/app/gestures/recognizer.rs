use super::Point;
use std::collections::HashMap;
use winit::event::{DeviceId, TouchPhase};
pub(super) type Contact = (DeviceId, u64);
const SLOP: f32 = 8.0;

pub(super) struct Drag<T> {
	id: Contact,
	origin: Point,
	last: Point,
	axis: Option<bool>,
	target: T,
}

struct Pinch {
	first: Contact,
	second: Contact,
	distance: f32,
	centre: Point,
}

#[derive(Debug, PartialEq)]
pub(super) enum Gesture<T> {
	Tap(T),
	Pan(T, Point),
	Pinch {
		factor: f32,
		centre: Point,
		pan: Point,
	},
}

pub(super) struct Recognizer<T> {
	pub(super) contacts: HashMap<Contact, Point>,
	pub(super) drag: Option<Drag<T>>,
	pinch: Option<Pinch>,
}

impl<T> Default for Recognizer<T> {
	fn default() -> Self {
		Self {
			contacts: HashMap::new(),
			drag: None,
			pinch: None,
		}
	}
}

impl<T: Clone> Recognizer<T> {
	pub(super) fn begin(&mut self, id: Contact, point: Point, target: T) {
		let first = self.contacts.is_empty();
		self.contacts.insert(id, point);
		if first {
			self.drag = Some(Drag {
				id,
				origin: point,
				last: point,
				axis: None,
				target,
			});
		} else {
			self.drag = None;
			self.begin_pinch();
		}
	}

	fn begin_pinch(&mut self) {
		self.pinch = None;
		if self.contacts.len() != 2 {
			return;
		}
		let mut ids = self.contacts.keys();
		let first = *ids.next().unwrap();
		let second = *ids.next().unwrap();
		let a = self.contacts[&first];
		let b = self.contacts[&second];
		self.pinch = Some(Pinch {
			first,
			second,
			distance: (b.0 - a.0).hypot(b.1 - a.1),
			centre: ((a.0 + b.0) / 2., (a.1 + b.1) / 2.),
		});
	}

	fn update_pinch(&mut self) -> Option<Gesture<T>> {
		let pinch = self.pinch.as_mut().unwrap();
		let a = self.contacts[&pinch.first];
		let b = self.contacts[&pinch.second];
		let distance = (b.0 - a.0).hypot(b.1 - a.1);
		let centre = ((a.0 + b.0) / 2., (a.1 + b.1) / 2.);
		let previous = (pinch.distance, pinch.centre);
		pinch.distance = distance;
		pinch.centre = centre;
		if previous.0 <= f32::EPSILON || distance <= f32::EPSILON {
			return None;
		}
		Some(Gesture::Pinch {
			factor: distance / previous.0,
			centre: previous.1,
			pan: (centre.0 - previous.1.0, centre.1 - previous.1.1),
		})
	}

	pub(super) fn update(
		&mut self,
		id: Contact,
		phase: TouchPhase,
		point: Point,
	) -> Option<Gesture<T>> {
		if !self.contacts.contains_key(&id) {
			return None;
		}
		let ending = matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled);
		if ending {
			self.contacts.remove(&id);
		} else {
			let position = self.contacts.get_mut(&id)?;
			*position = point;
		}
		if phase == TouchPhase::Cancelled {
			self.drag = None;
			self.pinch = None;
			return None;
		}
		if ending {
			self.pinch = None;
		} else if self.pinch.is_some() {
			return self.update_pinch();
		}
		let drag = self.drag.as_mut().filter(|drag| drag.id == id)?;
		let total = (point.0 - drag.origin.0, point.1 - drag.origin.1);
		let mut delta = (point.0 - drag.last.0, point.1 - drag.last.1);
		if drag.axis.is_none() && total.0.hypot(total.1) > SLOP {
			drag.axis = Some(total.0.abs() > total.1.abs());
			delta = total;
		}
		drag.last = point;
		let result = match drag.axis {
			Some(horizontal) => Some(Gesture::Pan(
				drag.target.clone(),
				if horizontal {
					(delta.0, 0.0)
				} else {
					(0.0, delta.1)
				},
			)),
			None if ending => Some(Gesture::Tap(drag.target.clone())),
			None => None,
		};
		if ending {
			self.drag = None;
		}
		result
	}
}

pub(super) use markview_interaction::Motion;
