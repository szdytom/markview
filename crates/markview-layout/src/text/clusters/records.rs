//! Short source ranges and row references share a compact record when they fit.
use super::CompactCluster;

#[derive(Clone, Copy, Debug)]
pub(super) struct NarrowCluster {
	start: u32,
	command: u32,
	x: f32,
	width: f32,
	len: u16,
	row_flags: u16,
}

#[derive(Debug)]
pub(super) enum Records {
	Wide(Box<[CompactCluster]>),
	Narrow {
		clusters: Box<[NarrowCluster]>,
		row_shift: u32,
	},
}
impl Records {
	pub(super) fn new(
		clusters: Vec<CompactCluster>,
		spacing_count: usize,
	) -> Self {
		let row_shift =
			2 + usize::BITS - spacing_count.saturating_sub(1).leading_zeros();
		if row_shift > u16::BITS
			|| !clusters.iter().all(|c| {
				c.row <= u32::from(u16::MAX) >> row_shift
					&& c.flags_spacing < 1 << row_shift
					&& c.range[1]
						.checked_sub(c.range[0])
						.is_some_and(|len| u16::try_from(len).is_ok())
			}) {
			return Self::Wide(clusters.into_boxed_slice());
		}
		Self::Narrow {
			clusters: clusters
				.into_iter()
				.map(|c| NarrowCluster {
					start: c.range[0],
					command: c.command,
					x: c.x,
					width: c.width,
					len: (c.range[1] - c.range[0]) as u16,
					row_flags: ((c.row << row_shift) | c.flags_spacing) as u16,
				})
				.collect(),
			row_shift,
		}
	}
	pub(super) fn len(&self) -> usize {
		match self {
			Self::Wide(clusters) => clusters.len(),
			Self::Narrow { clusters, .. } => clusters.len(),
		}
	}
	pub(super) fn get(&self, index: usize) -> Option<CompactCluster> {
		match self {
			Self::Wide(clusters) => clusters.get(index).copied(),
			Self::Narrow {
				clusters,
				row_shift,
			} => {
				let c = clusters.get(index)?;
				Some(CompactCluster {
					range: [c.start, c.start + u32::from(c.len)],
					command: c.command,
					row: u32::from(c.row_flags) >> row_shift,
					x: c.x,
					width: c.width,
					flags_spacing: u32::from(c.row_flags)
						& ((1 << row_shift) - 1),
				})
			}
		}
	}
	pub(super) fn storage_bytes(&self) -> usize {
		match self {
			Self::Wide(clusters) => size_of_val(&**clusters),
			Self::Narrow { clusters, .. } => size_of_val(&**clusters),
		}
	}
	#[cfg(feature = "fuzz")]
	pub(super) fn visit_storage(
		&self,
		visit: &mut crate::scene::StorageVisitor<'_>,
	) {
		use crate::scene::memory::allocation;
		match self {
			Self::Wide(clusters) => {
				allocation(clusters, clusters.len(), "clusters", visit)
			}
			Self::Narrow { clusters, .. } => {
				allocation(clusters, clusters.len(), "clusters", visit)
			}
		}
	}
}

#[cfg(test)]
mod tests;
