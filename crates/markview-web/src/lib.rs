//! The browser front end: Markview driven from JavaScript through WebAssembly.
//!
//! Browser-independent state also builds as a native `rlib`, so the workspace
//! tests font registration and publication without any browser code in it.
//!
//! The JavaScript component contract is documented in `docs/mvaac.md`.

// The pointer and the publication bookkeeping are pure state: they name no
// browser type, so native tests cover them directly and `api.rs` stays the
// only module a browser has to host. Only the wasm front end constructs them.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod selection;
mod state;

#[cfg(target_arch = "wasm32")]
mod api;
mod fonts;
mod images;

#[cfg(target_arch = "wasm32")]
pub use api::{FontSet, Markview, configure_fonts, create};
