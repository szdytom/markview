//! Shared helpers for the Markview fuzz targets: per-input budgets, a
//! counting allocator, structure-aware mutators, oracles, and pipeline
//! helpers. The targets themselves live in `../fuzz_targets`.
pub mod allocator;
pub mod budget;
pub mod coverage;
pub mod edit;
pub mod font_archive;
pub mod highlight_schedule;
pub mod history;
pub mod image_decode;
pub mod interaction_sequence;
pub mod mutators;
pub mod oracle;
pub mod page_oracle;
pub mod paginate_pipeline;
pub mod parallel;
pub mod pdf_oracle;
pub mod pipeline;
pub mod probe;
pub mod reading;
pub mod reparse_sequence;
pub mod seam;
pub mod web_layout;
pub mod webgen;
pub mod webpage;
