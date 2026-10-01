//! Window-independent Markdown reading and layout.
pub mod background;
pub mod document;
pub mod fonts;
mod highlight;
#[cfg(feature = "fuzz")]
pub use highlight::prewarm_highlight;
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
pub mod search;
pub mod shaping;
pub mod source;
pub mod style;
pub mod sync;
pub mod text;
pub mod text_input;

pub use microtype::JustificationLimits;
