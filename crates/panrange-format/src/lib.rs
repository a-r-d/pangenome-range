//! Low-level primitives for pangenome-range format experiments.
//!
//! This crate deliberately does not define an archive format. It provides the
//! byte-source seam and instrumentation shared by competing layout experiments.

mod cost;
mod source;

pub use cost::{NetworkCost, NetworkProfile};
pub use source::{FileRangeSource, RangeRead, RangeSource, TraceSummary, TracingRangeSource};
