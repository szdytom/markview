//! Line breaking needs advances and cluster boundaries, without paint or ink.
use super::{Cluster, ClusterMetrics, Span, TextShaper};
use std::ops::{Deref, DerefMut};

impl Deref for Cluster {
	type Target = ClusterMetrics;
	fn deref(&self) -> &ClusterMetrics {
		&self.metrics
	}
}
impl DerefMut for Cluster {
	fn deref_mut(&mut self) -> &mut ClusterMetrics {
		&mut self.metrics
	}
}

impl AsRef<ClusterMetrics> for ClusterMetrics {
	fn as_ref(&self) -> &ClusterMetrics {
		self
	}
}
impl AsMut<ClusterMetrics> for ClusterMetrics {
	fn as_mut(&mut self) -> &mut ClusterMetrics {
		self
	}
}
impl AsRef<ClusterMetrics> for Cluster {
	fn as_ref(&self) -> &ClusterMetrics {
		&self.metrics
	}
}
impl AsMut<ClusterMetrics> for Cluster {
	fn as_mut(&mut self) -> &mut ClusterMetrics {
		&mut self.metrics
	}
}

impl TextShaper {
	pub(crate) fn measure(
		&mut self,
		text: &str,
		spans: &[Span],
		size: f32,
	) -> Vec<ClusterMetrics> {
		if text.is_empty() {
			return Vec::new();
		}
		let base = self.appearance.clone();
		let (mut layout, _, _) = self.build_layout(text, spans, size, &base);
		let mut clusters: Vec<ClusterMetrics> = Vec::new();
		for line in layout.lines() {
			for run in line.runs() {
				let start = clusters.len();
				let mut inked: Option<usize> = None;
				for c in run.clusters() {
					let range = c.text_range();
					if c.is_ligature_continuation() {
						if let Some(index) = inked {
							let first = &mut clusters[index];
							first.range.start =
								first.range.start.min(range.start);
							first.range.end = first.range.end.max(range.end);
							first.width += c.advance();
						}
						continue;
					}
					inked = Some(clusters.len());
					clusters.push(ClusterMetrics {
						range,
						width: c.advance(),
						mixed: (false, false),
						glyph_count: c.glyphs().count(),
					});
				}
				if run.is_rtl() {
					clusters[start..].reverse();
				}
			}
		}
		layout.clear();
		self.layout = layout;
		clusters
	}
}

#[cfg(test)]
mod tests;
