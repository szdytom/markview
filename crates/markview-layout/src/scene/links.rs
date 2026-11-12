//! Occurrence bindings are independent of immutable link fragment geometry.
use super::{GeometryArray, LinkId, LinkRect, LinkTarget, Rect};
use std::{ops::Range, sync::Arc};
#[derive(Clone, Debug)]
struct Geometry {
	commands: Range<usize>,
	rect: Rect,
	url: Arc<str>,
	target: LinkTarget,
}
#[derive(Clone, Debug)]
pub struct SealedLinks {
	geometry: Arc<[Geometry]>,
	ids: GeometryArray<LinkId>,
}
#[derive(Clone, Debug)]
pub enum LinkBuffer {
	#[doc(hidden)]
	Building(Vec<LinkRect>),
	#[doc(hidden)]
	Sealed(SealedLinks),
}
impl Default for LinkBuffer {
	fn default() -> Self {
		Self::Building(Vec::new())
	}
}
impl From<Vec<LinkRect>> for LinkBuffer {
	fn from(v: Vec<LinkRect>) -> Self {
		Self::Building(v)
	}
}
impl FromIterator<LinkRect> for LinkBuffer {
	fn from_iter<I: IntoIterator<Item = LinkRect>>(i: I) -> Self {
		Vec::from_iter(i).into()
	}
}
#[derive(Clone, Copy, Debug)]
pub struct LinkFragment<'a> {
	pub commands: &'a Range<usize>,
	pub id: LinkId,
	pub rect: Rect,
	pub url: &'a Arc<str>,
	pub target: &'a LinkTarget,
}
impl LinkFragment<'_> {
	pub fn owned(self) -> LinkRect {
		LinkRect {
			commands: self.commands.clone(),
			id: self.id,
			rect: self.rect,
			url: self.url.clone(),
			target: self.target.clone(),
		}
	}
}
impl LinkBuffer {
	pub fn len(&self) -> usize {
		match self {
			Self::Building(v) => v.len(),
			Self::Sealed(s) => s.geometry.len(),
		}
	}
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
	pub fn get(&self, i: usize) -> Option<LinkFragment<'_>> {
		Some(match self {
			Self::Building(v) => {
				let l = v.get(i)?;
				LinkFragment {
					commands: &l.commands,
					id: l.id,
					rect: l.rect,
					url: &l.url,
					target: &l.target,
				}
			}
			Self::Sealed(s) => {
				let l = s.geometry.get(i)?;
				LinkFragment {
					commands: &l.commands,
					id: s.ids[i],
					rect: l.rect,
					url: &l.url,
					target: &l.target,
				}
			}
		})
	}
	pub fn at(&self, i: usize) -> LinkFragment<'_> {
		self.get(i).expect("link index in bounds")
	}
	pub fn iter(&self) -> impl ExactSizeIterator<Item = LinkFragment<'_>> + '_ {
		(0..self.len()).map(|i| self.at(i))
	}
	fn builder_mut(&mut self) -> &mut Vec<LinkRect> {
		if matches!(self, Self::Sealed(_)) {
			*self = self.iter().map(LinkFragment::owned).collect();
		}
		let Self::Building(v) = self else {
			unreachable!()
		};
		v
	}
	pub fn get_mut(&mut self, i: usize) -> Option<&mut LinkRect> {
		self.builder_mut().get_mut(i)
	}
	pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, LinkRect> {
		self.builder_mut().iter_mut()
	}
	pub(crate) fn range_mut(&mut self, r: Range<usize>) -> &mut [LinkRect] {
		&mut self.builder_mut()[r]
	}
	pub fn push(&mut self, l: LinkRect) {
		self.builder_mut().push(l);
	}
	pub fn extend(&mut self, i: impl IntoIterator<Item = LinkRect>) {
		self.builder_mut().extend(i);
	}
	pub(crate) fn seal(&mut self) {
		if let Self::Building(v) = self {
			if v.is_empty() {
				return;
			}
			let v = std::mem::take(v);
			let mut ids = Vec::with_capacity(v.len());
			let geometry = v
				.into_iter()
				.map(|l| {
					ids.push(l.id);
					Geometry {
						commands: l.commands,
						rect: l.rect,
						url: l.url,
						target: l.target,
					}
				})
				.collect();
			*self = Self::Sealed(SealedLinks {
				geometry,
				ids: ids.into(),
			});
		}
	}
	pub(crate) fn rebind(&mut self, mut map: impl FnMut(LinkId) -> LinkId) {
		match self {
			Self::Building(v) => {
				for l in v {
					l.id = map(l.id)
				}
			}
			Self::Sealed(s) => {
				for id in &mut s.ids {
					*id = map(*id)
				}
			}
		}
	}
	pub fn storage_bytes(&self) -> usize {
		match self {
			Self::Building(v) => v.capacity() * size_of::<LinkRect>(),
			Self::Sealed(s) => {
				size_of_val(&*s.geometry)
					+ 16
					+ s.ids.capacity() * size_of::<LinkId>()
					+ size_of::<Vec<LinkId>>()
					+ 16
			}
		}
	}
	pub fn geometry_id(&self) -> *const () {
		match self {
			Self::Building(v) => v.as_ptr().cast(),
			Self::Sealed(s) => s.geometry.as_ptr().cast(),
		}
	}
}
pub struct LinkIter<'a> {
	buffer: &'a LinkBuffer,
	range: Range<usize>,
}
impl<'a> Iterator for LinkIter<'a> {
	type Item = LinkFragment<'a>;
	fn next(&mut self) -> Option<Self::Item> {
		self.range.next().map(|i| self.buffer.at(i))
	}
	fn size_hint(&self) -> (usize, Option<usize>) {
		self.range.size_hint()
	}
}
impl ExactSizeIterator for LinkIter<'_> {}
impl<'a> IntoIterator for &'a LinkBuffer {
	type Item = LinkFragment<'a>;
	type IntoIter = LinkIter<'a>;
	fn into_iter(self) -> Self::IntoIter {
		LinkIter {
			buffer: self,
			range: 0..self.len(),
		}
	}
}
impl<'a> IntoIterator for &'a mut LinkBuffer {
	type Item = &'a mut LinkRect;
	type IntoIter = std::slice::IterMut<'a, LinkRect>;
	fn into_iter(self) -> Self::IntoIter {
		self.iter_mut()
	}
}

#[cfg(feature = "fuzz")]
impl LinkBuffer {
	pub(super) fn visit_storage(
		&self,
		visit: &mut super::memory::StorageVisitor<'_>,
	) {
		use super::memory::allocation;
		match self {
			Self::Building(v) => {
				allocation(v, v.capacity(), "link_geometry", visit)
			}
			Self::Sealed(s) => {
				if !s.geometry.is_empty() {
					visit(
						s.geometry.as_ptr() as usize,
						"link_geometry",
						size_of_val(&*s.geometry) + 2 * size_of::<usize>(),
						s.geometry.len(),
						s.geometry.len(),
					);
				}
				s.ids.visit_storage("link_bindings", visit);
			}
		}
	}
}
