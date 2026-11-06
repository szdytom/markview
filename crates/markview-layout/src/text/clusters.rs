//! Exact cluster geometry with shared row and spacing records.
use super::TextCluster;
use crate::{layout::Rect, scene::GeometryArray};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Copy, Debug)]
struct CompactCluster {
	range: [u32; 2],
	command: u32,
	row: u32,
	x: f32,
	width: f32,
	flags_spacing: u32,
}
#[derive(Debug)]
pub struct PackedClusters {
	clusters: Box<[CompactCluster]>,
	rows: Box<[[f32; 2]]>,
	spacing: Box<[[f32; 2]]>,
}
#[derive(Clone, Debug)]
enum Storage {
	Building(GeometryArray<TextCluster>),
	Packed(Arc<PackedClusters>),
}
/// Logical command placement is independent of the immutable cluster allocation.
#[derive(Clone, Debug)]
pub struct ClusterBuffer {
	storage: Storage,
	origin: usize,
	placement: usize,
}
impl Default for ClusterBuffer {
	fn default() -> Self {
		Vec::new().into()
	}
}
impl From<Vec<TextCluster>> for ClusterBuffer {
	fn from(v: Vec<TextCluster>) -> Self {
		Self {
			storage: Storage::Building(v.into()),
			origin: 0,
			placement: 0,
		}
	}
}
impl FromIterator<TextCluster> for ClusterBuffer {
	fn from_iter<I: IntoIterator<Item = TextCluster>>(iter: I) -> Self {
		Vec::from_iter(iter).into()
	}
}
impl ClusterBuffer {
	pub fn len(&self) -> usize {
		match &self.storage {
			Storage::Building(v) => v.len(),
			Storage::Packed(p) => p.clusters.len(),
		}
	}
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}
	pub fn capacity(&self) -> usize {
		match &self.storage {
			Storage::Building(v) => v.capacity(),
			Storage::Packed(_) => self.len(),
		}
	}
	pub fn get(&self, i: usize) -> Option<TextCluster> {
		let mut c = match &self.storage {
			Storage::Building(v) => v.get(i)?.clone(),
			Storage::Packed(p) => {
				let c = p.clusters.get(i)?;
				let [y, h] = p.rows[c.row as usize];
				let [left, right] = p.spacing[(c.flags_spacing >> 2) as usize];
				TextCluster {
					range: c.range[0] as usize..c.range[1] as usize,
					command: c.command as usize,
					rect: Rect {
						x: c.x,
						y,
						w: c.width,
						h,
					},
					mixed_spacing: (left, right),
					rtl: c.flags_spacing & 1 != 0,
					atomic: c.flags_spacing & 2 != 0,
				}
			}
		};
		c.command = c.command - self.origin + self.placement;
		Some(c)
	}
	pub fn at(&self, i: usize) -> TextCluster {
		self.get(i).expect("cluster index in bounds")
	}
	pub fn first(&self) -> Option<TextCluster> {
		self.get(0)
	}
	pub fn last(&self) -> Option<TextCluster> {
		self.len().checked_sub(1).and_then(|i| self.get(i))
	}
	pub fn iter(
		&self,
	) -> impl DoubleEndedIterator<Item = TextCluster> + ExactSizeIterator + '_
	{
		(0..self.len()).map(|i| self.at(i))
	}
	pub fn windows(
		&self,
		size: usize,
	) -> impl Iterator<Item = Vec<TextCluster>> + '_ {
		assert!(size != 0);
		(0..self.len().saturating_sub(size - 1))
			.map(move |i| (i..i + size).map(|i| self.at(i)).collect())
	}
	fn builder_mut(&mut self) -> &mut Vec<TextCluster> {
		if !matches!(self.storage, Storage::Building(_))
			|| self.origin != 0
			|| self.placement != 0
		{
			*self = self.iter().collect::<Vec<_>>().into();
		}
		let Storage::Building(v) = &mut self.storage else {
			unreachable!()
		};
		v
	}
	pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, TextCluster> {
		self.builder_mut().iter_mut()
	}
	pub fn push(&mut self, c: TextCluster) {
		self.builder_mut().push(c);
	}
	pub fn clear(&mut self) {
		*self = Self::default();
	}
	pub(crate) fn relocate(&mut self, from: usize, to: usize) {
		// Every cluster belongs to the complete relocated drawing span.
		self.origin += from - self.placement;
		self.placement = to;
	}
	pub(crate) fn seal(&mut self) {
		if !matches!(self.storage, Storage::Building(_)) {
			return;
		}
		let mut rows = Vec::new();
		let mut row_ids = HashMap::new();
		let mut spacing = Vec::new();
		let mut spacing_ids = HashMap::new();
		let mut last_row = None;
		let mut last_spacing = None;
		let mut clusters = Vec::with_capacity(self.len());
		let packed: Option<()> = self.iter().try_for_each(|c| {
			let row_key = [c.rect.y.to_bits(), c.rect.h.to_bits()];
			let row = if let Some((key, index)) = last_row
				&& key == row_key
			{
				index
			} else {
				let index = *row_ids.entry(row_key).or_insert_with(|| {
					let i = rows.len();
					rows.push([c.rect.y, c.rect.h]);
					i
				});
				last_row = Some((row_key, index));
				index
			};
			let spacing_key =
				[c.mixed_spacing.0.to_bits(), c.mixed_spacing.1.to_bits()];
			let spacing_id = if let Some((key, index)) = last_spacing
				&& key == spacing_key
			{
				index
			} else {
				let index =
					*spacing_ids.entry(spacing_key).or_insert_with(|| {
						let i = spacing.len();
						spacing.push([c.mixed_spacing.0, c.mixed_spacing.1]);
						i
					});
				last_spacing = Some((spacing_key, index));
				index
			};
			clusters.push(CompactCluster {
				range: [
					u32::try_from(c.range.start).ok()?,
					u32::try_from(c.range.end).ok()?,
				],
				command: u32::try_from(c.command).ok()?,
				row: u32::try_from(row).ok()?,
				x: c.rect.x,
				width: c.rect.w,
				flags_spacing: u32::try_from(spacing_id)
					.ok()?
					.checked_mul(4)?
					| u32::from(c.rtl)
					| (u32::from(c.atomic) << 1),
			});
			Some(())
		});
		if packed.is_some() {
			let packed = PackedClusters {
				clusters: clusters.into_boxed_slice(),
				rows: rows.into_boxed_slice(),
				spacing: spacing.into_boxed_slice(),
			};
			// Tiny image and formula nodes can cost less in their native representation.
			if size_of::<PackedClusters>()
				+ 16 + size_of_val(&*packed.clusters)
				+ size_of_val(&*packed.rows)
				+ size_of_val(&*packed.spacing)
				< self.len() * size_of::<TextCluster>()
					+ size_of::<Vec<TextCluster>>()
					+ 16
			{
				self.storage = Storage::Packed(Arc::new(packed));
				self.origin = 0;
				self.placement = 0;
			} else if let Storage::Building(v) = &mut self.storage
				&& v.len() != v.capacity()
			{
				v.shrink_to_fit();
			}
		}
	}
	pub fn storage_bytes(&self) -> usize {
		match &self.storage {
			Storage::Building(v) => {
				v.capacity() * size_of::<TextCluster>()
					+ size_of::<Vec<TextCluster>>()
					+ 16
			}
			Storage::Packed(p) => {
				size_of::<PackedClusters>()
					+ 16 + size_of_val(&*p.clusters)
					+ size_of_val(&*p.rows)
					+ size_of_val(&*p.spacing)
			}
		}
	}
	pub fn as_ptr(&self) -> *const () {
		match &self.storage {
			Storage::Building(v) => v.storage_id(),
			Storage::Packed(p) => Arc::as_ptr(p).cast(),
		}
	}
}
pub struct ClusterIter<'a> {
	buffer: &'a ClusterBuffer,
	range: std::ops::Range<usize>,
}
impl Iterator for ClusterIter<'_> {
	type Item = TextCluster;
	fn next(&mut self) -> Option<TextCluster> {
		self.range.next().map(|i| self.buffer.at(i))
	}
	fn size_hint(&self) -> (usize, Option<usize>) {
		self.range.size_hint()
	}
}
impl DoubleEndedIterator for ClusterIter<'_> {
	fn next_back(&mut self) -> Option<TextCluster> {
		self.range.next_back().map(|i| self.buffer.at(i))
	}
}
impl ExactSizeIterator for ClusterIter<'_> {}
impl<'a> IntoIterator for &'a ClusterBuffer {
	type Item = TextCluster;
	type IntoIter = ClusterIter<'a>;
	fn into_iter(self) -> Self::IntoIter {
		ClusterIter {
			buffer: self,
			range: 0..self.len(),
		}
	}
}
impl<'a> IntoIterator for &'a mut ClusterBuffer {
	type Item = &'a mut TextCluster;
	type IntoIter = std::slice::IterMut<'a, TextCluster>;
	fn into_iter(self) -> Self::IntoIter {
		self.iter_mut()
	}
}
impl IntoIterator for ClusterBuffer {
	type Item = TextCluster;
	type IntoIter = std::vec::IntoIter<TextCluster>;
	fn into_iter(self) -> Self::IntoIter {
		self.iter().collect::<Vec<_>>().into_iter()
	}
}

#[cfg(feature = "fuzz")]
impl ClusterBuffer {
	pub(super) fn visit_storage(
		&self,
		visit: &mut crate::scene::StorageVisitor<'_>,
	) {
		use crate::scene::memory::allocation;
		match &self.storage {
			Storage::Building(v) => v.visit_storage("clusters", visit),
			Storage::Packed(p) => {
				visit(
					Arc::as_ptr(p) as usize,
					"clusters",
					size_of::<PackedClusters>() + 2 * size_of::<usize>(),
					0,
					0,
				);
				allocation(&p.clusters, p.clusters.len(), "clusters", visit);
				allocation(&p.rows, p.rows.len(), "cluster_rows", visit);
				allocation(
					&p.spacing,
					p.spacing.len(),
					"cluster_spacing",
					visit,
				);
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn sample(i: usize) -> TextCluster {
		TextCluster {
			range: i * 2..i * 2 + 2,
			command: 400 + i,
			rect: Rect {
				x: i as f32,
				y: if i.is_multiple_of(2) { -0. } else { 3.25 },
				w: 7.125,
				h: if i.is_multiple_of(3) { 12. } else { 19.5 },
			},
			mixed_spacing: (-0., 0.5),
			rtl: i.is_multiple_of(2),
			atomic: i.is_multiple_of(5),
		}
	}
	fn bits(c: TextCluster) -> ([usize; 3], [u32; 6], bool, bool) {
		(
			[c.range.start, c.range.end, c.command],
			[
				c.rect.x,
				c.rect.y,
				c.rect.w,
				c.rect.h,
				c.mixed_spacing.0,
				c.mixed_spacing.1,
			]
			.map(f32::to_bits),
			c.rtl,
			c.atomic,
		)
	}
	#[test]
	fn mixed_baselines_and_relocated_snapshots_preserve_exact_geometry() {
		let original: Vec<_> = (0..1000).map(sample).collect();
		let mut buffer: ClusterBuffer = original.clone().into();
		buffer.seal();
		assert!(
			buffer.storage_bytes() < 1000 * size_of::<TextCluster>() * 3 / 4
		);
		assert_eq!(
			buffer.iter().map(bits).collect::<Vec<_>>(),
			original.into_iter().map(bits).collect::<Vec<_>>()
		);
		let mut placed = buffer.clone();
		placed.relocate(400, 10);
		assert_eq!(placed.as_ptr(), buffer.as_ptr());
		assert_eq!(placed.at(999).command, 1009);
		placed.relocate(10, 900);
		assert_eq!(placed.at(0).command, 900);
		placed.iter_mut().next().unwrap().rect.x = 88.;
		assert_eq!(buffer.at(0).rect.x.to_bits(), 0f32.to_bits());
		let Storage::Packed(p) = &buffer.storage else {
			panic!("packed")
		};
		let weak = Arc::downgrade(p);
		drop(buffer);
		assert!(weak.upgrade().is_none());
	}
	#[test]
	fn native_fallback_preserves_large_producer_offsets() {
		let Ok(large) = usize::try_from(u64::from(u32::MAX) + 1) else {
			return;
		};
		for c in [
			TextCluster {
				command: large,
				..sample(0)
			},
			TextCluster {
				range: large..large + 1,
				..sample(0)
			},
		] {
			let mut buffer: ClusterBuffer = vec![c.clone(); 100].into();
			buffer.seal();
			assert!(matches!(buffer.storage, Storage::Building(_)));
			assert_eq!(bits(buffer.at(99)), bits(c));
		}
	}
}
