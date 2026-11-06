//! Copy-on-write arrays separate immutable geometry from occurrence bindings.
use std::{
	ops::{Deref, DerefMut},
	sync::Arc,
};
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GeometryArray<T>(Arc<Vec<T>>);
impl<T> GeometryArray<T> {
	pub(crate) fn heap_bytes(&self) -> usize {
		size_of::<Vec<T>>()
			+ 2 * size_of::<usize>()
			+ self.0.capacity() * size_of::<T>()
	}

	pub(crate) fn storage_id(&self) -> *const () {
		Arc::as_ptr(&self.0).cast()
	}
}
impl<T> Default for GeometryArray<T> {
	fn default() -> Self {
		Self(Arc::new(Vec::new()))
	}
}
impl<T> From<Vec<T>> for GeometryArray<T> {
	fn from(v: Vec<T>) -> Self {
		Self(Arc::new(v))
	}
}
impl<T> Deref for GeometryArray<T> {
	type Target = Vec<T>;
	fn deref(&self) -> &Vec<T> {
		&self.0
	}
}
impl<T: Clone> DerefMut for GeometryArray<T> {
	fn deref_mut(&mut self) -> &mut Vec<T> {
		Arc::make_mut(&mut self.0)
	}
}
impl<'a, T> IntoIterator for &'a GeometryArray<T> {
	type Item = &'a T;
	type IntoIter = std::slice::Iter<'a, T>;
	fn into_iter(self) -> Self::IntoIter {
		self.iter()
	}
}
impl<'a, T: Clone> IntoIterator for &'a mut GeometryArray<T> {
	type Item = &'a mut T;
	type IntoIter = std::slice::IterMut<'a, T>;
	fn into_iter(self) -> Self::IntoIter {
		self.iter_mut()
	}
}
impl<T: Clone> IntoIterator for GeometryArray<T> {
	type Item = T;
	type IntoIter = std::vec::IntoIter<T>;
	fn into_iter(self) -> Self::IntoIter {
		Arc::unwrap_or_clone(self.0).into_iter()
	}
}
impl<T> FromIterator<T> for GeometryArray<T> {
	fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
		Vec::from_iter(iter).into()
	}
}

#[cfg(feature = "fuzz")]
impl<T> GeometryArray<T> {
	pub(crate) fn visit_storage(
		&self,
		category: &'static str,
		visit: &mut super::memory::StorageVisitor<'_>,
	) {
		visit(
			self.storage_id() as usize,
			category,
			size_of::<Vec<T>>() + 2 * size_of::<usize>(),
			0,
			0,
		);
		super::memory::allocation(self, self.capacity(), category, visit);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn mutations_detach_and_final_owner_releases_storage() {
		let mut original: GeometryArray<_> = vec![1, 2, 3].into();
		let shared = original.clone();
		let old = Arc::downgrade(&shared.0);
		assert_eq!(original.as_ptr(), shared.as_ptr());
		original[0] = 9;
		assert_eq!(&**shared, &[1, 2, 3]);
		assert_eq!(&**original, &[9, 2, 3]);
		drop(shared);
		assert!(old.upgrade().is_none());
	}
}
