//! Shared helpers for the Markview fuzz targets: per-input budgets, a
//! counting allocator, structure-aware mutators, oracles, and pipeline
//! helpers. The targets themselves live in `../fuzz_targets`.
pub mod allocator;
pub mod budget;
pub mod edit;
pub mod mutators;
pub mod oracle;
pub mod parallel;
pub mod pdf_oracle;
pub mod pipeline;
pub mod probe;
pub mod seam;
