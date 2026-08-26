//! Experimental converter interfaces.
//!
//! No candidate layout is blessed here. Experiments can implement
//! [`LayoutExperiment`] and report size/construction measurements consistently.

use pangenome_range_format::RangeSource;
use std::io;
use std::path::Path;

mod disk_source;
mod experiment;
mod fixed;
mod local_subgraph;
mod scale;
mod source;

pub use disk_source::{DiskGbzSource, DiskSourceStats};
pub use experiment::{ExperimentMode, ExperimentOptions, run_fixed_window_experiment};
pub use fixed::{
    ArchiveBuildMetrics, ArchiveBuildOptions, ArchiveValidationSummary, BuildProgressMode,
    ChunkCodec, FixedArchiveConfig, FixedArchiveReader, OracleResult, QueryMeasurement, QuerySpec,
    build_fixed_archive_from_source_with_options, build_fixed_archive_with_options,
    export_conformance_fixtures, internal_gbz_base_query, query_fixed_archive, source_oracle,
    validate_fixed_archive, validate_fixed_archive_with_options,
    validate_fixed_archive_with_progress,
};
pub use scale::{
    EncodeOptions, EncodeSourceMode, EncodeSummary, EncoderScaleOptions, run_encode,
    run_encoder_scale_experiment,
};
pub use source::{
    LoadedGbzSource, PangenomeSource, SourceMemoryPreflight, SourcePathIndex, SourceReference,
    SourceReferencePosition, SourceReferenceSeed, source_memory_preflight,
};

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
