//! Experimental converter interfaces.
//!
//! No candidate layout is blessed here. Experiments can implement
//! [`LayoutExperiment`] and report size/construction measurements consistently.

use pangenome_range_format::RangeSource;
use std::io;
use std::path::Path;

mod disk_source;
mod experiment;
mod features;
mod fixed;
mod gbwt_locate;
mod local_subgraph;
mod path_membership;
mod scale;
mod source;
#[cfg(test)]
mod test_support;

pub use disk_source::{
    DiskGbzSource, DiskSourceStats, PersistentSourceCache, SourceCacheDiskPreflight,
    SourceCacheManifest, SourceCacheOpenMetrics, build_persistent_source_cache,
    inspect_persistent_source_cache, open_persistent_source_cache, prune_persistent_source_cache,
    source_cache_disk_preflight,
};
pub use experiment::{ExperimentMode, ExperimentOptions, run_fixed_window_experiment};
pub use fixed::{
    ArchiveBuildMetrics, ArchiveBuildOptions, ArchiveValidationSummary, BuildProgressMode,
    ChunkCodec, FixedArchiveConfig, FixedArchiveReader, OracleResult, QueryMeasurement, QuerySpec,
    build_fixed_archive_from_source_with_options, build_fixed_archive_with_options,
    export_conformance_fixtures, internal_gbz_base_query, query_fixed_archive, source_oracle,
    source_oracle_for_haplotype, validate_fixed_archive, validate_fixed_archive_with_options,
    validate_fixed_archive_with_progress,
};
pub use scale::{
    EncodeOptions, EncodeSourceMode, EncodeSummary, EncoderScaleOptions, run_encode,
    run_encoder_scale_experiment,
};
pub use source::{
    LoadedGbzSource, PangenomeSource, SourceLocatedPosition, SourceMemoryPreflight,
    SourcePathCatalogRecord, SourcePathIndex, SourceReference, SourceReferencePosition,
    SourceReferenceSeed, source_memory_preflight,
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
