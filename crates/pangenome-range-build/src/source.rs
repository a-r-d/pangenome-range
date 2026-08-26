use gbz::bwt::{BWT, Record};
use gbz::support;
use gbz::{FullPathName, GBWT, GBZ, Orientation, Pos};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceReference {
    pub path_id: usize,
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

#[derive(Clone, Debug)]
pub struct SourceReferenceSeed {
    pub path_id: usize,
    pub name: FullPathName,
    pub position: Pos,
}

/// Source-side operations required by the record-preserving encoder.
///
/// Implementations may keep a GBZ in memory or serve these requests from a
/// bounded disk cache. Returned sequences and records are owned deliberately:
/// callers retain only the small active regional working set.
pub trait PangenomeSource: Sync {
    fn access_mode(&self) -> &'static str;
    /// Returns real-reference path names and their initial GBWT positions.
    ///
    /// # Errors
    ///
    /// Returns an error for missing or inconsistent source metadata.
    fn reference_seeds(&self) -> io::Result<Vec<SourceReferenceSeed>>;
    /// Returns the length of one forward node sequence without requiring the
    /// sequence body to be materialized.
    ///
    /// # Errors
    ///
    /// Returns an error for corrupt offsets or source I/O failures.
    fn sequence_len(&self, node_id: usize) -> io::Result<Option<usize>>;
    /// Copies one forward node sequence into the active regional working set.
    ///
    /// # Errors
    ///
    /// Returns an error for corrupt offsets or source I/O failures.
    fn sequence(&self, node_id: usize) -> io::Result<Option<Vec<u8>>>;
    /// Copies one exact packed GBWT record into the active working set.
    ///
    /// # Errors
    ///
    /// Returns an error for corrupt offsets or source I/O failures.
    fn packed_record(&self, packed_handle: usize) -> io::Result<Option<Vec<u8>>>;
}

pub struct LoadedGbzSource<'a> {
    graph: &'a GBZ,
}

impl<'a> LoadedGbzSource<'a> {
    #[must_use]
    pub const fn new(graph: &'a GBZ) -> Self {
        Self { graph }
    }
}

impl PangenomeSource for LoadedGbzSource<'_> {
    fn access_mode(&self) -> &'static str {
        "fully-loaded-gbz"
    }

    fn reference_seeds(&self) -> io::Result<Vec<SourceReferenceSeed>> {
        let metadata = self
            .graph
            .metadata()
            .ok_or_else(|| invalid_data("GBZ metadata is required"))?;
        let reference_ids: BTreeSet<_> =
            self.graph.reference_sample_ids(true).into_iter().collect();
        let index: &GBWT = self.graph.as_ref();
        let mut result = Vec::new();
        for (path_id, path_name) in metadata.path_iter().enumerate() {
            if !reference_ids.contains(&path_name.sample()) {
                continue;
            }
            let name = FullPathName::from_metadata(metadata, path_id)
                .ok_or_else(|| invalid_data(format!("missing metadata for path {path_id}")))?;
            let sequence_id = support::encode_path(path_id, Orientation::Forward);
            let position = index
                .start(sequence_id)
                .ok_or_else(|| invalid_data(format!("reference path {path_id} is empty")))?;
            result.push(SourceReferenceSeed {
                path_id,
                name,
                position,
            });
        }
        Ok(result)
    }

    fn sequence(&self, node_id: usize) -> io::Result<Option<Vec<u8>>> {
        Ok(self.graph.sequence(node_id).map(<[u8]>::to_vec))
    }

    fn sequence_len(&self, node_id: usize) -> io::Result<Option<usize>> {
        Ok(self.graph.sequence(node_id).map(<[u8]>::len))
    }

    fn packed_record(&self, packed_handle: usize) -> io::Result<Option<Vec<u8>>> {
        let index: &GBWT = self.graph.as_ref();
        if !index.has_node(packed_handle) {
            return Ok(None);
        }
        let records: &BWT = index.as_ref();
        let Some((edges, bwt)) = records.compressed_record(index.node_to_record(packed_handle))
        else {
            return Ok(None);
        };
        let mut result = Vec::with_capacity(edges.len() + bwt.len());
        result.extend_from_slice(edges);
        result.extend_from_slice(bwt);
        Ok(Some(result))
    }
}

#[derive(Clone, Debug)]
struct IndexedReference {
    reference: SourceReference,
    positions: Vec<(usize, Pos)>,
}

/// Project-owned sparse coordinate index for real reference paths.
///
/// This contains one sample roughly every `interval` reference bases, never
/// one row per global haplotype visit.
#[derive(Clone, Debug)]
pub struct SourcePathIndex {
    paths: Vec<IndexedReference>,
}

impl SourcePathIndex {
    /// Builds a compact reference-only coordinate index.
    ///
    /// # Errors
    ///
    /// Returns an error for missing records/sequences, empty references, or
    /// coordinate arithmetic overflow.
    pub fn new(source: &dyn PangenomeSource, interval: usize) -> io::Result<Self> {
        if interval == 0 {
            return Err(invalid_data("reference index interval must be nonzero"));
        }
        let mut paths = Vec::new();
        for seed in source.reference_seeds()? {
            let mut positions = Vec::new();
            let mut path_offset = 0_usize;
            let mut next_sample = 0_usize;
            let mut position = Some(seed.position);
            while let Some(current) = position {
                if path_offset >= next_sample {
                    positions.push((path_offset, current));
                    next_sample = path_offset
                        .checked_add(interval)
                        .ok_or_else(|| invalid_data("reference sample offset overflow"))?;
                }
                let node_id = support::node_id(current.node);
                let sequence_len = source
                    .sequence_len(node_id)?
                    .ok_or_else(|| invalid_data(format!("missing sequence for node {node_id}")))?;
                path_offset = path_offset
                    .checked_add(sequence_len)
                    .ok_or_else(|| invalid_data("reference path length overflow"))?;
                let bytes = source.packed_record(current.node)?.ok_or_else(|| {
                    invalid_data(format!("missing GBWT record for handle {}", current.node))
                })?;
                let record = Record::new(0, &bytes).ok_or_else(|| {
                    invalid_data(format!("empty GBWT record for handle {}", current.node))
                })?;
                position = record.lf(current.offset);
            }
            if positions.is_empty() {
                return Err(invalid_data(format!(
                    "reference path {} is empty",
                    seed.name
                )));
            }
            let start = u64::try_from(seed.name.fragment)
                .map_err(|_| invalid_data("reference fragment does not fit in u64"))?;
            let end = start
                .checked_add(
                    u64::try_from(path_offset)
                        .map_err(|_| invalid_data("reference path length does not fit in u64"))?,
                )
                .ok_or_else(|| invalid_data("reference coordinate overflow"))?;
            paths.push(IndexedReference {
                reference: SourceReference {
                    path_id: seed.path_id,
                    name: seed.name,
                    start,
                    end,
                },
                positions,
            });
        }
        if paths.is_empty() {
            return Err(invalid_data("GBZ has no reference paths"));
        }
        Ok(Self { paths })
    }

    #[must_use]
    pub fn references(
        &self,
        sample_filter: Option<&str>,
        contig_filter: Option<&str>,
    ) -> Vec<SourceReference> {
        self.paths
            .iter()
            .filter(|path| {
                sample_filter.is_none_or(|sample| path.reference.name.sample == sample)
                    && contig_filter.is_none_or(|contig| path.reference.name.contig == contig)
            })
            .map(|path| path.reference.clone())
            .collect()
    }

    /// Resolves a haplotype coordinate to the containing reference fragment.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is absent, the coordinate is outside the
    /// fragment, or packed record navigation fails.
    pub fn reference_position(
        &self,
        source: &dyn PangenomeSource,
        query: &FullPathName,
    ) -> io::Result<SourceReferencePosition> {
        let path = self
            .paths
            .iter()
            .filter(|path| {
                path.reference.name.sample == query.sample
                    && path.reference.name.contig == query.contig
                    && path.reference.name.haplotype == query.haplotype
                    && path.reference.name.fragment <= query.fragment
            })
            .max_by_key(|path| path.reference.name.fragment)
            .ok_or_else(|| invalid_data(format!("cannot find a path covering {query}")))?;
        let query_offset = query
            .fragment
            .checked_sub(path.reference.name.fragment)
            .ok_or_else(|| invalid_data("query starts before its reference fragment"))?;
        let sample = path
            .positions
            .partition_point(|(offset, _)| *offset <= query_offset)
            .checked_sub(1)
            .and_then(|index| path.positions.get(index))
            .copied()
            .ok_or_else(|| invalid_data(format!("reference path {query} has no index sample")))?;
        let (mut path_offset, mut position) = sample;
        loop {
            let node_id = support::node_id(position.node);
            let sequence_len = source
                .sequence_len(node_id)?
                .ok_or_else(|| invalid_data(format!("missing sequence for node {node_id}")))?;
            let node_end = path_offset
                .checked_add(sequence_len)
                .ok_or_else(|| invalid_data("reference path offset overflow"))?;
            if node_end > query_offset {
                return Ok(SourceReferencePosition {
                    query_offset,
                    node_offset: query_offset - path_offset,
                    position,
                    path_name: path.reference.name.clone(),
                });
            }
            path_offset = node_end;
            let bytes = source.packed_record(position.node)?.ok_or_else(|| {
                invalid_data(format!("missing GBWT record for handle {}", position.node))
            })?;
            let record = Record::new(0, &bytes).ok_or_else(|| {
                invalid_data(format!("empty GBWT record for handle {}", position.node))
            })?;
            position = record.lf(position.offset).ok_or_else(|| {
                invalid_data(format!(
                    "reference path {} ended before offset {query_offset}",
                    path.reference.name
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
