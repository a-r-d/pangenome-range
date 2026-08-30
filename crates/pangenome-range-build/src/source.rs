use gbz::bwt::{BWT, Record};
use gbz::support;
use gbz::{FullPathName, GBWT, GBZ, Orientation, Pos};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{self, Cursor, Read};

const SOURCE_PATH_INDEX_MAGIC: &[u8; 8] = b"PNGSPX01";
const SOURCE_PATH_INDEX_VERSION: u32 = 1;
const MAX_SOURCE_PATHS: usize = 1_000_000;
const MAX_SOURCE_PATH_SAMPLES: usize = 100_000_000;
const MAX_SOURCE_PATH_STRING_BYTES: usize = 1024 * 1024;

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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePathCatalogRecord {
    pub path_id: u64,
    #[serde(rename = "rawName")]
    pub canonical_name: String,
    pub sample: String,
    pub contig: String,
    pub haplotype: u64,
    pub fragment: u64,
    pub sense: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLocatedPosition {
    pub sequence_id: u64,
    pub path_id: u64,
    pub reversed: bool,
    pub lf_steps: u64,
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

    /// Returns the canonical path-name catalog when source identity support is available.
    ///
    /// # Errors
    ///
    /// Returns an error when source metadata is corrupt or cannot be read.
    fn path_catalog(&self) -> io::Result<Option<&[SourcePathCatalogRecord]>> {
        Ok(None)
    }

    /// Locates a bounded batch of GBWT positions to their source sequence IDs.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid positions, missing/corrupt DA support, source I/O
    /// failure, or when a position exceeds the LF-step limit.
    fn locate_positions(
        &self,
        _positions: &[Pos],
        _max_lf_steps: usize,
    ) -> io::Result<Option<Vec<SourceLocatedPosition>>> {
        Ok(None)
    }
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
    interval: usize,
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
        Ok(Self { interval, paths })
    }

    #[must_use]
    pub const fn interval(&self) -> usize {
        self.interval
    }

    #[must_use]
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    #[must_use]
    pub fn sample_count(&self) -> usize {
        self.paths.iter().map(|path| path.positions.len()).sum()
    }

    pub(crate) fn reference_seeds(&self) -> io::Result<Vec<SourceReferenceSeed>> {
        self.paths
            .iter()
            .map(|path| {
                Ok(SourceReferenceSeed {
                    path_id: path.reference.path_id,
                    name: path.reference.name.clone(),
                    position: path
                        .positions
                        .first()
                        .map(|(_, position)| *position)
                        .ok_or_else(|| invalid_data("cached reference has no path samples"))?,
                })
            })
            .collect()
    }

    /// Encodes the sparse real-reference coordinate index for a persistent source cache.
    ///
    /// # Errors
    ///
    /// Returns an error for non-representable lengths or arithmetic overflow.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        output.extend_from_slice(SOURCE_PATH_INDEX_MAGIC);
        output.extend_from_slice(&SOURCE_PATH_INDEX_VERSION.to_le_bytes());
        output.extend_from_slice(&0_u32.to_le_bytes());
        put_index_u64(&mut output, self.interval)?;
        put_index_u64(&mut output, self.paths.len())?;
        for path in &self.paths {
            put_index_u64(&mut output, path.reference.path_id)?;
            put_index_string(&mut output, &path.reference.name.sample)?;
            put_index_string(&mut output, &path.reference.name.contig)?;
            put_index_u64(&mut output, path.reference.name.haplotype)?;
            put_index_u64(&mut output, path.reference.name.fragment)?;
            output.extend_from_slice(&path.reference.start.to_le_bytes());
            output.extend_from_slice(&path.reference.end.to_le_bytes());
            put_index_u64(&mut output, path.positions.len())?;
            for (offset, position) in &path.positions {
                put_index_u64(&mut output, *offset)?;
                put_index_u64(&mut output, position.node)?;
                put_index_u64(&mut output, position.offset)?;
            }
        }
        Ok(output)
    }

    /// Decodes and validates a persistent sparse real-reference coordinate index.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed, excessive, trailing, or noncanonical data.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut reader = Cursor::new(bytes);
        let mut magic = [0_u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != SOURCE_PATH_INDEX_MAGIC
            || read_index_u32(&mut reader)? != SOURCE_PATH_INDEX_VERSION
            || read_index_u32(&mut reader)? != 0
        {
            return Err(invalid_data("invalid source path index header"));
        }
        let interval = read_index_usize(&mut reader, "source path interval")?;
        let path_count = read_index_usize(&mut reader, "source path count")?;
        if interval == 0 || path_count == 0 || path_count > MAX_SOURCE_PATHS {
            return Err(invalid_data("invalid source path index dimensions"));
        }
        let mut paths = Vec::with_capacity(path_count);
        let mut total_samples = 0_usize;
        for _ in 0..path_count {
            let path_id = read_index_usize(&mut reader, "source path id")?;
            let sample = read_index_string(&mut reader)?;
            let contig = read_index_string(&mut reader)?;
            let haplotype = read_index_usize(&mut reader, "source path haplotype")?;
            let fragment = read_index_usize(&mut reader, "source path fragment")?;
            let start = read_index_u64(&mut reader)?;
            let end = read_index_u64(&mut reader)?;
            let position_count = read_index_usize(&mut reader, "source path sample count")?;
            total_samples = total_samples
                .checked_add(position_count)
                .ok_or_else(|| invalid_data("source path sample count overflow"))?;
            if sample.is_empty()
                || contig.is_empty()
                || start >= end
                || position_count == 0
                || total_samples > MAX_SOURCE_PATH_SAMPLES
            {
                return Err(invalid_data("invalid cached source reference"));
            }
            let mut positions = Vec::with_capacity(position_count);
            let mut previous_offset = None;
            for _ in 0..position_count {
                let offset = read_index_usize(&mut reader, "source path sample offset")?;
                let node = read_index_usize(&mut reader, "source path sample node")?;
                let record_offset = read_index_usize(&mut reader, "source path record offset")?;
                if node == 0 || previous_offset.is_some_and(|previous| offset <= previous) {
                    return Err(invalid_data("noncanonical cached source path samples"));
                }
                previous_offset = Some(offset);
                positions.push((offset, Pos::new(node, record_offset)));
            }
            paths.push(IndexedReference {
                reference: SourceReference {
                    path_id,
                    name: FullPathName {
                        sample,
                        contig,
                        haplotype,
                        fragment,
                    },
                    start,
                    end,
                },
                positions,
            });
        }
        if usize::try_from(reader.position()).ok() != Some(bytes.len()) {
            return Err(invalid_data("trailing source path index bytes"));
        }
        Ok(Self { interval, paths })
    }

    /// Returns the stable digest of the real-reference identity and coordinate metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the metadata cannot be represented.
    pub fn reference_metadata_sha256(&self) -> io::Result<[u8; 32]> {
        let mut digest = Sha256::new();
        for path in &self.paths {
            digest.update(
                u64::try_from(path.reference.path_id)
                    .map_err(|_| invalid_data("source reference path id does not fit u64"))?
                    .to_le_bytes(),
            );
            digest.update(
                u64::try_from(path.reference.name.sample.len())
                    .map_err(|_| invalid_data("source reference sample length does not fit u64"))?
                    .to_le_bytes(),
            );
            digest.update(path.reference.name.sample.as_bytes());
            digest.update(
                u64::try_from(path.reference.name.contig.len())
                    .map_err(|_| invalid_data("source reference contig length does not fit u64"))?
                    .to_le_bytes(),
            );
            digest.update(path.reference.name.contig.as_bytes());
            digest.update(path.reference.start.to_le_bytes());
            digest.update(path.reference.end.to_le_bytes());
        }
        Ok(digest.finalize().into())
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

fn put_index_u64(output: &mut Vec<u8>, value: usize) -> io::Result<()> {
    output.extend_from_slice(
        &u64::try_from(value)
            .map_err(|_| invalid_data("source index value does not fit u64"))?
            .to_le_bytes(),
    );
    Ok(())
}

fn put_index_string(output: &mut Vec<u8>, value: &str) -> io::Result<()> {
    if value.len() > MAX_SOURCE_PATH_STRING_BYTES {
        return Err(invalid_data("source path string exceeds its limit"));
    }
    put_index_u64(output, value.len())?;
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn read_index_u32(reader: &mut Cursor<&[u8]>) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_index_u64(reader: &mut Cursor<&[u8]>) -> io::Result<u64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_index_usize(reader: &mut Cursor<&[u8]>, label: &str) -> io::Result<usize> {
    usize::try_from(read_index_u64(reader)?)
        .map_err(|_| invalid_data(format!("{label} does not fit usize")))
}

fn read_index_string(reader: &mut Cursor<&[u8]>) -> io::Result<String> {
    let length = read_index_usize(reader, "source path string length")?;
    if length > MAX_SOURCE_PATH_STRING_BYTES {
        return Err(invalid_data("source path string exceeds its limit"));
    }
    let mut bytes = vec![0_u8; length];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid_data("source path string is not UTF-8"))
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

    #[test]
    fn sparse_source_path_index_round_trips_canonically() {
        let index = SourcePathIndex {
            interval: 1_000,
            paths: vec![IndexedReference {
                reference: SourceReference {
                    path_id: 7,
                    name: FullPathName {
                        sample: "GRCh38".into(),
                        contig: "chr1".into(),
                        haplotype: 0,
                        fragment: 100,
                    },
                    start: 100,
                    end: 2_100,
                },
                positions: vec![(0, Pos::new(4, 2)), (1_000, Pos::new(8, 3))],
            }],
        };
        let bytes = index.encode().unwrap();
        let decoded = SourcePathIndex::decode(&bytes).unwrap();
        assert_eq!(decoded.interval(), 1_000);
        assert_eq!(decoded.references(None, None), index.references(None, None));
        assert_eq!(decoded.encode().unwrap(), bytes);
        assert_eq!(
            decoded.reference_metadata_sha256().unwrap(),
            index.reference_metadata_sha256().unwrap()
        );
        assert!(SourcePathIndex::decode(&bytes[..bytes.len() - 1]).is_err());
    }
}
