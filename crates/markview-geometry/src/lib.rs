//! Shared geometry for document layout and native UI.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
	pub x: f32,
	pub y: f32,
	pub w: f32,
	pub h: f32,
}
impl Rect {
	/// Returns only positive-area overlap; touching edges and degenerate boxes have none.
	/// Geometry callers supply finite coordinates and nonnegative sizes without overflow.
	pub fn intersect(self, other: Self) -> Option<Self> {
		let x = self.x.max(other.x);
		let y = self.y.max(other.y);
		let w = (self.x + self.w).min(other.x + other.w) - x;
		let h = (self.y + self.h).min(other.y + other.h) - y;
		(w > 0.0 && h > 0.0).then_some(Self { x, y, w, h })
	}
	/// Tests inclusive edges of a positive-size rectangle; zero or negative sizes miss.
	pub fn contains(self, x: f32, y: f32) -> bool {
		self.w > 0.0
			&& self.h > 0.0
			&& x >= self.x
			&& x <= self.x + self.w
			&& y >= self.y
			&& y <= self.y + self.h
	}
}

#[cfg(test)]
mod proptests;
