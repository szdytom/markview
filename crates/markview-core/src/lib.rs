//! Window-independent Markdown reading and layout.
pub mod document;
pub mod fonts;
mod highlight;
mod html;
pub mod image;
pub mod layout;
pub mod limits;
pub mod linebreak;
pub mod math;
mod microtype;
pub mod paginate;
pub mod profile;
pub mod scene;
pub mod shaping;
pub mod style;
pub mod text;

pub use microtype::JustificationLimits;
