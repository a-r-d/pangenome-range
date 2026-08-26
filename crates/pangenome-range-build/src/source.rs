use gbz::bwt::{BWT, Record};
use gbz::support;
use gbz::{FullPathName, GBWT, GBZ, Orientation, Pos};
use gbz_base::{GBZPath, PathIndex};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceReference {
    pub name: FullPathName,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceReferencePosition {
    pub query_offset: usize,
    pub node_offset: usize,
    pub position: Pos,
    pub path_name: FullPathName,
}

/// Source-side operations required by the record-preserving encoder.
///
/// The loaded GBZ implementation remains the correctness baseline. This seam
/// deliberately describes metadata, sequence/record borrowing, and reference
/// lookup without prescribing full deserialization, mmap, or a row-per-visit
/// index.
pub trait PangenomeSource {
    fn access_mode(&self) -> &'static str;
    /// Discovers real reference fragments matching optional identity filters.
    ///
    /// # Errors
    ///
    /// Returns an error for missing metadata/sequences or coordinate overflow.
    fn references(
        &self,
        sample_filter: Option<&str>,
        contig_filter: Option<&str>,
    ) -> io::Result<Vec<SourceReference>>;
    fn sequence(&self, node_id: usize) -> Option<&[u8]>;
    fn record(&self, packed_handle: usize) -> Option<Record<'_>>;
    /// Resolves a reference coordinate to its packed GBWT occurrence.
    ///
    /// # Errors
    ///
    /// Returns an error when the path/index/sequence is missing, the query is
    /// outside its fragment, or reference traversal arithmetic fails.
    fn reference_position(&self, query: &FullPathName) -> io::Result<SourceReferencePosition>;
}

pub struct LoadedGbzSource<'a> {
    graph: &'a GBZ,
    path_index: Option<&'a PathIndex>,
}

impl<'a> LoadedGbzSource<'a> {
    #[must_use]
    pub const fn new(graph: &'a GBZ, path_index: Option<&'a PathIndex>) -> Self {
        Self { graph, path_index }
    }
}

impl PangenomeSource for LoadedGbzSource<'_> {
    fn access_mode(&self) -> &'static str {
        "fully-loaded-gbz"
    }

    fn references(
        &self,
        sample_filter: Option<&str>,
        contig_filter: Option<&str>,
    ) -> io::Result<Vec<SourceReference>> {
        let metadata = self
            .graph
            .metadata()
            .ok_or_else(|| invalid_data("GBZ metadata is required"))?;
        let reference_ids: BTreeSet<_> =
            self.graph.reference_sample_ids(true).into_iter().collect();
        let mut result = Vec::new();
        for (path_id, path_name) in metadata.path_iter().enumerate() {
            if !reference_ids.contains(&path_name.sample()) {
                continue;
            }
            let name = FullPathName::from_metadata(metadata, path_id)
                .ok_or_else(|| invalid_data(format!("missing metadata for path {path_id}")))?;
            if sample_filter.is_some_and(|sample| name.sample != sample)
                || contig_filter.is_some_and(|contig| name.contig != contig)
            {
                continue;
            }
            let length = self
                .graph
                .path(path_id, Orientation::Forward)
                .ok_or_else(|| invalid_data(format!("missing path {path_id}")))?
                .try_fold(0_u64, |length, (node_id, _)| {
                    let node_len = self.graph.sequence_len(node_id).ok_or_else(|| {
                        invalid_data(format!("missing sequence for node {node_id}"))
                    })?;
                    length
                        .checked_add(
                            u64::try_from(node_len)
                                .map_err(|_| invalid_data("sequence length does not fit in u64"))?,
                        )
                        .ok_or_else(|| invalid_data("reference path length overflow"))
                })?;
            let start = u64::try_from(name.fragment)
                .map_err(|_| invalid_data("reference fragment does not fit in u64"))?;
            let end = start
                .checked_add(length)
                .ok_or_else(|| invalid_data("reference coordinate overflow"))?;
            result.push(SourceReference { name, start, end });
        }
        Ok(result)
    }

    fn sequence(&self, node_id: usize) -> Option<&[u8]> {
        self.graph.sequence(node_id)
    }

    fn record(&self, packed_handle: usize) -> Option<Record<'_>> {
        let index: &GBWT = self.graph.as_ref();
        let records: &BWT = index.as_ref();
        records.record(index.node_to_record(packed_handle))
    }

    fn reference_position(&self, query: &FullPathName) -> io::Result<SourceReferencePosition> {
        let path_index = self
            .path_index
            .ok_or_else(|| invalid_data("reference lookup requires a path index"))?;
        let path = GBZPath::with_name(self.graph, query)
            .ok_or_else(|| invalid_data(format!("cannot find a path covering {query}")))?;
        let query_offset = query
            .fragment
            .checked_sub(path.name.fragment)
            .ok_or_else(|| invalid_data("query starts before its reference fragment"))?;
        let index_offset = path_index.path_to_offset(path.handle).ok_or_else(|| {
            invalid_data(format!("reference path {} is not indexed", path.name()))
        })?;
        let (mut path_offset, mut position) = path_index
            .indexed_position(index_offset, query_offset)
            .ok_or_else(|| {
                invalid_data(format!(
                    "reference path {} has no indexed position",
                    path.name()
                ))
            })?;
        loop {
            let node_id = support::node_id(position.node);
            let sequence_len = self.graph.sequence_len(node_id).ok_or_else(|| {
                invalid_data(format!("missing sequence for reference node {node_id}"))
            })?;
            let node_end = path_offset
                .checked_add(sequence_len)
                .ok_or_else(|| invalid_data("reference path offset overflow"))?;
            if node_end > query_offset {
                return Ok(SourceReferencePosition {
                    query_offset,
                    node_offset: query_offset - path_offset,
                    position,
                    path_name: path.name,
                });
            }
            path_offset = node_end;
            let record = self.record(position.node).ok_or_else(|| {
                invalid_data(format!("missing GBWT record for handle {}", position.node))
            })?;
            position = record.lf(position.offset).ok_or_else(|| {
                invalid_data(format!(
                    "reference path {} ended before offset {query_offset}",
                    path.name()
                ))
            })?;
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceMemoryPreflight {
    pub source_bytes: u64,
    pub recommended_available_bytes: u64,
    pub available_bytes: Option<u64>,
    pub sufficient: Option<bool>,
    pub estimate: &'static str,
}

/// Reports a conservative full-load memory preflight for the current adapter.
///
/// # Errors
///
/// Returns an error if the heuristic byte calculation overflows.
pub fn source_memory_preflight(source_bytes: u64) -> io::Result<SourceMemoryPreflight> {
    let recommended_available_bytes = source_bytes
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(512 * 1024 * 1024))
        .ok_or_else(|| invalid_data("source memory preflight estimate overflow"))?;
    let available_bytes = linux_available_memory_bytes();
    Ok(SourceMemoryPreflight {
        source_bytes,
        recommended_available_bytes,
        available_bytes,
        sufficient: available_bytes.map(|bytes| bytes >= recommended_available_bytes),
        estimate: "2x source bytes plus 512 MiB; conservative heuristic, not a bounded-access guarantee",
    })
}

fn linux_available_memory_bytes() -> Option<u64> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kib = contents.lines().find_map(|line| {
        let value = line.strip_prefix("MemAvailable:")?.trim();
        value.split_whitespace().next()?.parse::<u64>().ok()
    })?;
    kib.checked_mul(1024)
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_preflight_is_checked_and_explicitly_heuristic() {
        let report = source_memory_preflight(1024).unwrap();
        assert_eq!(report.recommended_available_bytes, 512 * 1024 * 1024 + 2048);
        assert!(report.estimate.contains("not a bounded-access guarantee"));
        assert!(source_memory_preflight(u64::MAX).is_err());
    }
}
