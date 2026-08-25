//! Experimental converter interfaces.
//!
//! No candidate layout is blessed here. Experiments can implement
//! [`LayoutExperiment`] and report size/construction measurements consistently.

use panrange_format::RangeSource;
use std::io;
use std::path::Path;

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
