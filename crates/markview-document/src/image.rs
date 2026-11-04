//! Semantic image sources, before resource loading or layout.

/// The directory a relative resource address resolves against.
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum ResourceBase {
	/// The directory of the document that contains the resource.
	#[default]
	Document,
	/// The open folder's root, as an Obsidian vault embed expects.
	Vault,
}

/// One address together with the base it resolves against.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ResourceRef {
	pub address: String,
	pub base: ResourceBase,
}

impl ResourceRef {
	pub fn document(address: impl Into<String>) -> Self {
		Self {
			address: address.into(),
			base: ResourceBase::Document,
		}
	}

	pub fn vault(address: impl Into<String>) -> Self {
		Self {
			address: address.into(),
			base: ResourceBase::Vault,
		}
	}
}

/// A diagram language and its source, rendered by an application renderer.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Diagram {
	pub language: String,
	pub source: String,
}

/// What an image displays: a resource address or diagram source.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum ImageSource {
	Resource(ResourceRef),
	Diagram(Diagram),
}

impl ImageSource {
	pub fn resource(&self) -> Option<&ResourceRef> {
		match self {
			Self::Resource(resource) => Some(resource),
			Self::Diagram(_) => None,
		}
	}

	pub fn diagram(&self) -> Option<&Diagram> {
		match self {
			Self::Diagram(diagram) => Some(diagram),
			Self::Resource(_) => None,
		}
	}
}

/// One Markdown or HTML image as the document sees it, before any I/O.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ImageSpec {
	pub source: ImageSource,
	pub alt: String,
	pub title: String,
	/// Explicit `width` / `height` attributes; `None` keeps the aspect ratio.
	pub width: Option<u32>,
	pub height: Option<u32>,
}

impl ImageSpec {
	pub fn resource(&self) -> Option<&ResourceRef> {
		self.source.resource()
	}

	/// The resource address, or a diagram's source text.
	pub fn address(&self) -> &str {
		match &self.source {
			ImageSource::Resource(resource) => &resource.address,
			ImageSource::Diagram(diagram) => &diagram.source,
		}
	}
}
