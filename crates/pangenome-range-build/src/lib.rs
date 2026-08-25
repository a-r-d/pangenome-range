//! Experimental converter interfaces.
//!
//! No candidate layout is blessed here. Experiments can implement
//! [`LayoutExperiment`] and report size/construction measurements consistently.

use pangenome_range_format::RangeSource;
use std::io;
use std::path::Path;

mod experiment;
mod fixed;
mod scale;

pub use experiment::{ExperimentMode, ExperimentOptions, run_fixed_window_experiment};
pub use fixed::{
    ChunkCodec, FixedArchiveConfig, FixedArchiveReader, OracleResult, QueryMeasurement, QuerySpec,
    internal_gbz_base_query, query_fixed_archive,
};
pub use scale::{EncoderScaleOptions, run_encoder_scale_experiment};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildReport {
    pub experiment: String,
    pub source_bytes: u64,
    pub output_bytes: u64,
}

/// A named, replaceable candidate-layout experiment.
pub trait LayoutExperiment {
    fn name(&self) -> &str;

    /// Builds one candidate archive and reports its byte sizes.
    ///
    /// # Errors
    ///
    /// Returns an error if the source cannot be read, the destination cannot be
    /// written, or the candidate converter rejects the input.
    fn build(&self, source: &dyn RangeSource, destination: &Path) -> io::Result<BuildReport>;
}
