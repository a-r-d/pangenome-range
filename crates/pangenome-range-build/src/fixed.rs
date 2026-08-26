use gbz::bwt::{BWT, Record};
use gbz::support;
use gbz::{FullPathName, GBWT, GBZ, Orientation, Pos};
use gbz_base::{GBZPath, HaplotypeOutput, PathIndex, Subgraph, SubgraphQuery};
use pangenome_range_format::{FileRangeSource, NetworkProfile, RangeSource, TracingRangeSource};
use pangenome_range_query::{
    CanonicalHaplotypeTile, CanonicalPath, CanonicalSubgraph, Edge, HaplotypeSemantics,
    OrientedNode, ReferenceInterval, WeightedTraversal,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, VecDeque};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub type ExperimentResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const ARCHIVE_MAGIC_V3: &[u8; 8] = b"PNGRNG03";
const ARCHIVE_MAGIC: &[u8; 8] = b"PNGRNG04";
const REGION_MAGIC_NAMED_V2: &[u8; 8] = b"PNGRGN02";
const REGION_MAGIC_WEIGHTED_V3: &[u8; 8] = b"PNGRGN03";
const REGION_MAGIC: &[u8; 8] = b"PNGRGN04";
const ARCHIVE_VERSION: u32 = 4;
const REGION_VERSION: u32 = 4;
const HEADER_LEN: usize = 64;
const DIRECTORY_PAGE_BYTES: usize = 4 * 1024;
const DIRECTORY_PAGE_HEADER_BYTES: usize = 16;
const DIRECTORY_ENTRY_BYTES: usize = 5 * std::mem::size_of::<u64>();
const DIRECTORY_ENTRIES_PER_PAGE: usize =
    (DIRECTORY_PAGE_BYTES - DIRECTORY_PAGE_HEADER_BYTES) / DIRECTORY_ENTRY_BYTES;
const DIRECTORY_BUCKET_WINDOWS: u64 = 32;
pub const BOOTSTRAP_LEN: usize = 16 * 1024;
pub const CONSTRUCTION_CONTEXT: u64 = 100;
pub const DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_MIN_WINDOW_SIZE: u64 = 1024;
pub const DEFAULT_MAX_QUEUED_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_PROGRESS_INTERVAL_MS: u64 = 5_000;
const MAX_DECODED_OCCURRENCES_PER_TILE: u64 = 16 * 1024 * 1024;
const DEFAULT_DIRECTORY_CACHE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChunkCodec {
    None,
    Zstd1,
    Zstd3,
    Zstd6,
}

impl ChunkCodec {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Zstd1 => "zstd-1",
            Self::Zstd3 => "zstd-3",
            Self::Zstd6 => "zstd-6",
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Zstd1 => 1,
            Self::Zstd3 => 3,
            Self::Zstd6 => 6,
        }
    }

    fn from_code(code: u8) -> io::Result<Self> {
        match code {
            0 => Ok(Self::None),
            1 => Ok(Self::Zstd1),
            3 => Ok(Self::Zstd3),
            6 => Ok(Self::Zstd6),
            _ => Err(invalid_data(format!("unknown chunk codec {code}"))),
        }
    }

    fn level(self) -> Option<i32> {
        match self {
            Self::None => None,
            Self::Zstd1 => Some(1),
            Self::Zstd3 => Some(3),
            Self::Zstd6 => Some(6),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedArchiveConfig {
    pub experiment_id: String,
    pub window_size: u64,
    pub codec: ChunkCodec,
    pub deduplicate_chunks: bool,
    pub max_uncompressed_chunk_bytes: u64,
    pub min_window_size: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildProgressMode {
    #[default]
    Off,
    Plain,
    Json,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveBuildOptions {
    pub sample: Option<String>,
    pub contig: Option<String>,
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub max_chunks: Option<u64>,
    pub threads: usize,
    pub max_queued_bytes: u64,
    pub keep_partial: bool,
    pub progress: BuildProgressMode,
    pub progress_interval_ms: u64,
}

impl Default for ArchiveBuildOptions {
    fn default() -> Self {
        Self {
            sample: None,
            contig: None,
            start: None,
            end: None,
            max_chunks: None,
            threads: 1,
            max_queued_bytes: DEFAULT_MAX_QUEUED_BYTES,
            keep_partial: false,
            progress: BuildProgressMode::Off,
            progress_interval_ms: DEFAULT_PROGRESS_INTERVAL_MS,
        }
    }
}

fn emit_progress(mode: BuildProgressMode, phase: &str, message: &str) {
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!("{phase}: {message}"),
        BuildProgressMode::Json => eprintln!(
            "{}",
            serde_json::json!({ "phase": phase, "message": message })
        ),
    }
}

#[derive(Clone, Debug, Serialize)]
struct BuildProgressSnapshot {
    phase: &'static str,
    sequence: u64,
    reference_ordinal: u64,
    reference_count: u64,
    references_completed: u64,
    sample: String,
    contig: String,
    current_chunk_start: u64,
    current_chunk_end: u64,
    reference_start: u64,
    reference_end: u64,
    reference_percent_complete: f64,
    processed_reference_bases: u64,
    total_reference_bases: u64,
    percent_complete: f64,
    accepted_chunks: u64,
    physical_chunks: u64,
    reference_bp_per_second: f64,
    chunks_per_second: f64,
    estimated_seconds_remaining: Option<f64>,
    processing_elapsed_seconds: f64,
    build_elapsed_seconds: f64,
    temporary_archive_bytes: u64,
}

fn emit_progress_snapshot(mode: BuildProgressMode, snapshot: &BuildProgressSnapshot) {
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!(
            "chunk_progress: {:.3}% ref={}/{} {}#{}:{}-{} ref={:.2}% bases={}/{} chunks={} physical={} bp/s={:.0} chunks/s={:.2} eta_s={} elapsed_s={:.1} temp_bytes={}",
            snapshot.percent_complete,
            snapshot.reference_ordinal,
            snapshot.reference_count,
            snapshot.sample,
            snapshot.contig,
            snapshot.current_chunk_start,
            snapshot.current_chunk_end,
            snapshot.reference_percent_complete,
            snapshot.processed_reference_bases,
            snapshot.total_reference_bases,
            snapshot.accepted_chunks,
            snapshot.physical_chunks,
            snapshot.reference_bp_per_second,
            snapshot.chunks_per_second,
            snapshot
                .estimated_seconds_remaining
                .map_or_else(|| "unknown".into(), |seconds| format!("{seconds:.0}")),
            snapshot.processing_elapsed_seconds,
            snapshot.temporary_archive_bytes,
        ),
        BuildProgressMode::Json => eprintln!(
            "{}",
            serde_json::to_string(snapshot).expect("progress snapshot is JSON-serializable")
        ),
    }
}

fn emit_encoding_plan(
    mode: BuildProgressMode,
    reference_count: u64,
    total_reference_bases: u64,
    planned_base_windows: u64,
    temporary_archive_prefix_bytes: u64,
    progress_interval_ms: u64,
) {
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!(
            "encoding_plan: references={reference_count} total_bases={total_reference_bases} base_windows={planned_base_windows} temp_prefix_bytes={temporary_archive_prefix_bytes} progress_interval_ms={progress_interval_ms}"
        ),
        BuildProgressMode::Json => eprintln!(
            "{}",
            serde_json::json!({
                "phase": "encoding_plan",
                "reference_count": reference_count,
                "total_reference_bases": total_reference_bases,
                "planned_base_windows": planned_base_windows,
                "temporary_archive_prefix_bytes": temporary_archive_prefix_bytes,
                "progress_interval_ms": progress_interval_ms,
            })
        ),
    }
}

fn emit_reference_start(
    mode: BuildProgressMode,
    reference_ordinal: u64,
    reference_count: u64,
    reference: &ReferencePathSpec,
) {
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!(
            "reference_start: ref={reference_ordinal}/{reference_count} {}#{} {}-{}",
            reference.name.sample, reference.name.contig, reference.start, reference.end
        ),
        BuildProgressMode::Json => eprintln!(
            "{}",
            serde_json::json!({
                "phase": "reference_start",
                "reference_ordinal": reference_ordinal,
                "reference_count": reference_count,
                "sample": reference.name.sample,
                "contig": reference.name.contig,
                "reference_start": reference.start,
                "reference_end": reference.end,
            })
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuerySpec {
    pub id: String,
    pub class: String,
    pub sample: String,
    pub contig: String,
    pub start: u64,
    pub end: u64,
    pub context: u64,
}

impl QuerySpec {
    #[must_use]
    pub fn length(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    fn validate(&self) -> io::Result<()> {
        if self.start >= self.end {
            return Err(invalid_input(format!(
                "query {} has an empty or reversed interval",
                self.id
            )));
        }
        if self.context > CONSTRUCTION_CONTEXT {
            return Err(invalid_input(format!(
                "query context {} exceeds construction halo {CONSTRUCTION_CONTEXT}",
                self.context
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ArchiveBuildMetrics {
    pub experiment_id: String,
    pub source_gbz_bytes: u64,
    pub archive_bytes: u64,
    pub expansion_ratio: f64,
    pub index_bytes: u64,
    pub index_ratio: f64,
    pub root_index_bytes: u64,
    pub directory_pages: u64,
    pub directory_entries: u64,
    pub physical_chunks: u64,
    pub deduplicated_entries: u64,
    pub duplicate_payload_entries_observed: u64,
    pub avoidable_compressed_payload_bytes: u64,
    pub mean_chunk_bytes: f64,
    pub median_chunk_bytes: u64,
    pub p95_chunk_bytes: u64,
    pub max_chunk_bytes: u64,
    pub max_uncompressed_chunk_bytes: u64,
    pub peak_raw_chunk_bytes: u64,
    pub peak_compressed_chunk_bytes: u64,
    pub payload_spool_bytes: u64,
    pub path_occurrence_index_bytes: u64,
    pub path_occurrence_index_wall_ms: f64,
    pub reference_manifest_discovery_wall_ms: f64,
    #[serde(rename = "subgraph_selection_worker_ms")]
    pub subgraph_selection_wall_ms: f64,
    pub local_haplotype_extraction_wall_ms: f64,
    #[serde(rename = "regional_materialization_worker_ms")]
    pub regional_materialization_wall_ms: f64,
    #[serde(rename = "regional_encoding_worker_ms")]
    pub regional_encoding_wall_ms: f64,
    pub payload_pipeline_wall_ms: f64,
    pub compression_wall_ms: f64,
    pub preflight_selection_wall_ms: f64,
    pub writer_finalization_wall_ms: f64,
    pub archive_validation_wall_ms: f64,
    pub final_copy_wall_ms: f64,
    pub adaptive_splits: u64,
    pub preflight_splits: u64,
    pub post_materialization_splits: u64,
    pub largest_rejected_parent_bytes: u64,
    pub first_payload_wall_ms: f64,
    pub references_processed: u64,
    pub total_reference_bases: u64,
    pub reference_bases_processed: u64,
    pub progress_events_emitted: u64,
    pub scratch_bytes: u64,
    pub scratch_bytes_before_first_payload: u64,
    pub temporary_file_bytes_before_first_payload: u64,
    pub temporary_file_peak_bytes: u64,
    pub payload_bytes: u64,
    pub peak_queued_raw_bytes: u64,
    pub peak_queued_compressed_bytes: u64,
    pub peak_queued_total_bytes: u64,
    pub peak_ready_raw_bytes: u64,
    pub haplotype_extraction_evidence: Option<HaplotypeExtractionEvidence>,
    pub construction_wall_ms: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HaplotypeModeEvidence {
    pub mode: String,
    pub emitted_traversals: u64,
    pub total_weight: u64,
    pub emitted_traversal_nodes: u64,
    pub weighted_traversal_nodes: u64,
    pub extraction_wall_ms: f64,
    pub raw_json_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HaplotypeExtractionEvidence {
    pub reference_sample: String,
    pub reference_contig: String,
    pub core_start: u64,
    pub core_end: u64,
    pub all: HaplotypeModeEvidence,
    pub distinct: HaplotypeModeEvidence,
    pub exact_multiset_equivalent: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct QueryMeasurement {
    pub experiment_id: String,
    pub query_id: String,
    pub query_class: String,
    pub query_size: u64,
    pub coalescing_gap: u64,
    pub physical_reads: u64,
    pub mergeable_reads: u64,
    pub dependency_rounds: u64,
    pub total_bytes_fetched: u64,
    pub unique_bytes_fetched: u64,
    pub duplicate_bytes_fetched: u64,
    pub bootstrap_bytes_fetched: u64,
    pub logical_index_bytes: u64,
    pub directory_page_bytes_fetched: u64,
    pub directory_pages_selected: u64,
    pub directory_page_cache_hits: u64,
    pub data_bytes_fetched: u64,
    pub required_compressed_payload_bytes: u64,
    pub canonical_payload_bytes: u64,
    pub read_amplification: f64,
    pub canonical_amplification: f64,
    pub index_lookup_us: f64,
    pub decompression_us: f64,
    pub decode_us: f64,
    pub graph_reconstruction_us: f64,
    pub total_local_query_us: f64,
    pub selected_chunks: u64,
    pub selected_nodes: u64,
    pub canonical_hash: String,
    pub correctness: bool,
    pub haplotype_tiles_checked: u64,
    pub haplotype_tiles_correct: bool,
    pub simulated_20ms_ms: f64,
    pub simulated_50ms_ms: f64,
    pub simulated_100ms_ms: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArchiveValidationSummary {
    pub schema_version: u32,
    pub archive_version: u32,
    pub archive_path: PathBuf,
    pub archive_bytes: u64,
    pub reference_manifests: u64,
    pub directory_pages: u64,
    pub directory_entries: u64,
    pub physical_payloads: u64,
    pub compressed_payload_bytes: u64,
    pub uncompressed_payload_bytes: u64,
    pub validation_wall_ms: f64,
}

#[derive(Clone, Debug)]
pub struct OracleResult {
    pub canonical: CanonicalSubgraph,
    pub encoded: Vec<u8>,
}

#[derive(Clone, Debug)]
struct ArchiveEntry {
    start: u64,
    end: u64,
    offset: u64,
    compressed_len: u64,
    uncompressed_len: u64,
    codec: ChunkCodec,
}

#[derive(Clone, Debug)]
struct ArchiveIndex {
    entries: Vec<ArchiveEntry>,
}

#[derive(Clone, Debug)]
struct ReferenceManifest {
    sample: String,
    contig: String,
    start: u64,
    end: u64,
    grid_start: u64,
    window_size: u64,
    bucket_span: u64,
    first_page_offset: u64,
    page_count: u64,
    entry_count: u64,
    codec: ChunkCodec,
}

#[derive(Clone, Debug)]
struct RootIndex {
    logical_bytes: u64,
    manifests: Vec<ReferenceManifest>,
}

#[derive(Clone, Debug)]
struct DirectoryLookup {
    entries: Vec<ArchiveEntry>,
    logical_bytes: u64,
    fetched_bytes: u64,
    fetched_ranges: u64,
    selected_pages: u64,
    cache_hits: u64,
}

#[derive(Clone, Debug)]
struct RegionalPath {
    path_id: u64,
    sample: String,
    contig: String,
    haplotype: u64,
    fragment: u64,
    is_reference: bool,
    visits: BTreeMap<u64, OrientedNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RegionalReferencePath {
    sample: String,
    contig: String,
    haplotype: u64,
    start: u64,
    end: u64,
    traversal: Vec<OrientedNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReferenceVisit {
    path_id: u64,
    visit_index: u64,
    start: u64,
    end: u64,
    node: OrientedNode,
}

#[derive(Clone, Debug)]
struct RegionalGraph {
    nodes: BTreeMap<u64, Vec<u8>>,
    edges: BTreeSet<Edge>,
    semantics: HaplotypeSemantics,
    reference_paths: Vec<RegionalReferencePath>,
    haplotype_tiles: Vec<CanonicalHaplotypeTile>,
    // Populated only when decoding retained v3 payloads.
    paths: BTreeMap<u64, RegionalPath>,
    reference_visits: BTreeSet<ReferenceVisit>,
}

impl Default for RegionalGraph {
    fn default() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeSet::new(),
            semantics: HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
            reference_paths: Vec::new(),
            haplotype_tiles: Vec::new(),
            paths: BTreeMap::new(),
            reference_visits: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct StoredChunk {
    archive_offset: u64,
    compressed_len: u64,
    uncompressed_len: u64,
}

#[derive(Debug)]
struct PendingRawChunk {
    reference_id: usize,
    start: u64,
    end: u64,
    raw: Vec<u8>,
    hash: blake3::Hash,
}

#[derive(Default)]
struct DirectWriterState {
    chunks: Vec<StoredChunk>,
    chunk_by_hash: HashMap<blake3::Hash, Vec<usize>>,
    duplicate_payload_entries_observed: u64,
    avoidable_compressed_payload_bytes: u64,
    peak_compressed_chunk_bytes: u64,
    compression_wall_ms: f64,
    first_payload_wall_ms: Option<f64>,
    accepted_chunks: u64,
    reference_bases_processed: u64,
    peak_queued_raw_bytes: u64,
    peak_queued_compressed_bytes: u64,
    peak_queued_total_bytes: u64,
    last_progress_emit_ms: f64,
    progress_events_emitted: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ByteRange {
    start: u64,
    end: u64,
}

impl ByteRange {
    fn len(self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

#[derive(Clone, Debug)]
struct Bootstrap {
    bytes: Vec<u8>,
    root: RootIndex,
    dependency_rounds: u64,
}

#[derive(Debug)]
struct CachedDirectoryPage {
    entries: Vec<ArchiveEntry>,
    last_used: u64,
}

/// Reusable archive reader with a byte-bounded leaf-directory cache.
///
/// The Rust implementation is a correctness and layout prototype. The later
/// TypeScript reader can mirror the same bootstrap, arithmetic page lookup,
/// and cache boundaries without having to reproduce Rust collection layouts.
#[derive(Debug)]
pub struct FixedArchiveReader {
    source: TracingRangeSource<FileRangeSource>,
    bootstrap: Bootstrap,
    directory_cache: HashMap<(usize, u64), CachedDirectoryPage>,
    directory_cache_bytes: usize,
    directory_cache_limit: usize,
    cache_clock: u64,
    first_query: bool,
}

#[derive(Debug)]
struct TemporaryFile {
    file: File,
    path: PathBuf,
    keep_on_drop: bool,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if !self.keep_on_drop && !self.path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReferencePathSpec {
    pub name: FullPathName,
    pub start: u64,
    pub end: u64,
}

pub fn reference_paths(graph: &GBZ) -> ExperimentResult<Vec<ReferencePathSpec>> {
    reference_paths_filtered(graph, None, None)
}

fn reference_paths_filtered(
    graph: &GBZ,
    sample_filter: Option<&str>,
    contig_filter: Option<&str>,
) -> ExperimentResult<Vec<ReferencePathSpec>> {
    let metadata = graph
        .metadata()
        .ok_or_else(|| invalid_data("GBZ metadata is required"))?;
    let reference_ids: BTreeSet<_> = graph.reference_sample_ids(true).into_iter().collect();
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
        let length = graph
            .path(path_id, Orientation::Forward)
            .ok_or_else(|| invalid_data(format!("missing path {path_id}")))?
            .try_fold(0_u64, |length, (node_id, _)| {
                let node_len = graph
                    .sequence_len(node_id)
                    .ok_or_else(|| invalid_data(format!("missing sequence for node {node_id}")))?;
                length
                    .checked_add(usize_to_u64(node_len)?)
                    .ok_or_else(|| invalid_data("reference path length overflow"))
            })?;
        let start = usize_to_u64(name.fragment)?;
        let end = start
            .checked_add(length)
            .ok_or_else(|| invalid_data("reference coordinate overflow"))?;
        result.push(ReferencePathSpec { name, start, end });
    }
    Ok(result)
}

static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

fn temporary_path_near(output: &Path, kind: &str) -> io::Result<PathBuf> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pangenome-range");
    for _ in 0..1024 {
        let id = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".{name}.{kind}.{}.{id}", std::process::id()));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary path",
    ))
}

impl TemporaryFile {
    fn create_near(output: &Path, kind: &str) -> io::Result<Self> {
        for _ in 0..1024 {
            let path = temporary_path_near(output, kind)?;
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        file,
                        path,
                        keep_on_drop: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique temporary file",
        ))
    }

    fn persist(mut self, output: &Path) -> io::Result<()> {
        self.file.sync_all()?;
        std::fs::rename(&self.path, output)?;
        self.path.clear();
        let parent = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        File::open(parent)?.sync_all()?;
        Ok(())
    }

    fn keep_on_failure(&mut self) {
        self.keep_on_drop = true;
    }
}

fn parse_oriented_traversal(value: &serde_json::Value) -> ExperimentResult<Vec<OrientedNode>> {
    value
        .as_array()
        .ok_or_else(|| invalid_data("upstream path field is not an array"))?
        .iter()
        .map(|visit| {
            let id = visit
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| invalid_data("upstream path visit has no string id"))?
                .parse::<u64>()?;
            let reverse = visit
                .get("is_reverse")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| invalid_data("upstream path visit has no orientation"))?;
            Ok(OrientedNode { id, reverse })
        })
        .collect()
}

fn path_interval_from_name(name: &str) -> io::Result<(u64, u64)> {
    let open = name
        .rfind('[')
        .ok_or_else(|| invalid_data("upstream reference name has no interval"))?;
    let close = name.strip_suffix(']').map_or(name.len(), str::len);
    let interval = name
        .get(open + 1..close)
        .ok_or_else(|| invalid_data("upstream reference interval is malformed"))?;
    let (start, end) = interval
        .split_once('-')
        .ok_or_else(|| invalid_data("upstream reference interval has no separator"))?;
    let start = start
        .parse::<u64>()
        .map_err(|_| invalid_data("upstream reference start is invalid"))?;
    let end = end
        .parse::<u64>()
        .map_err(|_| invalid_data("upstream reference end is invalid"))?;
    if start >= end {
        return Err(invalid_data("upstream reference interval is empty"));
    }
    Ok((start, end))
}

#[cfg(test)]
fn aggregate_traversals(
    traversals: &[WeightedTraversal],
) -> io::Result<BTreeMap<Vec<OrientedNode>, u64>> {
    let mut result = BTreeMap::new();
    for item in traversals {
        if item.weight == 0 || item.traversal.is_empty() {
            return Err(invalid_data("local traversal has zero weight or no visits"));
        }
        let weight = result.entry(item.traversal.clone()).or_insert(0_u64);
        *weight = weight
            .checked_add(item.weight)
            .ok_or_else(|| invalid_data("local traversal weight overflow"))?;
    }
    Ok(result)
}

fn mode_evidence(
    mode: HaplotypeOutput,
    traversals: &[WeightedTraversal],
    extraction_wall_ms: f64,
    raw_json_bytes: usize,
) -> ExperimentResult<HaplotypeModeEvidence> {
    let emitted_traversal_nodes = traversals.iter().try_fold(0_u64, |total, item| {
        total
            .checked_add(usize_to_u64(item.traversal.len())?)
            .ok_or_else(|| invalid_data("emitted traversal node count overflow"))
    })?;
    let weighted_traversal_nodes = traversals.iter().try_fold(0_u64, |total, item| {
        let nodes = usize_to_u64(item.traversal.len())?
            .checked_mul(item.weight)
            .ok_or_else(|| invalid_data("weighted traversal node count overflow"))?;
        total
            .checked_add(nodes)
            .ok_or_else(|| invalid_data("weighted traversal node count overflow"))
    })?;
    Ok(HaplotypeModeEvidence {
        mode: mode.to_string(),
        emitted_traversals: usize_to_u64(traversals.len())?,
        total_weight: traversals.iter().try_fold(0_u64, |total, item| {
            total
                .checked_add(item.weight)
                .ok_or_else(|| invalid_data("local traversal total weight overflow"))
        })?,
        emitted_traversal_nodes,
        weighted_traversal_nodes,
        extraction_wall_ms,
        raw_json_bytes: usize_to_u64(raw_json_bytes)?,
    })
}

fn extract_local_region(
    graph: &GBZ,
    path_index: &PathIndex,
    reference: &FullPathName,
    core_start: u64,
    core_end: u64,
    context: u64,
    output: HaplotypeOutput,
) -> ExperimentResult<(RegionalGraph, HaplotypeModeEvidence)> {
    extract_local_region_with_subgraph(
        graph,
        path_index,
        &mut Subgraph::new(),
        reference,
        core_start,
        core_end,
        context,
        output,
    )
}

#[allow(clippy::too_many_arguments)]
fn extract_local_region_with_subgraph(
    graph: &GBZ,
    path_index: &PathIndex,
    subgraph: &mut Subgraph,
    reference: &FullPathName,
    core_start: u64,
    core_end: u64,
    context: u64,
    output: HaplotypeOutput,
) -> ExperimentResult<(RegionalGraph, HaplotypeModeEvidence)> {
    let query = SubgraphQuery::path_interval(
        reference,
        u64_to_usize(core_start)?..u64_to_usize(core_end)?,
    )
    .with_context(u64_to_usize(context)?)
    .with_haplotypes(output);
    let extraction_started = Instant::now();
    subgraph.from_gbz(graph, Some(path_index), None, &query)?;
    let extraction_wall_ms = extraction_started.elapsed().as_secs_f64() * 1_000.0;
    let mut json = Vec::new();
    subgraph.write_json(&mut json, false)?;
    let document: serde_json::Value = serde_json::from_slice(&json)?;
    let paths = document
        .get("paths")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid_data("upstream subgraph JSON has no paths"))?;
    let reference_path = paths
        .first()
        .ok_or_else(|| invalid_data("local extraction did not emit its reference path"))?;
    let reference_name = reference_path
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid_data("upstream reference path has no name"))?;
    let (reference_start, reference_end) = path_interval_from_name(reference_name)?;
    let reference_traversal = parse_oriented_traversal(
        reference_path
            .get("path")
            .ok_or_else(|| invalid_data("upstream reference path has no traversal"))?,
    )?;
    let reference_weight = reference_path.get("weight").map_or(Ok(1_u64), |weight| {
        weight
            .as_u64()
            .ok_or_else(|| invalid_data("upstream reference weight is invalid"))
    })?;
    if reference_weight == 0 {
        return Err(invalid_data("upstream reference weight is zero").into());
    }

    let mut traversals = Vec::new();
    if reference_weight > 1 {
        traversals.push(WeightedTraversal {
            weight: reference_weight - 1,
            traversal: reference_traversal.clone(),
        });
    }
    for path in paths.iter().skip(1) {
        let traversal = parse_oriented_traversal(
            path.get("path")
                .ok_or_else(|| invalid_data("upstream anonymous path has no traversal"))?,
        )?;
        let weight = path.get("weight").map_or(Ok(1_u64), |weight| {
            weight
                .as_u64()
                .ok_or_else(|| invalid_data("upstream anonymous path weight is invalid"))
        })?;
        traversals.push(WeightedTraversal { weight, traversal });
    }
    traversals.sort();
    let evidence = mode_evidence(output, &traversals, extraction_wall_ms, json.len())?;

    let mut regional = RegionalGraph::topology_from_subgraph(subgraph)?;
    regional.semantics = match output {
        HaplotypeOutput::All => HaplotypeSemantics::AnonymousAllTilePaths,
        HaplotypeOutput::Distinct => HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
        _ => return Err(invalid_input("local extraction requires All or Distinct").into()),
    };
    regional.reference_paths.push(RegionalReferencePath {
        sample: reference.sample.clone(),
        contig: reference.contig.clone(),
        haplotype: usize_to_u64(reference.haplotype)?,
        start: reference_start,
        end: reference_end,
        traversal: reference_traversal,
    });
    regional.haplotype_tiles.push(CanonicalHaplotypeTile {
        reference_sample: reference.sample.clone(),
        reference_contig: reference.contig.clone(),
        core_start,
        core_end,
        traversals,
    });
    Ok((regional, evidence))
}

#[derive(Debug)]
struct DirectReferencePosition {
    query_offset: usize,
    node_offset: usize,
    gbwt_pos: Pos,
    path_name: FullPathName,
}

#[derive(Debug)]
struct DirectPathRecord {
    handle: usize,
    successors: Vec<Pos>,
    has_predecessor: Vec<bool>,
    sequence_len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PackedGbwtRecord {
    handle: u64,
    occurrence_count: u64,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordRegionalPayload {
    semantics: HaplotypeSemantics,
    core_start: u64,
    core_end: u64,
    context: u64,
    reference_sample: String,
    reference_contig: String,
    reference_haplotype: u64,
    reference_fragment_start: u64,
    reference_query_offset: u64,
    reference_node_offset: u64,
    reference_position: (u64, u64),
    nodes: BTreeMap<u64, Vec<u8>>,
    edges: BTreeSet<Edge>,
    records: Vec<PackedGbwtRecord>,
    total_occurrences: u64,
}

#[derive(Debug)]
struct RecordChunk {
    raw: Vec<u8>,
    subgraph_selection_wall_ms: f64,
    regional_materialization_wall_ms: f64,
    regional_encoding_wall_ms: f64,
}

#[derive(Debug)]
enum RecordChunkOutcome {
    Accepted(RecordChunk),
    Split {
        estimated_bytes: u64,
        subgraph_selection_wall_ms: f64,
    },
}

#[derive(Clone, Copy, Debug)]
struct ChunkTask {
    start: u64,
    end: u64,
}

#[derive(Debug)]
enum ChunkWorkItem {
    Task(ChunkTask),
    Ready(ChunkTask, RecordChunk),
}

fn gbwt_record(graph: &GBZ, handle: usize) -> Option<Record<'_>> {
    let index: &GBWT = graph.as_ref();
    let records: &BWT = index.as_ref();
    records.record(index.node_to_record(handle))
}

fn direct_reference_position(
    graph: &GBZ,
    path_index: &PathIndex,
    query_pos: &FullPathName,
) -> ExperimentResult<DirectReferencePosition> {
    let path = GBZPath::with_name(graph, query_pos)
        .ok_or_else(|| invalid_data(format!("cannot find a path covering {query_pos}")))?;
    let query_offset = query_pos
        .fragment
        .checked_sub(path.name.fragment)
        .ok_or_else(|| invalid_data("query starts before its reference fragment"))?;
    let index_offset = path_index
        .path_to_offset(path.handle)
        .ok_or_else(|| invalid_data(format!("reference path {} is not indexed", path.name())))?;
    let (mut path_offset, mut pos) = path_index
        .indexed_position(index_offset, query_offset)
        .ok_or_else(|| {
            invalid_data(format!(
                "reference path {} has no indexed position",
                path.name()
            ))
        })?;
    loop {
        let node_id = support::node_id(pos.node);
        let sequence_len = graph.sequence_len(node_id).ok_or_else(|| {
            invalid_data(format!("missing sequence for reference node {node_id}"))
        })?;
        let node_end = path_offset
            .checked_add(sequence_len)
            .ok_or_else(|| invalid_data("reference path offset overflow"))?;
        if node_end > query_offset {
            return Ok(DirectReferencePosition {
                query_offset,
                node_offset: query_offset - path_offset,
                gbwt_pos: pos,
                path_name: path.name,
            });
        }
        path_offset = node_end;
        let record = gbwt_record(graph, pos.node)
            .ok_or_else(|| invalid_data(format!("missing GBWT record for handle {}", pos.node)))?;
        pos = record.lf(pos.offset).ok_or_else(|| {
            invalid_data(format!(
                "reference path {} ended before offset {query_offset}",
                path.name()
            ))
        })?;
    }
}

fn handles_to_oriented(path: &[usize]) -> ExperimentResult<Vec<OrientedNode>> {
    Ok(path
        .iter()
        .map(|&handle| {
            let (node_id, orientation) = support::decode_node(handle);
            oriented(node_id, orientation)
        })
        .collect::<io::Result<Vec<_>>>()?)
}

#[cfg(test)]
#[allow(clippy::too_many_lines)]
fn direct_weighted_paths(
    graph: &GBZ,
    subgraph: &Subgraph,
    reference: &DirectReferencePosition,
) -> ExperimentResult<(Vec<OrientedNode>, u64, u64, Vec<WeightedTraversal>)> {
    let handles = subgraph.handle_iter().collect::<Vec<_>>();
    let mut handle_to_record = HashMap::with_capacity(handles.len());
    let mut records = Vec::with_capacity(handles.len());
    for handle in handles {
        let record = gbwt_record(graph, handle)
            .ok_or_else(|| invalid_data(format!("missing GBWT record for handle {handle}")))?;
        let successors = record.decompress();
        let sequence_len = graph
            .sequence_len(support::node_id(handle))
            .ok_or_else(|| invalid_data(format!("missing sequence for handle {handle}")))?;
        handle_to_record.insert(handle, records.len());
        records.push(DirectPathRecord {
            handle,
            has_predecessor: vec![false; successors.len()],
            successors,
            sequence_len,
        });
    }

    for source in 0..records.len() {
        for offset in 0..records[source].successors.len() {
            let successor = records[source].successors[offset];
            let Some(&target) = handle_to_record.get(&successor.node) else {
                continue;
            };
            let predecessor = records[target]
                .has_predecessor
                .get_mut(successor.offset)
                .ok_or_else(|| invalid_data("GBWT successor offset is outside the local record"))?;
            *predecessor = true;
        }
    }
    let local_occurrences = records.iter().try_fold(0_usize, |total, item| {
        total
            .checked_add(item.successors.len())
            .ok_or_else(|| invalid_data("local GBWT occurrence count overflow"))
    })?;

    let mut paths = Vec::<Vec<usize>>::new();
    let mut reference_path = None::<Vec<usize>>;
    let mut reference_offset = None::<usize>;
    for record in &records {
        for offset in 0..record.successors.len() {
            if record.has_predecessor[offset] {
                continue;
            }
            let mut pos = Some(Pos::new(record.handle, offset));
            let mut path = Vec::new();
            let mut matched_reference = None;
            while let Some(current) = pos {
                if current == reference.gbwt_pos {
                    matched_reference = Some(path.len());
                }
                path.push(current.node);
                let current_record = handle_to_record
                    .get(&current.node)
                    .and_then(|&index| records.get(index))
                    .ok_or_else(|| {
                        invalid_data("local GBWT traversal left the selected records")
                    })?;
                let next = current_record
                    .successors
                    .get(current.offset)
                    .copied()
                    .ok_or_else(|| invalid_data("local GBWT traversal offset is out of bounds"))?;
                pos = (next.node != 0 && handle_to_record.contains_key(&next.node)).then_some(next);
                if path.len() > local_occurrences {
                    return Err(invalid_data("cyclic local GBWT traversal").into());
                }
            }
            if let Some(offset) = matched_reference {
                reference_offset = Some(offset);
                reference_path = Some(path.clone());
                paths.push(path);
            } else if support::encoded_path_is_canonical(&path) {
                paths.push(path);
            }
        }
    }

    let reference_path = reference_path
        .ok_or_else(|| invalid_data("could not find the reference path in direct extraction"))?;
    let reference_offset = reference_offset
        .ok_or_else(|| invalid_data("direct reference path has no matching GBWT position"))?;
    let reference_len = reference_path.iter().try_fold(0_usize, |total, handle| {
        let sequence_len = handle_to_record
            .get(handle)
            .and_then(|&index| records.get(index))
            .map(|record| record.sequence_len)
            .ok_or_else(|| invalid_data("reference traversal handle is not local"))?;
        total
            .checked_add(sequence_len)
            .ok_or_else(|| invalid_data("reference traversal length overflow"))
    })?;
    let prefix_len = reference_path.iter().take(reference_offset).try_fold(
        reference.node_offset,
        |total, handle| {
            let sequence_len = handle_to_record
                .get(handle)
                .and_then(|&index| records.get(index))
                .map(|record| record.sequence_len)
                .ok_or_else(|| invalid_data("reference prefix handle is not local"))?;
            total
                .checked_add(sequence_len)
                .ok_or_else(|| invalid_data("reference prefix length overflow"))
        },
    )?;
    let relative_start = reference
        .query_offset
        .checked_sub(prefix_len)
        .ok_or_else(|| invalid_data("reference context starts before its fragment"))?;
    let reference_start = reference
        .path_name
        .fragment
        .checked_add(relative_start)
        .ok_or_else(|| invalid_data("reference interval start overflow"))?;
    let reference_end = reference_start
        .checked_add(reference_len)
        .ok_or_else(|| invalid_data("reference interval end overflow"))?;

    paths.sort_unstable();
    let mut traversals = Vec::new();
    let mut index = 0;
    while index < paths.len() {
        let mut end = index + 1;
        while end < paths.len() && paths[end] == paths[index] {
            end += 1;
        }
        let count = usize_to_u64(end - index)?;
        let anonymous_weight = if paths[index] == reference_path {
            count.saturating_sub(1)
        } else {
            count
        };
        if anonymous_weight > 0 {
            traversals.push(WeightedTraversal {
                weight: anonymous_weight,
                traversal: handles_to_oriented(&paths[index])?,
            });
        }
        index = end;
    }
    traversals.sort();

    Ok((
        handles_to_oriented(&reference_path)?,
        usize_to_u64(reference_start)?,
        usize_to_u64(reference_end)?,
        traversals,
    ))
}

fn record_payload_size_estimate(
    graph: &GBZ,
    subgraph: &Subgraph,
    reference: &DirectReferencePosition,
) -> ExperimentResult<(u64, u64)> {
    let index: &GBWT = graph.as_ref();
    let bwt: &BWT = index.as_ref();
    let mut bytes = 128_u64
        .checked_add(16)
        .and_then(|value| value.checked_add(usize_to_u64(reference.path_name.sample.len()).ok()?))
        .and_then(|value| value.checked_add(usize_to_u64(reference.path_name.contig.len()).ok()?))
        .ok_or_else(|| invalid_data("record payload header size overflow"))?;
    let mut total_occurrences = 0_u64;
    for node_id in subgraph.node_iter() {
        let sequence_len = subgraph
            .sequence_len(node_id)
            .ok_or_else(|| invalid_data(format!("missing local sequence for node {node_id}")))?;
        bytes = bytes
            .checked_add(16)
            .and_then(|value| value.checked_add(usize_to_u64(sequence_len).ok()?))
            .ok_or_else(|| invalid_data("record payload node size overflow"))?;
        for orientation in [Orientation::Forward, Orientation::Reverse] {
            for (next_id, next_orientation) in subgraph
                .supergraph_successors(node_id, orientation)
                .ok_or_else(|| invalid_data(format!("missing local node {node_id}")))?
            {
                if support::edge_is_canonical((node_id, orientation), (next_id, next_orientation)) {
                    bytes = bytes
                        .checked_add(16)
                        .ok_or_else(|| invalid_data("record payload edge size overflow"))?;
                }
            }
        }
    }
    for handle in subgraph.handle_iter() {
        let record_id = index.node_to_record(handle);
        let (edges, bwt_bytes) = bwt.compressed_record(record_id).ok_or_else(|| {
            invalid_data(format!(
                "missing compressed GBWT record for handle {handle}"
            ))
        })?;
        let occurrence_count = gbwt_record(graph, handle)
            .ok_or_else(|| invalid_data(format!("missing GBWT record for handle {handle}")))?
            .len();
        total_occurrences = total_occurrences
            .checked_add(usize_to_u64(occurrence_count)?)
            .ok_or_else(|| invalid_data("record payload occurrence count overflow"))?;
        bytes = bytes
            .checked_add(24)
            .and_then(|value| value.checked_add(usize_to_u64(edges.len()).ok()?))
            .and_then(|value| value.checked_add(usize_to_u64(bwt_bytes.len()).ok()?))
            .ok_or_else(|| invalid_data("record payload GBWT size overflow"))?;
    }
    Ok((bytes, total_occurrences))
}

impl RecordRegionalPayload {
    fn from_subgraph(
        graph: &GBZ,
        subgraph: &Subgraph,
        reference: &DirectReferencePosition,
        core_start: u64,
        core_end: u64,
    ) -> ExperimentResult<Self> {
        let mut nodes = BTreeMap::new();
        let mut edges = BTreeSet::new();
        for node_id in subgraph.node_iter() {
            let sequence = subgraph.sequence(node_id).ok_or_else(|| {
                invalid_data(format!("missing local sequence for node {node_id}"))
            })?;
            nodes.insert(usize_to_u64(node_id)?, sequence.to_vec());
            for orientation in [Orientation::Forward, Orientation::Reverse] {
                for (next_id, next_orientation) in subgraph
                    .supergraph_successors(node_id, orientation)
                    .ok_or_else(|| invalid_data(format!("missing local node {node_id}")))?
                {
                    if support::edge_is_canonical(
                        (node_id, orientation),
                        (next_id, next_orientation),
                    ) {
                        edges.insert(Edge {
                            from: oriented(node_id, orientation)?,
                            to: oriented(next_id, next_orientation)?,
                        });
                    }
                }
            }
        }

        let index: &GBWT = graph.as_ref();
        let bwt: &BWT = index.as_ref();
        let mut records = Vec::with_capacity(subgraph.handle_iter().count());
        let mut total_occurrences = 0_u64;
        for handle in subgraph.handle_iter() {
            let record_id = index.node_to_record(handle);
            let (edge_bytes, bwt_bytes) = bwt.compressed_record(record_id).ok_or_else(|| {
                invalid_data(format!(
                    "missing compressed GBWT record for handle {handle}"
                ))
            })?;
            let occurrence_count = usize_to_u64(
                gbwt_record(graph, handle)
                    .ok_or_else(|| {
                        invalid_data(format!("missing GBWT record for handle {handle}"))
                    })?
                    .len(),
            )?;
            total_occurrences = total_occurrences
                .checked_add(occurrence_count)
                .ok_or_else(|| invalid_data("record payload occurrence count overflow"))?;
            let mut bytes = Vec::with_capacity(edge_bytes.len() + bwt_bytes.len());
            bytes.extend_from_slice(edge_bytes);
            bytes.extend_from_slice(bwt_bytes);
            records.push(PackedGbwtRecord {
                handle: usize_to_u64(handle)?,
                occurrence_count,
                bytes,
            });
        }
        Ok(Self {
            semantics: HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
            core_start,
            core_end,
            context: CONSTRUCTION_CONTEXT,
            reference_sample: reference.path_name.sample.clone(),
            reference_contig: reference.path_name.contig.clone(),
            reference_haplotype: usize_to_u64(reference.path_name.haplotype)?,
            reference_fragment_start: usize_to_u64(reference.path_name.fragment)?,
            reference_query_offset: usize_to_u64(reference.query_offset)?,
            reference_node_offset: usize_to_u64(reference.node_offset)?,
            reference_position: (
                usize_to_u64(reference.gbwt_pos.node)?,
                usize_to_u64(reference.gbwt_pos.offset)?,
            ),
            nodes,
            edges,
            records,
            total_occurrences,
        })
    }

    fn encode(&self) -> ExperimentResult<Vec<u8>> {
        if self.semantics != HaplotypeSemantics::AnonymousDistinctWeightedTilePaths {
            return Err(invalid_data("record payload requires distinct weighted semantics").into());
        }
        let mut output = Vec::new();
        output.extend_from_slice(REGION_MAGIC);
        put_u32(&mut output, REGION_VERSION);
        put_u32(&mut output, 1);
        output.push(semantics_code(self.semantics));
        output.extend_from_slice(&[0_u8; 7]);
        put_u64(&mut output, usize_to_u64(self.nodes.len())?);
        put_u64(&mut output, usize_to_u64(self.edges.len())?);
        put_u64(&mut output, usize_to_u64(self.records.len())?);
        put_u64(&mut output, self.total_occurrences);
        put_u64(&mut output, self.core_start);
        put_u64(&mut output, self.core_end);
        put_u64(&mut output, self.context);
        put_u64(&mut output, self.reference_haplotype);
        put_u64(&mut output, self.reference_fragment_start);
        put_u64(&mut output, self.reference_query_offset);
        put_u64(&mut output, self.reference_node_offset);
        put_u64(&mut output, self.reference_position.0);
        put_u64(&mut output, self.reference_position.1);
        put_string(&mut output, &self.reference_sample)?;
        put_string(&mut output, &self.reference_contig)?;
        let mut previous_node_id = 0_u64;
        for (node_id, sequence) in &self.nodes {
            put_u64(
                &mut output,
                node_id
                    .checked_sub(previous_node_id)
                    .ok_or_else(|| invalid_data("record payload nodes are not sorted"))?,
            );
            put_bytes(&mut output, sequence)?;
            previous_node_id = *node_id;
        }
        for edge in &self.edges {
            put_u64(&mut output, pack_oriented(edge.from)?);
            put_u64(&mut output, pack_oriented(edge.to)?);
        }
        let mut previous_handle = None;
        let mut total_occurrences = 0_u64;
        for record in &self.records {
            if previous_handle.is_some_and(|handle| record.handle <= handle) {
                return Err(invalid_data("record payload handles are not strictly sorted").into());
            }
            previous_handle = Some(record.handle);
            total_occurrences = total_occurrences
                .checked_add(record.occurrence_count)
                .ok_or_else(|| invalid_data("record payload occurrence count overflow"))?;
            put_u64(&mut output, record.handle);
            put_u64(&mut output, record.occurrence_count);
            put_bytes(&mut output, &record.bytes)?;
        }
        if total_occurrences != self.total_occurrences {
            return Err(
                invalid_data("record payload occurrence total differs from records").into(),
            );
        }
        Ok(output)
    }
}

#[allow(clippy::too_many_lines)]
fn weighted_paths_from_decoded_records(
    mut records: Vec<DirectPathRecord>,
    reference: &DirectReferencePosition,
) -> ExperimentResult<(Vec<OrientedNode>, u64, u64, Vec<WeightedTraversal>)> {
    let handle_to_record = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.handle, index))
        .collect::<HashMap<_, _>>();
    for source in 0..records.len() {
        for offset in 0..records[source].successors.len() {
            let successor = records[source].successors[offset];
            let Some(&target) = handle_to_record.get(&successor.node) else {
                continue;
            };
            let predecessor = records[target]
                .has_predecessor
                .get_mut(successor.offset)
                .ok_or_else(|| invalid_data("GBWT successor offset is outside the local record"))?;
            *predecessor = true;
        }
    }
    let local_occurrences = records.iter().try_fold(0_usize, |total, item| {
        total
            .checked_add(item.successors.len())
            .ok_or_else(|| invalid_data("local GBWT occurrence count overflow"))
    })?;
    let mut paths = Vec::<Vec<usize>>::new();
    let mut reference_path = None::<Vec<usize>>;
    let mut reference_offset = None::<usize>;
    for record in &records {
        for offset in 0..record.successors.len() {
            if record.has_predecessor[offset] {
                continue;
            }
            let mut pos = Some(Pos::new(record.handle, offset));
            let mut path = Vec::new();
            let mut matched_reference = None;
            while let Some(current) = pos {
                if current == reference.gbwt_pos {
                    matched_reference = Some(path.len());
                }
                path.push(current.node);
                let current_record = handle_to_record
                    .get(&current.node)
                    .and_then(|&index| records.get(index))
                    .ok_or_else(|| {
                        invalid_data("local GBWT traversal left the selected records")
                    })?;
                let next = current_record
                    .successors
                    .get(current.offset)
                    .copied()
                    .ok_or_else(|| invalid_data("local GBWT traversal offset is out of bounds"))?;
                pos = (next.node != 0 && handle_to_record.contains_key(&next.node)).then_some(next);
                if path.len() > local_occurrences {
                    return Err(invalid_data("cyclic local GBWT traversal").into());
                }
            }
            if let Some(offset) = matched_reference {
                reference_offset = Some(offset);
                reference_path = Some(path.clone());
                paths.push(path);
            } else if support::encoded_path_is_canonical(&path) {
                paths.push(path);
            }
        }
    }
    let reference_path = reference_path
        .ok_or_else(|| invalid_data("could not find the reference path in record payload"))?;
    let reference_offset = reference_offset
        .ok_or_else(|| invalid_data("record payload reference has no matching GBWT position"))?;
    let reference_len = reference_path.iter().try_fold(0_usize, |total, handle| {
        let sequence_len = handle_to_record
            .get(handle)
            .and_then(|&index| records.get(index))
            .map(|record| record.sequence_len)
            .ok_or_else(|| invalid_data("reference traversal handle is not local"))?;
        total
            .checked_add(sequence_len)
            .ok_or_else(|| invalid_data("reference traversal length overflow"))
    })?;
    let prefix_len = reference_path.iter().take(reference_offset).try_fold(
        reference.node_offset,
        |total, handle| {
            let sequence_len = handle_to_record
                .get(handle)
                .and_then(|&index| records.get(index))
                .map(|record| record.sequence_len)
                .ok_or_else(|| invalid_data("reference prefix handle is not local"))?;
            total
                .checked_add(sequence_len)
                .ok_or_else(|| invalid_data("reference prefix length overflow"))
        },
    )?;
    let relative_start = reference
        .query_offset
        .checked_sub(prefix_len)
        .ok_or_else(|| invalid_data("reference context starts before its fragment"))?;
    let reference_start = reference
        .path_name
        .fragment
        .checked_add(relative_start)
        .ok_or_else(|| invalid_data("reference interval start overflow"))?;
    let reference_end = reference_start
        .checked_add(reference_len)
        .ok_or_else(|| invalid_data("reference interval end overflow"))?;
    paths.sort_unstable();
    let mut traversals = Vec::new();
    let mut index = 0;
    while index < paths.len() {
        let mut end = index + 1;
        while end < paths.len() && paths[end] == paths[index] {
            end += 1;
        }
        let count = usize_to_u64(end - index)?;
        let anonymous_weight = if paths[index] == reference_path {
            count.saturating_sub(1)
        } else {
            count
        };
        if anonymous_weight > 0 {
            traversals.push(WeightedTraversal {
                weight: anonymous_weight,
                traversal: handles_to_oriented(&paths[index])?,
            });
        }
        index = end;
    }
    traversals.sort();
    Ok((
        handles_to_oriented(&reference_path)?,
        usize_to_u64(reference_start)?,
        usize_to_u64(reference_end)?,
        traversals,
    ))
}

#[allow(clippy::too_many_arguments)]
fn construct_record_chunk(
    graph: &GBZ,
    path_index: &PathIndex,
    reference: &FullPathName,
    start: u64,
    end: u64,
    max_uncompressed_bytes: u64,
) -> ExperimentResult<RecordChunkOutcome> {
    let query = SubgraphQuery::path_interval(reference, u64_to_usize(start)?..u64_to_usize(end)?)
        .with_context(u64_to_usize(CONSTRUCTION_CONTEXT)?)
        .with_haplotypes(HaplotypeOutput::None);
    let selection_started = Instant::now();
    let mut subgraph = Subgraph::new();
    subgraph.from_gbz(graph, Some(path_index), None, &query)?;
    let mut query_position = reference.clone();
    query_position.fragment = u64_to_usize(start)?;
    let reference_position = direct_reference_position(graph, path_index, &query_position)?;
    let subgraph_selection_wall_ms = selection_started.elapsed().as_secs_f64() * 1_000.0;
    let (estimated_bytes, total_occurrences) =
        record_payload_size_estimate(graph, &subgraph, &reference_position)?;
    if estimated_bytes > max_uncompressed_bytes {
        return Ok(RecordChunkOutcome::Split {
            estimated_bytes,
            subgraph_selection_wall_ms,
        });
    }
    if total_occurrences > MAX_DECODED_OCCURRENCES_PER_TILE {
        return Ok(RecordChunkOutcome::Split {
            estimated_bytes: estimated_bytes.max(max_uncompressed_bytes.saturating_add(1)),
            subgraph_selection_wall_ms,
        });
    }
    let materialization_started = Instant::now();
    let payload =
        RecordRegionalPayload::from_subgraph(graph, &subgraph, &reference_position, start, end)?;
    let regional_materialization_wall_ms =
        materialization_started.elapsed().as_secs_f64() * 1_000.0;
    let encoding_started = Instant::now();
    let raw = payload.encode()?;
    let regional_encoding_wall_ms = encoding_started.elapsed().as_secs_f64() * 1_000.0;
    if usize_to_u64(raw.len())? != estimated_bytes {
        return Err(invalid_data(format!(
            "record payload estimate {estimated_bytes} differs from encoded size {}",
            raw.len()
        ))
        .into());
    }
    Ok(RecordChunkOutcome::Accepted(RecordChunk {
        raw,
        subgraph_selection_wall_ms,
        regional_materialization_wall_ms,
        regional_encoding_wall_ms,
    }))
}

fn decode_bytecode_integer(bytes: &[u8], position: &mut usize) -> io::Result<u64> {
    let mut result = 0_u64;
    let mut shift = 0_u32;
    loop {
        let byte = *bytes
            .get(*position)
            .ok_or_else(|| invalid_data("truncated GBWT bytecode integer"))?;
        *position = position
            .checked_add(1)
            .ok_or_else(|| invalid_data("GBWT bytecode position overflow"))?;
        let payload = u64::from(byte & 0x7f);
        if shift >= 64 || payload > (u64::MAX >> shift) {
            return Err(invalid_data("GBWT bytecode integer overflow"));
        }
        result = result
            .checked_add(payload << shift)
            .ok_or_else(|| invalid_data("GBWT bytecode integer overflow"))?;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift = shift
            .checked_add(7)
            .ok_or_else(|| invalid_data("GBWT bytecode shift overflow"))?;
    }
}

fn decode_packed_gbwt_record(
    record: &PackedGbwtRecord,
    expand: bool,
) -> ExperimentResult<Vec<Pos>> {
    if record.occurrence_count == 0 {
        return Err(invalid_data("GBWT record has no occurrences").into());
    }
    if record.occurrence_count > MAX_DECODED_OCCURRENCES_PER_TILE {
        return Err(invalid_data("GBWT record exceeds the decoded occurrence safety limit").into());
    }
    let mut position = 0_usize;
    let sigma = decode_bytecode_integer(&record.bytes, &mut position)?;
    if sigma == 0 {
        return Err(invalid_data("GBWT record has an empty edge alphabet").into());
    }
    let sigma = u64_to_usize(sigma)?;
    if sigma > record.bytes.len().saturating_sub(position) / 2 {
        return Err(invalid_data("GBWT edge count exceeds the record bytes").into());
    }
    let mut edges = Vec::with_capacity(sigma);
    let mut previous_node = 0_u64;
    for edge_index in 0..sigma {
        let delta = decode_bytecode_integer(&record.bytes, &mut position)?;
        let node = previous_node
            .checked_add(delta)
            .ok_or_else(|| invalid_data("GBWT successor handle overflow"))?;
        if edge_index > 0 && node <= previous_node {
            return Err(invalid_data("GBWT successor handles are not strictly sorted").into());
        }
        let offset = decode_bytecode_integer(&record.bytes, &mut position)?;
        edges.push((u64_to_usize(node)?, u64_to_usize(offset)?));
        previous_node = node;
    }
    if position >= record.bytes.len() {
        return Err(invalid_data("GBWT record has no run-length data").into());
    }

    let expected = u64_to_usize(record.occurrence_count)?;
    let mut successors = Vec::new();
    if expand {
        successors
            .try_reserve_exact(expected)
            .map_err(|error| invalid_data(format!("cannot allocate GBWT successors: {error}")))?;
    }
    let threshold = if sigma < 255 { 256 / sigma } else { 0 };
    let mut decoded = 0_usize;
    let mut next_offsets = edges.clone();
    while position < record.bytes.len() {
        let (rank, run_len) = if sigma >= 255 {
            let rank = u64_to_usize(decode_bytecode_integer(&record.bytes, &mut position)?)?;
            let run_len = u64_to_usize(
                decode_bytecode_integer(&record.bytes, &mut position)?
                    .checked_add(1)
                    .ok_or_else(|| invalid_data("GBWT run length overflow"))?,
            )?;
            (rank, run_len)
        } else {
            let byte = usize::from(
                *record
                    .bytes
                    .get(position)
                    .ok_or_else(|| invalid_data("truncated GBWT run"))?,
            );
            position += 1;
            let rank = byte % sigma;
            let mut run_len = byte / sigma + 1;
            if run_len == threshold {
                run_len = run_len
                    .checked_add(u64_to_usize(decode_bytecode_integer(
                        &record.bytes,
                        &mut position,
                    )?)?)
                    .ok_or_else(|| invalid_data("GBWT run length overflow"))?;
            }
            (rank, run_len)
        };
        let edge = next_offsets
            .get_mut(rank)
            .ok_or_else(|| invalid_data("GBWT run rank is outside its edge alphabet"))?;
        decoded = decoded
            .checked_add(run_len)
            .filter(|count| *count <= expected)
            .ok_or_else(|| invalid_data("GBWT runs exceed the declared occurrence count"))?;
        if expand {
            for _ in 0..run_len {
                successors.push(Pos::new(edge.0, edge.1));
                edge.1 = edge
                    .1
                    .checked_add(1)
                    .ok_or_else(|| invalid_data("GBWT successor offset overflow"))?;
            }
        } else {
            edge.1 = edge
                .1
                .checked_add(run_len)
                .ok_or_else(|| invalid_data("GBWT successor offset overflow"))?;
        }
    }
    if decoded != expected {
        return Err(invalid_data("GBWT runs differ from the declared occurrence count").into());
    }
    Ok(successors)
}

impl RecordRegionalPayload {
    #[allow(clippy::too_many_lines)]
    fn decode(bytes: &[u8]) -> ExperimentResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        if reader.take(8)? != REGION_MAGIC {
            return Err(invalid_data("invalid record regional chunk magic").into());
        }
        let version = reader.u32()?;
        if version != REGION_VERSION {
            return Err(invalid_data(format!("unsupported regional version {version}")).into());
        }
        if reader.u32()? != 1 {
            return Err(invalid_data("unsupported record regional flags").into());
        }
        let semantics = semantics_from_code(reader.u8()?)?;
        if semantics != HaplotypeSemantics::AnonymousDistinctWeightedTilePaths {
            return Err(invalid_data("record payload must use distinct weighted semantics").into());
        }
        if reader.take(7)? != [0_u8; 7] {
            return Err(invalid_data("record payload reserved bytes are nonzero").into());
        }
        let node_count = reader.u64()?;
        let edge_count = reader.u64()?;
        let record_count = reader.u64()?;
        let total_occurrences = reader.u64()?;
        let core_start = reader.u64()?;
        let core_end = reader.u64()?;
        let context = reader.u64()?;
        let reference_haplotype = reader.u64()?;
        let reference_fragment_start = reader.u64()?;
        let reference_query_offset = reader.u64()?;
        let reference_node_offset = reader.u64()?;
        let reference_position = (reader.u64()?, reader.u64()?);
        if core_start >= core_end {
            return Err(invalid_data("record payload has an invalid core interval").into());
        }
        if context != CONSTRUCTION_CONTEXT {
            return Err(invalid_data(format!(
                "unsupported record payload construction context {context}"
            ))
            .into());
        }
        if reference_fragment_start.checked_add(reference_query_offset) != Some(core_start) {
            return Err(
                invalid_data("record payload query offset does not match core start").into(),
            );
        }
        if total_occurrences == 0 || total_occurrences > MAX_DECODED_OCCURRENCES_PER_TILE {
            return Err(
                invalid_data("record payload occurrence total exceeds its safety bound").into(),
            );
        }
        if record_count
            != node_count
                .checked_mul(2)
                .ok_or_else(|| invalid_data("record count overflow"))?
        {
            return Err(
                invalid_data("record payload must contain both handles for every node").into(),
            );
        }
        let reference_sample = reader.string()?;
        let reference_contig = reader.string()?;
        if reference_sample.is_empty() || reference_contig.is_empty() {
            return Err(invalid_data("record payload reference provenance is empty").into());
        }
        let mut nodes = BTreeMap::new();
        let mut previous_node_id = 0_u64;
        for _ in
            0..count_bounded_by_bytes(node_count, reader.remaining(), 16, "record payload nodes")?
        {
            let node_id = previous_node_id
                .checked_add(reader.u64()?)
                .ok_or_else(|| invalid_data("record payload node delta overflow"))?;
            if node_id == 0 || node_id <= previous_node_id {
                return Err(invalid_data("record payload nodes are not strictly sorted").into());
            }
            let sequence = reader.bytes()?;
            if sequence.is_empty() {
                return Err(invalid_data("record payload contains an empty node sequence").into());
            }
            nodes.insert(node_id, sequence);
            previous_node_id = node_id;
        }
        let mut edges = BTreeSet::new();
        for _ in
            0..count_bounded_by_bytes(edge_count, reader.remaining(), 16, "record payload edges")?
        {
            let edge = Edge {
                from: unpack_oriented(reader.u64()?),
                to: unpack_oriented(reader.u64()?),
            };
            if !nodes.contains_key(&edge.from.id) {
                return Err(invalid_data("record payload edge source is not local").into());
            }
            let from = (
                u64_to_usize(edge.from.id)?,
                if edge.from.reverse {
                    Orientation::Reverse
                } else {
                    Orientation::Forward
                },
            );
            let to = (
                u64_to_usize(edge.to.id)?,
                if edge.to.reverse {
                    Orientation::Reverse
                } else {
                    Orientation::Forward
                },
            );
            if !support::edge_is_canonical(from, to) {
                return Err(invalid_data("record payload edge is not canonical").into());
            }
            if !edges.insert(edge) {
                return Err(invalid_data("record payload contains a duplicate edge").into());
            }
        }
        let mut records = Vec::with_capacity(count_bounded_by_bytes(
            record_count,
            reader.remaining(),
            24,
            "record payload GBWT records",
        )?);
        let mut previous_handle = None;
        let mut decoded_occurrences = 0_u64;
        for _ in 0..record_count {
            let record = PackedGbwtRecord {
                handle: reader.u64()?,
                occurrence_count: reader.u64()?,
                bytes: reader.bytes()?,
            };
            if record.handle == 0 || previous_handle.is_some_and(|handle| record.handle <= handle) {
                return Err(invalid_data("record payload handles are not strictly sorted").into());
            }
            if !nodes.contains_key(&(record.handle / 2)) {
                return Err(invalid_data("record payload handle refers to an absent node").into());
            }
            let _ = u64_to_usize(record.handle)?;
            decode_packed_gbwt_record(&record, false)?;
            decoded_occurrences = decoded_occurrences
                .checked_add(record.occurrence_count)
                .ok_or_else(|| invalid_data("record payload occurrence total overflow"))?;
            previous_handle = Some(record.handle);
            records.push(record);
        }
        reader.finish()?;
        if decoded_occurrences != total_occurrences {
            return Err(
                invalid_data("record payload occurrence total differs from records").into(),
            );
        }
        let reference_record = records
            .binary_search_by_key(&reference_position.0, |record| record.handle)
            .ok()
            .and_then(|index| records.get(index))
            .ok_or_else(|| invalid_data("record payload reference handle is not local"))?;
        if reference_position.1 >= reference_record.occurrence_count {
            return Err(invalid_data("record payload reference offset is out of bounds").into());
        }
        let reference_node = nodes
            .get(&(reference_position.0 / 2))
            .ok_or_else(|| invalid_data("record payload reference node is absent"))?;
        if reference_node_offset >= usize_to_u64(reference_node.len())? {
            return Err(
                invalid_data("record payload reference node offset is out of bounds").into(),
            );
        }
        Ok(Self {
            semantics,
            core_start,
            core_end,
            context,
            reference_sample,
            reference_contig,
            reference_haplotype,
            reference_fragment_start,
            reference_query_offset,
            reference_node_offset,
            reference_position,
            nodes,
            edges,
            records,
            total_occurrences,
        })
    }

    fn into_regional_graph(self) -> ExperimentResult<RegionalGraph> {
        let mut direct_records = Vec::with_capacity(self.records.len());
        for record in &self.records {
            let successors = decode_packed_gbwt_record(record, true)?;
            let sequence_len = self
                .nodes
                .get(&(record.handle / 2))
                .ok_or_else(|| invalid_data("record payload handle has no local sequence"))?
                .len();
            direct_records.push(DirectPathRecord {
                handle: u64_to_usize(record.handle)?,
                has_predecessor: vec![false; successors.len()],
                successors,
                sequence_len,
            });
        }
        let reference = DirectReferencePosition {
            query_offset: u64_to_usize(self.reference_query_offset)?,
            node_offset: u64_to_usize(self.reference_node_offset)?,
            gbwt_pos: Pos::new(
                u64_to_usize(self.reference_position.0)?,
                u64_to_usize(self.reference_position.1)?,
            ),
            path_name: FullPathName {
                sample: self.reference_sample.clone(),
                contig: self.reference_contig.clone(),
                haplotype: u64_to_usize(self.reference_haplotype)?,
                fragment: u64_to_usize(self.reference_fragment_start)?,
            },
        };
        let (reference_traversal, reference_start, reference_end, traversals) =
            weighted_paths_from_decoded_records(direct_records, &reference)?;
        let mut result = RegionalGraph {
            nodes: self.nodes,
            edges: self.edges,
            semantics: self.semantics,
            ..RegionalGraph::default()
        };
        result.reference_paths.push(RegionalReferencePath {
            sample: self.reference_sample.clone(),
            contig: self.reference_contig.clone(),
            haplotype: self.reference_haplotype,
            start: reference_start,
            end: reference_end,
            traversal: reference_traversal,
        });
        result.haplotype_tiles.push(CanonicalHaplotypeTile {
            reference_sample: self.reference_sample,
            reference_contig: self.reference_contig,
            core_start: self.core_start,
            core_end: self.core_end,
            traversals,
        });
        Ok(result)
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn extract_local_region_direct(
    graph: &GBZ,
    path_index: &PathIndex,
    subgraph: &mut Subgraph,
    reference: &FullPathName,
    core_start: u64,
    core_end: u64,
    context: u64,
) -> ExperimentResult<(RegionalGraph, HaplotypeModeEvidence)> {
    let query = SubgraphQuery::path_interval(
        reference,
        u64_to_usize(core_start)?..u64_to_usize(core_end)?,
    )
    .with_context(u64_to_usize(context)?)
    .with_haplotypes(HaplotypeOutput::None);
    let extraction_started = Instant::now();
    subgraph.from_gbz(graph, Some(path_index), None, &query)?;
    let mut query_position = reference.clone();
    query_position.fragment = u64_to_usize(core_start)?;
    let reference_position = direct_reference_position(graph, path_index, &query_position)?;
    let (reference_traversal, reference_start, reference_end, traversals) =
        direct_weighted_paths(graph, subgraph, &reference_position)?;
    let extraction_wall_ms = extraction_started.elapsed().as_secs_f64() * 1_000.0;
    let evidence = mode_evidence(
        HaplotypeOutput::Distinct,
        &traversals,
        extraction_wall_ms,
        0,
    )?;

    let mut regional = RegionalGraph::topology_from_subgraph(subgraph)?;
    regional.semantics = HaplotypeSemantics::AnonymousDistinctWeightedTilePaths;
    regional.reference_paths.push(RegionalReferencePath {
        sample: reference.sample.clone(),
        contig: reference.contig.clone(),
        haplotype: usize_to_u64(reference.haplotype)?,
        start: reference_start,
        end: reference_end,
        traversal: reference_traversal,
    });
    regional.haplotype_tiles.push(CanonicalHaplotypeTile {
        reference_sample: reference.sample.clone(),
        reference_contig: reference.contig.clone(),
        core_start,
        core_end,
        traversals,
    });
    Ok((regional, evidence))
}

#[cfg(test)]
pub fn compare_haplotype_outputs(
    graph: &GBZ,
    path_index: &PathIndex,
    reference: &ReferencePathSpec,
    core_start: u64,
    core_end: u64,
) -> ExperimentResult<HaplotypeExtractionEvidence> {
    let (all, all_evidence) = extract_local_region(
        graph,
        path_index,
        &reference.name,
        core_start,
        core_end,
        CONSTRUCTION_CONTEXT,
        HaplotypeOutput::All,
    )?;
    let (distinct, distinct_evidence) = extract_local_region(
        graph,
        path_index,
        &reference.name,
        core_start,
        core_end,
        CONSTRUCTION_CONTEXT,
        HaplotypeOutput::Distinct,
    )?;
    let all_traversals = &all.haplotype_tiles[0].traversals;
    let distinct_traversals = &distinct.haplotype_tiles[0].traversals;
    let exact_multiset_equivalent =
        aggregate_traversals(all_traversals)? == aggregate_traversals(distinct_traversals)?;
    Ok(HaplotypeExtractionEvidence {
        reference_sample: reference.name.sample.clone(),
        reference_contig: reference.name.contig.clone(),
        core_start,
        core_end,
        all: all_evidence,
        distinct: distinct_evidence,
        exact_multiset_equivalent,
    })
}

fn selected_reference_paths(
    graph: &GBZ,
    config: &FixedArchiveConfig,
    options: &ArchiveBuildOptions,
) -> ExperimentResult<Vec<ReferencePathSpec>> {
    let mut references =
        reference_paths_filtered(graph, options.sample.as_deref(), options.contig.as_deref())?
            .into_iter()
            .filter_map(|mut reference| {
                reference.start = reference
                    .start
                    .max(options.start.unwrap_or(reference.start));
                reference.end = reference.end.min(options.end.unwrap_or(reference.end));
                (reference.start < reference.end).then_some(reference)
            })
            .collect::<Vec<_>>();
    if let Some(max_chunks) = options.max_chunks {
        if max_chunks == 0 {
            return Err(invalid_input("--max-chunks must be greater than zero").into());
        }
        if references.len() != 1 {
            return Err(invalid_input(
                "--max-chunks requires filters that select exactly one reference path",
            )
            .into());
        }
        let max_span = max_chunks
            .checked_mul(config.window_size)
            .ok_or_else(|| invalid_data("--max-chunks coordinate span overflow"))?;
        references[0].end = references[0]
            .end
            .min(references[0].start.saturating_add(max_span));
    }
    Ok(references)
}

#[allow(clippy::cast_precision_loss, clippy::too_many_arguments)]
fn maybe_emit_chunk_progress(
    mode: BuildProgressMode,
    interval_ms: u64,
    force: bool,
    archive: &File,
    manifest: &ReferenceManifest,
    reference_id: usize,
    reference_count: usize,
    chunk_start: u64,
    chunk_end: u64,
    total_reference_bases: u64,
    build_started: Instant,
    processing_started: Instant,
    state: &mut DirectWriterState,
) -> ExperimentResult<()> {
    if mode == BuildProgressMode::Off {
        return Ok(());
    }
    let processing_elapsed_seconds = processing_started.elapsed().as_secs_f64();
    let processing_elapsed_ms = processing_elapsed_seconds * 1_000.0;
    if !force
        && state.progress_events_emitted > 0
        && processing_elapsed_ms - state.last_progress_emit_ms < interval_ms as f64
    {
        return Ok(());
    }
    let reference_bases = manifest.end.saturating_sub(manifest.start);
    let reference_bases_processed = chunk_end.min(manifest.end).saturating_sub(manifest.start);
    let reference_bp_per_second = if processing_elapsed_seconds > 0.0 {
        state.reference_bases_processed as f64 / processing_elapsed_seconds
    } else {
        0.0
    };
    let chunks_per_second = if processing_elapsed_seconds > 0.0 {
        state.accepted_chunks as f64 / processing_elapsed_seconds
    } else {
        0.0
    };
    let estimated_seconds_remaining = (reference_bp_per_second > 0.0).then(|| {
        total_reference_bases.saturating_sub(state.reference_bases_processed) as f64
            / reference_bp_per_second
    });
    state.progress_events_emitted = state
        .progress_events_emitted
        .checked_add(1)
        .ok_or_else(|| invalid_data("progress event count overflow"))?;
    state.last_progress_emit_ms = processing_elapsed_ms;
    let reference_complete = chunk_end >= manifest.end;
    let snapshot = BuildProgressSnapshot {
        phase: "chunk_progress",
        sequence: state.progress_events_emitted,
        reference_ordinal: usize_to_u64(reference_id + 1)?,
        reference_count: usize_to_u64(reference_count)?,
        references_completed: usize_to_u64(reference_id + usize::from(reference_complete))?,
        sample: manifest.sample.clone(),
        contig: manifest.contig.clone(),
        current_chunk_start: chunk_start,
        current_chunk_end: chunk_end,
        reference_start: manifest.start,
        reference_end: manifest.end,
        reference_percent_complete: ratio(reference_bases_processed, reference_bases) * 100.0,
        processed_reference_bases: state.reference_bases_processed,
        total_reference_bases,
        percent_complete: ratio(state.reference_bases_processed, total_reference_bases) * 100.0,
        accepted_chunks: state.accepted_chunks,
        physical_chunks: usize_to_u64(state.chunks.len())?,
        reference_bp_per_second,
        chunks_per_second,
        estimated_seconds_remaining,
        processing_elapsed_seconds,
        build_elapsed_seconds: build_started.elapsed().as_secs_f64(),
        temporary_archive_bytes: archive.metadata()?.len(),
    };
    emit_progress_snapshot(mode, &snapshot);
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn flush_pending_chunks(
    pending: &mut Vec<PendingRawChunk>,
    archive: &mut File,
    manifests: &mut [ReferenceManifest],
    bucket_entries: &mut [Vec<Vec<ArchiveEntry>>],
    bucket_span: u64,
    config: &FixedArchiveConfig,
    options: &ArchiveBuildOptions,
    build_started: Instant,
    processing_started: Instant,
    total_reference_bases: u64,
    reference_count: usize,
    state: &mut DirectWriterState,
) -> ExperimentResult<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let batch = std::mem::take(pending);
    let queued_raw_bytes = batch.iter().try_fold(0_u64, |total, chunk| {
        total
            .checked_add(usize_to_u64(chunk.raw.len())?)
            .ok_or_else(|| invalid_data("queued raw byte count overflow"))
    })?;
    state.peak_queued_raw_bytes = state.peak_queued_raw_bytes.max(queued_raw_bytes);

    let compression_started = Instant::now();
    let compressed = if options.threads == 1 || batch.len() == 1 {
        batch
            .iter()
            .map(|chunk| compress(config.codec, &chunk.raw))
            .collect::<io::Result<Vec<_>>>()?
    } else {
        std::thread::scope(|scope| {
            let handles = batch
                .iter()
                .map(|chunk| scope.spawn(|| compress(config.codec, &chunk.raw)))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .map_err(|_| invalid_data("compression worker panicked"))?
                })
                .collect::<io::Result<Vec<_>>>()
        })?
    };
    state.compression_wall_ms += compression_started.elapsed().as_secs_f64() * 1_000.0;
    let queued_compressed_bytes = compressed.iter().try_fold(0_u64, |total, bytes| {
        total
            .checked_add(usize_to_u64(bytes.len())?)
            .ok_or_else(|| invalid_data("queued compressed byte count overflow"))
    })?;
    state.peak_queued_compressed_bytes = state
        .peak_queued_compressed_bytes
        .max(queued_compressed_bytes);
    let queued_total_bytes = queued_raw_bytes
        .checked_add(queued_compressed_bytes)
        .ok_or_else(|| invalid_data("total queued byte count overflow"))?;
    state.peak_queued_total_bytes = state.peak_queued_total_bytes.max(queued_total_bytes);
    if queued_total_bytes > options.max_queued_bytes {
        return Err(invalid_data(format!(
            "compression batch exceeded the {} byte queue cap",
            options.max_queued_bytes
        ))
        .into());
    }

    for (chunk, compressed) in batch.into_iter().zip(compressed) {
        let raw_len = usize_to_u64(chunk.raw.len())?;
        let identical = find_identical_archive_chunk(
            archive,
            &state.chunks,
            state.chunk_by_hash.get(&chunk.hash),
            config.codec,
            &chunk.raw,
        )?;
        if let Some(existing) = identical {
            state.duplicate_payload_entries_observed = state
                .duplicate_payload_entries_observed
                .checked_add(1)
                .ok_or_else(|| invalid_data("duplicate payload count overflow"))?;
            state.avoidable_compressed_payload_bytes = state
                .avoidable_compressed_payload_bytes
                .checked_add(state.chunks[existing].compressed_len)
                .ok_or_else(|| invalid_data("avoidable payload byte count overflow"))?;
        }
        let chunk_id = if config.deduplicate_chunks {
            identical
        } else {
            None
        }
        .map_or_else(
            || -> ExperimentResult<usize> {
                let compressed_len = usize_to_u64(compressed.len())?;
                state.peak_compressed_chunk_bytes =
                    state.peak_compressed_chunk_bytes.max(compressed_len);
                let archive_offset = archive.seek(SeekFrom::End(0))?;
                archive.write_all(&compressed)?;
                if state.first_payload_wall_ms.is_none() {
                    let elapsed = build_started.elapsed().as_secs_f64() * 1_000.0;
                    state.first_payload_wall_ms = Some(elapsed);
                    emit_progress(
                        options.progress,
                        "writer_append",
                        &format!("first payload written after {elapsed:.3} ms"),
                    );
                }
                let chunk_id = state.chunks.len();
                state.chunks.push(StoredChunk {
                    archive_offset,
                    compressed_len,
                    uncompressed_len: raw_len,
                });
                state
                    .chunk_by_hash
                    .entry(chunk.hash)
                    .or_default()
                    .push(chunk_id);
                Ok(chunk_id)
            },
            Ok,
        )?;
        state.accepted_chunks = state
            .accepted_chunks
            .checked_add(1)
            .ok_or_else(|| invalid_data("accepted chunk count overflow"))?;
        if options
            .max_chunks
            .is_some_and(|max_chunks| state.accepted_chunks > max_chunks)
        {
            return Err(invalid_data(
                "adaptive splitting exceeded --max-chunks; narrow the interval or raise the guard",
            )
            .into());
        }
        let manifest = manifests
            .get_mut(chunk.reference_id)
            .ok_or_else(|| invalid_data("pending chunk reference is out of range"))?;
        let bucket_index = chunk
            .start
            .checked_sub(manifest.grid_start)
            .ok_or_else(|| invalid_data("chunk starts before its reference grid"))?
            / bucket_span;
        manifest.entry_count = manifest
            .entry_count
            .checked_add(1)
            .ok_or_else(|| invalid_data("manifest entry count overflow"))?;
        let bucket = bucket_entries
            .get_mut(chunk.reference_id)
            .and_then(|buckets| buckets.get_mut(u64_to_usize(bucket_index).ok()?))
            .ok_or_else(|| invalid_data("chunk directory bucket is out of range"))?;
        if bucket.len() >= DIRECTORY_ENTRIES_PER_PAGE {
            return Err(invalid_data(format!(
                "directory bucket {bucket_index} exceeds fixed page capacity {DIRECTORY_ENTRIES_PER_PAGE}",
            ))
            .into());
        }
        let stored = &state.chunks[chunk_id];
        bucket.push(ArchiveEntry {
            start: chunk.start,
            end: chunk.end,
            offset: stored.archive_offset,
            compressed_len: stored.compressed_len,
            uncompressed_len: stored.uncompressed_len,
            codec: config.codec,
        });
        state.reference_bases_processed = state
            .reference_bases_processed
            .checked_add(chunk.end - chunk.start)
            .ok_or_else(|| invalid_data("processed reference bases overflow"))?;
        maybe_emit_chunk_progress(
            options.progress,
            options.progress_interval_ms,
            chunk.end >= manifest.end,
            archive,
            manifest,
            chunk.reference_id,
            reference_count,
            chunk.start,
            chunk.end,
            total_reference_bases,
            build_started,
            processing_started,
            state,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn queue_record_chunk(
    record_chunk: RecordChunk,
    task: ChunkTask,
    reference_id: usize,
    pending: &mut Vec<PendingRawChunk>,
    pending_memory_bound: &mut u64,
    peak_raw_chunk_bytes: &mut u64,
    archive: &mut File,
    manifests: &mut [ReferenceManifest],
    bucket_entries: &mut [Vec<Vec<ArchiveEntry>>],
    bucket_span: u64,
    config: &FixedArchiveConfig,
    options: &ArchiveBuildOptions,
    build_started: Instant,
    processing_started: Instant,
    total_reference_bases: u64,
    reference_count: usize,
    writer: &mut DirectWriterState,
) -> ExperimentResult<()> {
    let raw_len = usize_to_u64(record_chunk.raw.len())?;
    *peak_raw_chunk_bytes = (*peak_raw_chunk_bytes).max(raw_len);
    let compressed_bound = match config.codec {
        ChunkCodec::None => raw_len,
        _ => usize_to_u64(zstd::zstd_safe::compress_bound(record_chunk.raw.len()))?,
    };
    let chunk_memory_bound = raw_len
        .checked_add(compressed_bound)
        .ok_or_else(|| invalid_data("chunk queue bound overflow"))?;
    if chunk_memory_bound > options.max_queued_bytes {
        return Err(invalid_input(format!(
            "accepted raw+compressed chunk bound is {chunk_memory_bound} bytes, above the {} byte queue cap",
            options.max_queued_bytes
        ))
        .into());
    }
    if !pending.is_empty()
        && (pending.len() >= options.threads
            || pending_memory_bound.saturating_add(chunk_memory_bound) > options.max_queued_bytes)
    {
        flush_pending_chunks(
            pending,
            archive,
            manifests,
            bucket_entries,
            bucket_span,
            config,
            options,
            build_started,
            processing_started,
            total_reference_bases,
            reference_count,
            writer,
        )?;
        *pending_memory_bound = 0;
    }
    let hash = blake3::hash(&record_chunk.raw);
    pending.push(PendingRawChunk {
        reference_id,
        start: task.start,
        end: task.end,
        raw: record_chunk.raw,
        hash,
    });
    *pending_memory_bound = pending_memory_bound
        .checked_add(chunk_memory_bound)
        .ok_or_else(|| invalid_data("pending queue bound overflow"))?;
    if pending.len() >= options.threads {
        flush_pending_chunks(
            pending,
            archive,
            manifests,
            bucket_entries,
            bucket_span,
            config,
            options,
            build_started,
            processing_started,
            total_reference_bases,
            reference_count,
            writer,
        )?;
        *pending_memory_bound = 0;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub fn build_fixed_archive(
    graph: &GBZ,
    path_index: &PathIndex,
    source_gbz_bytes: u64,
    output: &Path,
    config: &FixedArchiveConfig,
) -> ExperimentResult<ArchiveBuildMetrics> {
    build_fixed_archive_with_options(
        graph,
        path_index,
        source_gbz_bytes,
        output,
        config,
        &ArchiveBuildOptions::default(),
    )
}

#[allow(clippy::too_many_lines)]
/// Builds a filtered v4 archive through the bounded direct-write pipeline.
///
/// # Errors
///
/// Returns an error for invalid filters or limits, GBZ extraction failures,
/// archive encoding/corruption errors, or destination I/O failures.
pub fn build_fixed_archive_with_options(
    graph: &GBZ,
    path_index: &PathIndex,
    source_gbz_bytes: u64,
    output: &Path,
    config: &FixedArchiveConfig,
    options: &ArchiveBuildOptions,
) -> ExperimentResult<ArchiveBuildMetrics> {
    if config.window_size == 0 {
        return Err(invalid_input("window size must be greater than zero").into());
    }
    if config.min_window_size == 0 || config.min_window_size > config.window_size {
        return Err(invalid_input("minimum adaptive window must be in 1..=window_size").into());
    }
    if config.max_uncompressed_chunk_bytes == 0 {
        return Err(invalid_input("maximum uncompressed chunk bytes must be nonzero").into());
    }
    if options.threads == 0 {
        return Err(invalid_input("thread count must be greater than zero").into());
    }
    if options.max_queued_bytes == 0 {
        return Err(invalid_input("maximum queued bytes must be greater than zero").into());
    }
    if (options.start.is_some() || options.end.is_some()) && options.contig.is_none() {
        return Err(invalid_input("--start/--end require a contig filter").into());
    }
    let started = Instant::now();
    let manifest_started = Instant::now();
    let references = selected_reference_paths(graph, config, options)?;
    let reference_manifest_discovery_wall_ms = manifest_started.elapsed().as_secs_f64() * 1_000.0;
    if references.is_empty() {
        return Err(invalid_input("no reference paths match the requested filters").into());
    }
    let total_reference_bases = references.iter().try_fold(0_u64, |total, reference| {
        total
            .checked_add(reference.end.saturating_sub(reference.start))
            .ok_or_else(|| invalid_data("total reference base count overflow"))
    })?;
    let planned_base_windows = references.iter().try_fold(0_u64, |total, reference| {
        let grid_start = (reference.start / config.window_size) * config.window_size;
        let span = reference
            .end
            .checked_sub(grid_start)
            .ok_or_else(|| invalid_data("reference window span underflow"))?;
        let windows = span
            .checked_add(config.window_size - 1)
            .ok_or_else(|| invalid_data("reference window count overflow"))?
            / config.window_size;
        total
            .checked_add(windows)
            .ok_or_else(|| invalid_data("planned base window count overflow"))
    })?;
    let bucket_span = config
        .window_size
        .checked_mul(DIRECTORY_BUCKET_WINDOWS)
        .ok_or_else(|| invalid_data("directory bucket span overflow"))?;
    let mut manifests = references
        .iter()
        .map(|reference| {
            let grid_start = (reference.start / config.window_size) * config.window_size;
            let span = reference
                .end
                .checked_sub(grid_start)
                .ok_or_else(|| invalid_data("reference grid interval underflow"))?;
            let page_count = span
                .checked_add(bucket_span - 1)
                .ok_or_else(|| invalid_data("reference directory span overflow"))?
                / bucket_span;
            Ok(ReferenceManifest {
                sample: reference.name.sample.clone(),
                contig: reference.name.contig.clone(),
                start: reference.start,
                end: reference.end,
                grid_start,
                window_size: config.window_size,
                bucket_span,
                first_page_offset: 0,
                page_count,
                entry_count: 0,
                codec: config.codec,
            })
        })
        .collect::<ExperimentResult<Vec<_>>>()?;
    let provisional_root = encode_root_index(&manifests)?;
    let mut next_page_offset = usize_to_u64(HEADER_LEN + provisional_root.len())?;
    for manifest in &mut manifests {
        manifest.first_page_offset = next_page_offset;
        next_page_offset = next_page_offset
            .checked_add(
                manifest
                    .page_count
                    .checked_mul(usize_to_u64(DIRECTORY_PAGE_BYTES)?)
                    .ok_or_else(|| invalid_data("directory byte count overflow"))?,
            )
            .ok_or_else(|| invalid_data("archive data offset overflow"))?;
    }
    let data_offset = next_page_offset;
    let root = encode_root_index(&manifests)?;
    if root.len() != provisional_root.len() {
        return Err(invalid_data("root index size changed after assigning offsets").into());
    }
    let mut archive_temp = TemporaryFile::create_near(output, "archive")?;
    if options.keep_partial {
        archive_temp.keep_on_failure();
    }
    archive_temp
        .file
        .write_all(&encode_header(usize_to_u64(root.len())?, 0, data_offset))?;
    archive_temp.file.write_all(&root)?;
    archive_temp.file.set_len(data_offset)?;
    archive_temp.file.seek(SeekFrom::Start(data_offset))?;

    let mut bucket_entries = manifests
        .iter()
        .map(|manifest| {
            Ok(vec![
                Vec::<ArchiveEntry>::new();
                u64_to_usize(manifest.page_count)?
            ])
        })
        .collect::<Result<Vec<_>, io::Error>>()?;
    let mut writer = DirectWriterState::default();
    let mut pending = Vec::<PendingRawChunk>::with_capacity(options.threads);
    let mut pending_memory_bound = 0_u64;
    let mut ready_raw_bytes = 0_u64;
    let mut peak_ready_raw_bytes = 0_u64;
    let mut peak_raw_chunk_bytes = 0_u64;
    let mut adaptive_splits = 0_u64;
    let preflight_splits = 0_u64;
    let mut post_materialization_splits = 0_u64;
    let mut largest_rejected_parent_bytes = 0_u64;
    let preflight_selection_wall_ms = 0.0;
    let mut subgraph_selection_wall_ms = 0.0;
    let mut regional_materialization_wall_ms = 0.0;
    let mut regional_encoding_wall_ms = 0.0;
    let haplotype_extraction_evidence = None;
    let processing_started = Instant::now();
    emit_encoding_plan(
        options.progress,
        usize_to_u64(references.len())?,
        total_reference_bases,
        planned_base_windows,
        data_offset,
        options.progress_interval_ms,
    );

    for (reference_id, reference) in references.iter().enumerate() {
        emit_reference_start(
            options.progress,
            usize_to_u64(reference_id + 1)?,
            usize_to_u64(references.len())?,
            reference,
        );
        let first_boundary = (reference.start / config.window_size) * config.window_size;
        let mut boundary = first_boundary;
        let mut work = VecDeque::new();
        while boundary < reference.end {
            let boundary_end = boundary
                .checked_add(config.window_size)
                .ok_or_else(|| invalid_data("window boundary overflow"))?;
            let start = boundary.max(reference.start);
            let end = boundary_end.min(reference.end);
            if start < end {
                work.push_back(ChunkWorkItem::Task(ChunkTask { start, end }));
            }
            boundary = boundary_end;
        }
        let query_name = FullPathName {
            sample: reference.name.sample.clone(),
            contig: reference.name.contig.clone(),
            haplotype: reference.name.haplotype,
            fragment: 0,
        };
        while !work.is_empty() {
            if matches!(work.front(), Some(ChunkWorkItem::Ready(_, _))) {
                let Some(ChunkWorkItem::Ready(task, chunk)) = work.pop_front() else {
                    unreachable!("front was checked as a ready chunk");
                };
                ready_raw_bytes = ready_raw_bytes
                    .checked_sub(usize_to_u64(chunk.raw.len())?)
                    .ok_or_else(|| invalid_data("ready raw byte count underflow"))?;
                subgraph_selection_wall_ms += chunk.subgraph_selection_wall_ms;
                regional_materialization_wall_ms += chunk.regional_materialization_wall_ms;
                regional_encoding_wall_ms += chunk.regional_encoding_wall_ms;
                queue_record_chunk(
                    chunk,
                    task,
                    reference_id,
                    &mut pending,
                    &mut pending_memory_bound,
                    &mut peak_raw_chunk_bytes,
                    &mut archive_temp.file,
                    &mut manifests,
                    &mut bucket_entries,
                    bucket_span,
                    config,
                    options,
                    started,
                    processing_started,
                    total_reference_bases,
                    references.len(),
                    &mut writer,
                )?;
                continue;
            }
            flush_pending_chunks(
                &mut pending,
                &mut archive_temp.file,
                &mut manifests,
                &mut bucket_entries,
                bucket_span,
                config,
                options,
                started,
                processing_started,
                total_reference_bases,
                references.len(),
                &mut writer,
            )?;
            pending_memory_bound = 0;
            let mut tasks = Vec::with_capacity(options.threads);
            while tasks.len() < options.threads {
                match work.front() {
                    Some(ChunkWorkItem::Task(_)) => {
                        let Some(ChunkWorkItem::Task(task)) = work.pop_front() else {
                            unreachable!("front was checked as a task");
                        };
                        tasks.push(task);
                    }
                    _ => break,
                }
            }
            let outcomes = if tasks.len() == 1 {
                vec![construct_record_chunk(
                    graph,
                    path_index,
                    &query_name,
                    tasks[0].start,
                    tasks[0].end,
                    config.max_uncompressed_chunk_bytes,
                )?]
            } else {
                std::thread::scope(|scope| {
                    let query_name = &query_name;
                    let handles = tasks
                        .iter()
                        .map(|task| {
                            scope.spawn(move || {
                                construct_record_chunk(
                                    graph,
                                    path_index,
                                    query_name,
                                    task.start,
                                    task.end,
                                    config.max_uncompressed_chunk_bytes,
                                )
                            })
                        })
                        .collect::<Vec<_>>();
                    handles
                        .into_iter()
                        .map(|handle| {
                            handle
                                .join()
                                .map_err(|_| invalid_data("record construction worker panicked"))?
                        })
                        .collect::<ExperimentResult<Vec<_>>>()
                })?
            };
            for (task, outcome) in tasks.into_iter().zip(outcomes).rev() {
                match outcome {
                    RecordChunkOutcome::Accepted(chunk) => {
                        ready_raw_bytes = ready_raw_bytes
                            .checked_add(usize_to_u64(chunk.raw.len())?)
                            .ok_or_else(|| invalid_data("ready raw byte count overflow"))?;
                        peak_ready_raw_bytes = peak_ready_raw_bytes.max(ready_raw_bytes);
                        work.push_front(ChunkWorkItem::Ready(task, chunk));
                    }
                    RecordChunkOutcome::Split {
                        estimated_bytes,
                        subgraph_selection_wall_ms: selection_ms,
                    } => {
                        subgraph_selection_wall_ms += selection_ms;
                        let [left, right] = adaptive_split_interval(
                            task.start,
                            task.end,
                            estimated_bytes,
                            config.max_uncompressed_chunk_bytes,
                            config.min_window_size,
                        )?
                        .ok_or_else(|| {
                            invalid_data("record payload requested a split below its byte cap")
                        })?;
                        work.push_front(ChunkWorkItem::Task(ChunkTask {
                            start: right.0,
                            end: right.1,
                        }));
                        work.push_front(ChunkWorkItem::Task(ChunkTask {
                            start: left.0,
                            end: left.1,
                        }));
                        post_materialization_splits = post_materialization_splits
                            .checked_add(1)
                            .ok_or_else(|| invalid_data("record payload split count overflow"))?;
                        adaptive_splits = adaptive_splits
                            .checked_add(1)
                            .ok_or_else(|| invalid_data("adaptive split count overflow"))?;
                        largest_rejected_parent_bytes =
                            largest_rejected_parent_bytes.max(estimated_bytes);
                    }
                }
            }
        }
        flush_pending_chunks(
            &mut pending,
            &mut archive_temp.file,
            &mut manifests,
            &mut bucket_entries,
            bucket_span,
            config,
            options,
            started,
            processing_started,
            total_reference_bases,
            references.len(),
            &mut writer,
        )?;
        pending_memory_bound = 0;
    }

    flush_pending_chunks(
        &mut pending,
        &mut archive_temp.file,
        &mut manifests,
        &mut bucket_entries,
        bucket_span,
        config,
        options,
        started,
        processing_started,
        total_reference_bases,
        references.len(),
        &mut writer,
    )?;
    let payload_pipeline_wall_ms = processing_started.elapsed().as_secs_f64() * 1_000.0;

    let directory_entries = manifests.iter().try_fold(0_u64, |total, manifest| {
        total
            .checked_add(manifest.entry_count)
            .ok_or_else(|| invalid_data("directory entry count overflow"))
    })?;
    emit_progress(
        options.progress,
        "writer_finalization",
        "backfilling directory pages and final header",
    );
    let finalization_started = Instant::now();
    let root = encode_root_index(&manifests)?;
    archive_temp.file.seek(SeekFrom::Start(0))?;
    archive_temp.file.write_all(&encode_header(
        usize_to_u64(root.len())?,
        directory_entries,
        data_offset,
    ))?;
    archive_temp.file.write_all(&root)?;
    for (reference_id, manifest) in manifests.iter().enumerate() {
        for bucket_index in 0..manifest.page_count {
            let bucket_start = manifest
                .grid_start
                .checked_add(
                    bucket_index
                        .checked_mul(manifest.bucket_span)
                        .ok_or_else(|| invalid_data("directory bucket offset overflow"))?,
                )
                .ok_or_else(|| invalid_data("directory bucket coordinate overflow"))?;
            let entries = &bucket_entries[reference_id][u64_to_usize(bucket_index)?];
            let page = encode_directory_page(entries, bucket_start)?;
            archive_temp
                .file
                .seek(SeekFrom::Start(directory_page_offset(
                    manifest,
                    bucket_index,
                )?))?;
            archive_temp.file.write_all(&page)?;
        }
    }
    archive_temp.file.sync_all()?;
    let writer_finalization_wall_ms = finalization_started.elapsed().as_secs_f64() * 1_000.0;
    let archive_bytes = archive_temp.file.metadata()?.len();
    emit_progress(
        options.progress,
        "archive_validation",
        "validating all directory entries and physical payloads",
    );
    let validation_started = Instant::now();
    let _validation = validate_fixed_archive(&archive_temp.path)?;
    let archive_validation_wall_ms = validation_started.elapsed().as_secs_f64() * 1_000.0;
    archive_temp.persist(output)?;

    let mut chunk_sizes = writer
        .chunks
        .iter()
        .map(|chunk| chunk.compressed_len)
        .collect::<Vec<_>>();
    let max_uncompressed_chunk_bytes = writer
        .chunks
        .iter()
        .map(|chunk| chunk.uncompressed_len)
        .max()
        .unwrap_or(0);
    chunk_sizes.sort_unstable();
    let index_bytes = data_offset;
    let root_index_bytes = usize_to_u64(HEADER_LEN + root.len())?;
    let physical_chunks = usize_to_u64(writer.chunks.len())?;
    let directory_pages = manifests.iter().map(|manifest| manifest.page_count).sum();
    Ok(ArchiveBuildMetrics {
        experiment_id: config.experiment_id.clone(),
        source_gbz_bytes,
        archive_bytes,
        expansion_ratio: ratio(archive_bytes, source_gbz_bytes),
        index_bytes,
        index_ratio: ratio(index_bytes, archive_bytes),
        root_index_bytes,
        directory_pages,
        directory_entries,
        physical_chunks,
        deduplicated_entries: directory_entries.saturating_sub(physical_chunks),
        duplicate_payload_entries_observed: writer.duplicate_payload_entries_observed,
        avoidable_compressed_payload_bytes: writer.avoidable_compressed_payload_bytes,
        mean_chunk_bytes: mean(&chunk_sizes),
        median_chunk_bytes: percentile_u64(&chunk_sizes, 0.5),
        p95_chunk_bytes: percentile_u64(&chunk_sizes, 0.95),
        max_chunk_bytes: chunk_sizes.last().copied().unwrap_or(0),
        max_uncompressed_chunk_bytes,
        peak_raw_chunk_bytes,
        peak_compressed_chunk_bytes: writer.peak_compressed_chunk_bytes,
        payload_spool_bytes: 0,
        path_occurrence_index_bytes: 0,
        path_occurrence_index_wall_ms: 0.0,
        reference_manifest_discovery_wall_ms,
        subgraph_selection_wall_ms,
        local_haplotype_extraction_wall_ms: 0.0,
        regional_materialization_wall_ms,
        regional_encoding_wall_ms,
        payload_pipeline_wall_ms,
        compression_wall_ms: writer.compression_wall_ms,
        preflight_selection_wall_ms,
        writer_finalization_wall_ms,
        archive_validation_wall_ms,
        final_copy_wall_ms: 0.0,
        adaptive_splits,
        preflight_splits,
        post_materialization_splits,
        largest_rejected_parent_bytes,
        first_payload_wall_ms: writer.first_payload_wall_ms.unwrap_or(0.0),
        references_processed: usize_to_u64(references.len())?,
        total_reference_bases,
        reference_bases_processed: writer.reference_bases_processed,
        progress_events_emitted: writer.progress_events_emitted,
        scratch_bytes: 0,
        scratch_bytes_before_first_payload: 0,
        temporary_file_bytes_before_first_payload: data_offset,
        temporary_file_peak_bytes: archive_bytes,
        payload_bytes: archive_bytes.saturating_sub(data_offset),
        peak_queued_raw_bytes: writer.peak_queued_raw_bytes,
        peak_queued_compressed_bytes: writer.peak_queued_compressed_bytes,
        peak_queued_total_bytes: writer.peak_queued_total_bytes,
        peak_ready_raw_bytes,
        haplotype_extraction_evidence,
        construction_wall_ms: started.elapsed().as_secs_f64() * 1_000.0,
    })
}

fn adaptive_split_interval(
    start: u64,
    end: u64,
    raw_bytes: u64,
    max_raw_bytes: u64,
    min_window_size: u64,
) -> io::Result<Option<[(u64, u64); 2]>> {
    if raw_bytes <= max_raw_bytes {
        return Ok(None);
    }
    let width = end.saturating_sub(start);
    if width <= min_window_size {
        return Err(invalid_data(format!(
            "regional payload for {start}-{end} is {raw_bytes} bytes, above the {max_raw_bytes} byte cap at the minimum {min_window_size} bp window"
        )));
    }
    let midpoint = start + width / 2;
    if midpoint == start || midpoint == end {
        return Err(invalid_data("adaptive chunk split made no progress"));
    }
    Ok(Some([(start, midpoint), (midpoint, end)]))
}

/// Extracts the canonical source result used to verify an archive query.
///
/// # Errors
///
/// Returns an error when the query is invalid or the requested source region
/// cannot be extracted or encoded canonically.
pub fn source_oracle(
    graph: &GBZ,
    path_index: &PathIndex,
    query: &QuerySpec,
) -> ExperimentResult<OracleResult> {
    query.validate()?;
    let query_name = FullPathName::reference(&query.sample, &query.contig);
    let (regional, _) = extract_local_region(
        graph,
        path_index,
        &query_name,
        query.start,
        query.end,
        query.context,
        HaplotypeOutput::Distinct,
    )?;
    let canonical_ids = regional.nodes.keys().copied().collect::<BTreeSet<_>>();
    let canonical = regional.canonical(&canonical_ids, query)?;
    let encoded = encode_canonical(&canonical)?;
    Ok(OracleResult { canonical, encoded })
}

#[allow(clippy::too_many_lines)]
/// Executes a cold one-shot query by opening a reusable reader internally.
///
/// # Errors
///
/// Returns an error for invalid coordinates, malformed archive data, failed
/// range reads, decompression/decoding failures, or an oracle mismatch.
pub fn query_fixed_archive(
    archive: &Path,
    config: &FixedArchiveConfig,
    query: &QuerySpec,
    coalescing_gap: u64,
    oracle: &OracleResult,
    graph: &GBZ,
    path_index: &PathIndex,
) -> ExperimentResult<QueryMeasurement> {
    let mut reader = FixedArchiveReader::open(archive)?;
    reader.query(
        config,
        query,
        coalescing_gap,
        oracle,
        Some((graph, path_index)),
    )
}

impl FixedArchiveReader {
    /// Opens an archive and reads its bootstrap once.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be opened or its header, root,
    /// or offsets fail validation.
    pub fn open(archive: &Path) -> ExperimentResult<Self> {
        let source = TracingRangeSource::new(FileRangeSource::open(archive)?);
        let bootstrap = load_bootstrap(&source)?;
        Ok(Self {
            source,
            bootstrap,
            directory_cache: HashMap::new(),
            directory_cache_bytes: 0,
            directory_cache_limit: DEFAULT_DIRECTORY_CACHE_BYTES,
            cache_clock: 0,
            first_query: true,
        })
    }

    /// Changes the leaf-directory cache budget. A zero budget disables it.
    #[must_use]
    pub fn with_directory_cache_bytes(mut self, bytes: usize) -> Self {
        self.directory_cache_limit = bytes;
        self.evict_directory_cache();
        self
    }

    /// Drops all reusable leaf-directory state while keeping the file open.
    pub fn clear_directory_cache(&mut self) {
        self.directory_cache.clear();
        self.directory_cache_bytes = 0;
    }

    /// Executes a query while reusing the open source and cached leaf pages.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid coordinates, failed range reads, malformed
    /// directory/payload data, decompression failure, or an oracle mismatch.
    #[allow(clippy::too_many_lines)]
    pub fn query(
        &mut self,
        config: &FixedArchiveConfig,
        query: &QuerySpec,
        coalescing_gap: u64,
        oracle: &OracleResult,
        oracle_source: Option<(&GBZ, &PathIndex)>,
    ) -> ExperimentResult<QueryMeasurement> {
        query.validate()?;
        let includes_bootstrap = self.first_query;
        if !includes_bootstrap {
            self.source.clear();
        }
        self.first_query = false;
        let total_started = Instant::now();

        let lookup_started = Instant::now();
        let DirectoryLookup {
            entries,
            logical_bytes: logical_index_bytes,
            fetched_bytes: directory_page_bytes_fetched,
            fetched_ranges: directory_page_ranges_fetched,
            selected_pages: directory_pages_selected,
            cache_hits: directory_page_cache_hits,
        } = self.lookup_directory_cached(query)?;
        let index_lookup_us = lookup_started.elapsed().as_secs_f64() * 1_000_000.0;
        let source = &self.source;
        let bootstrap = &self.bootstrap;

        let bootstrap_end = usize_to_u64(bootstrap.bytes.len())?;
        let mut needed_ranges = BTreeSet::new();
        for entry in &entries {
            let end = entry
                .offset
                .checked_add(entry.compressed_len)
                .ok_or_else(|| invalid_data("chunk range overflow"))?;
            let start = entry.offset.max(bootstrap_end);
            if start < end {
                needed_ranges.insert(ByteRange { start, end });
            }
        }
        let planned_ranges = coalesce_ranges(needed_ranges.into_iter().collect(), coalescing_gap);
        let mut fetched = Vec::with_capacity(planned_ranges.len());
        for range in &planned_ranges {
            let bytes = source.read_range(range.start, u64_to_usize(range.len())?)?;
            fetched.push((*range, bytes));
        }

        let mut regional: Option<RegionalGraph> = None;
        let mut decoded_chunks = BTreeSet::new();
        let mut decompression_us = 0.0;
        let mut decode_us = 0.0;
        let mut haplotype_tiles_checked = 0_u64;
        let mut haplotype_tiles_correct = true;
        for entry in &entries {
            if !decoded_chunks.insert((entry.offset, entry.compressed_len)) {
                continue;
            }
            let compressed = collect_chunk(entry, &bootstrap.bytes, &fetched)?;
            let decompress_started = Instant::now();
            let raw = decompress(entry.codec, &compressed, entry.uncompressed_len)?;
            decompression_us += decompress_started.elapsed().as_secs_f64() * 1_000_000.0;
            let decode_started = Instant::now();
            let chunk = RegionalGraph::decode(&raw)?;
            decode_us += decode_started.elapsed().as_secs_f64() * 1_000_000.0;
            if let Some((graph, path_index)) = oracle_source {
                let tile = chunk
                    .haplotype_tiles
                    .first()
                    .ok_or_else(|| invalid_data("v4 query chunk has no haplotype tile"))?;
                if tile.core_start != entry.start || tile.core_end != entry.end {
                    return Err(invalid_data(
                        "v4 tile provenance does not match its directory entry",
                    )
                    .into());
                }
                let reference = chunk
                    .reference_paths
                    .first()
                    .ok_or_else(|| invalid_data("v4 query chunk has no reference path"))?;
                let name = FullPathName {
                    sample: reference.sample.clone(),
                    contig: reference.contig.clone(),
                    haplotype: u64_to_usize(reference.haplotype)?,
                    fragment: 0,
                };
                let (expected, _) = extract_local_region(
                    graph,
                    path_index,
                    &name,
                    tile.core_start,
                    tile.core_end,
                    CONSTRUCTION_CONTEXT,
                    HaplotypeOutput::Distinct,
                )?;
                let expected_tile = &expected.haplotype_tiles[0];
                let tile_correct = tile.normalized() == expected_tile.normalized()
                    && chunk.semantics == HaplotypeSemantics::AnonymousDistinctWeightedTilePaths;
                haplotype_tiles_checked = haplotype_tiles_checked
                    .checked_add(1)
                    .ok_or_else(|| invalid_data("tile correctness count overflow"))?;
                haplotype_tiles_correct &= tile_correct;
                if !tile_correct {
                    return Err(invalid_data(format!(
                        "weighted tile semantics differ for {}#{}:{}-{}",
                        tile.reference_sample,
                        tile.reference_contig,
                        tile.core_start,
                        tile.core_end
                    ))
                    .into());
                }
            }
            if let Some(existing) = &mut regional {
                existing.merge(chunk)?;
            } else {
                regional = Some(chunk);
            }
        }
        let regional = regional.ok_or_else(|| invalid_data("query selected no regional chunks"))?;

        let reconstruction_started = Instant::now();
        let selected = regional.select_nodes(query)?;
        let canonical = regional.canonical(&selected, query)?;
        let graph_reconstruction_us = reconstruction_started.elapsed().as_secs_f64() * 1_000_000.0;
        let total_local_query_us = total_started.elapsed().as_secs_f64() * 1_000_000.0;
        let correctness = canonical.equivalent_to(&oracle.canonical);
        if !correctness {
            return Err(invalid_data(format!(
                "candidate semantics differ from the source oracle for query {}: {}",
                query.id,
                canonical_mismatch_summary(&canonical, &oracle.canonical)
            ))
            .into());
        }
        let canonical_hash = canonical.canonical_hash().to_hex().to_string();
        let required_compressed = compress(config.codec, &oracle.encoded)?;
        let trace = source.summary();
        let data_bytes_fetched = planned_ranges.iter().map(|range| range.len()).sum::<u64>();
        let logical_data_round = u64::from(!planned_ranges.is_empty());
        let logical_directory_round = u64::from(directory_page_ranges_fetched > 0);
        let bootstrap_dependency_rounds = if includes_bootstrap {
            bootstrap.dependency_rounds
        } else {
            0
        };
        let dependency_rounds =
            bootstrap_dependency_rounds + logical_directory_round + logical_data_round;
        let required_compressed_payload_bytes = usize_to_u64(required_compressed.len())?;
        let canonical_payload_bytes = usize_to_u64(oracle.encoded.len())?;
        let mut dependency_groups = vec![1_u64; u64_to_usize(bootstrap_dependency_rounds)?];
        if directory_page_ranges_fetched > 0 {
            dependency_groups.push(directory_page_ranges_fetched);
        }
        if !planned_ranges.is_empty() {
            dependency_groups.push(usize_to_u64(planned_ranges.len())?);
        }
        let simulated_20ms_ms = NetworkProfile::GOOD_CDN
            .estimate_dependency_groups(&dependency_groups, trace.total_bytes_requested)
            .estimated_total_ms;
        let simulated_50ms_ms = NetworkProfile::MODERATE_INTERNET
            .estimate_dependency_groups(&dependency_groups, trace.total_bytes_requested)
            .estimated_total_ms;
        let simulated_100ms_ms = NetworkProfile::POOR_MOBILE
            .estimate_dependency_groups(&dependency_groups, trace.total_bytes_requested)
            .estimated_total_ms;
        Ok(QueryMeasurement {
            experiment_id: config.experiment_id.clone(),
            query_id: query.id.clone(),
            query_class: query.class.clone(),
            query_size: query.length(),
            coalescing_gap,
            physical_reads: trace.read_operations,
            mergeable_reads: trace.mergeable_reads,
            dependency_rounds,
            total_bytes_fetched: trace.total_bytes_requested,
            unique_bytes_fetched: trace.unique_bytes_requested,
            duplicate_bytes_fetched: trace.duplicate_bytes_requested,
            bootstrap_bytes_fetched: if includes_bootstrap {
                usize_to_u64(bootstrap.bytes.len())?
            } else {
                0
            },
            logical_index_bytes,
            directory_page_bytes_fetched,
            directory_pages_selected,
            directory_page_cache_hits,
            data_bytes_fetched,
            required_compressed_payload_bytes,
            canonical_payload_bytes,
            read_amplification: ratio(
                trace.total_bytes_requested,
                required_compressed_payload_bytes,
            ),
            canonical_amplification: ratio(trace.total_bytes_requested, canonical_payload_bytes),
            index_lookup_us,
            decompression_us,
            decode_us,
            graph_reconstruction_us,
            total_local_query_us,
            selected_chunks: usize_to_u64(entries.len())?,
            selected_nodes: usize_to_u64(selected.len())?,
            canonical_hash,
            correctness,
            haplotype_tiles_checked,
            haplotype_tiles_correct,
            simulated_20ms_ms,
            simulated_50ms_ms,
            simulated_100ms_ms,
        })
    }
}

impl FixedArchiveReader {
    #[allow(clippy::too_many_lines)]
    fn lookup_directory_cached(&mut self, query: &QuerySpec) -> ExperimentResult<DirectoryLookup> {
        let manifests = self
            .bootstrap
            .root
            .manifests
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, manifest)| {
                manifest.sample == query.sample
                    && manifest.contig == query.contig
                    && manifest.start < query.end
                    && manifest.end > query.start
            })
            .collect::<Vec<_>>();
        if manifests.is_empty() {
            return Err(
                invalid_data(format!("no reference manifest covers query {}", query.id)).into(),
            );
        }

        let mut selected_pages = Vec::new();
        for (manifest_id, manifest) in manifests {
            let start = query.start.max(manifest.start);
            let end = query.end.min(manifest.end);
            let first_bucket = start
                .checked_sub(manifest.grid_start)
                .ok_or_else(|| invalid_data("query starts before reference grid"))?
                / manifest.bucket_span;
            let last_bucket = end
                .checked_sub(1)
                .and_then(|value| value.checked_sub(manifest.grid_start))
                .ok_or_else(|| invalid_data("query ends before reference grid"))?
                / manifest.bucket_span;
            if last_bucket >= manifest.page_count {
                return Err(invalid_data("query directory bucket exceeds manifest").into());
            }
            for bucket_index in first_bucket..=last_bucket {
                selected_pages.push((manifest_id, manifest.clone(), bucket_index));
            }
        }

        let mut page_entries = HashMap::new();
        let mut missing_pages = Vec::new();
        let mut cache_hits = 0_u64;
        for (manifest_id, manifest, bucket_index) in &selected_pages {
            let key = (*manifest_id, *bucket_index);
            self.cache_clock = self.cache_clock.wrapping_add(1);
            if let Some(cached) = self.directory_cache.get_mut(&key) {
                cached.last_used = self.cache_clock;
                page_entries.insert(key, cached.entries.clone());
                cache_hits += 1;
            } else {
                missing_pages.push((key, manifest.clone(), *bucket_index));
            }
        }

        let bootstrap_end = usize_to_u64(self.bootstrap.bytes.len())?;
        let page_bytes = usize_to_u64(DIRECTORY_PAGE_BYTES)?;
        let mut needed_ranges = BTreeSet::new();
        for (_, manifest, bucket_index) in &missing_pages {
            let page_offset = directory_page_offset(manifest, *bucket_index)?;
            let page_end = page_offset
                .checked_add(page_bytes)
                .ok_or_else(|| invalid_data("directory page range overflow"))?;
            let start = page_offset.max(bootstrap_end);
            if start < page_end {
                needed_ranges.insert(ByteRange {
                    start,
                    end: page_end,
                });
            }
        }
        let planned_ranges = coalesce_ranges(needed_ranges.into_iter().collect(), 0);
        let mut fetched = Vec::with_capacity(planned_ranges.len());
        for range in &planned_ranges {
            let bytes = self
                .source
                .read_range(range.start, u64_to_usize(range.len())?)?;
            fetched.push((*range, bytes));
        }

        for (key, manifest, bucket_index) in missing_pages {
            let page_offset = directory_page_offset(&manifest, bucket_index)?;
            let bytes = collect_stored_range(
                ByteRange {
                    start: page_offset,
                    end: page_offset
                        .checked_add(page_bytes)
                        .ok_or_else(|| invalid_data("directory page range overflow"))?,
                },
                &self.bootstrap.bytes,
                &fetched,
            )?;
            let entries = decode_directory_page(&bytes, &manifest, bucket_index)?.entries;
            page_entries.insert(key, entries.clone());
            self.insert_directory_cache(key, entries);
        }

        let mut entries = Vec::new();
        for (manifest_id, _, bucket_index) in &selected_pages {
            let page = page_entries
                .get(&(*manifest_id, *bucket_index))
                .ok_or_else(|| invalid_data("selected directory page was not decoded"))?;
            entries.extend(
                page.iter()
                    .filter(|entry| entry.start < query.end && entry.end > query.start)
                    .cloned(),
            );
        }
        if entries.is_empty() {
            return Err(invalid_data(format!("no chunks cover query {}", query.id)).into());
        }

        let logical_bytes = self
            .bootstrap
            .root
            .logical_bytes
            .checked_add(
                usize_to_u64(selected_pages.len())?
                    .checked_mul(page_bytes)
                    .ok_or_else(|| invalid_data("logical directory byte count overflow"))?,
            )
            .ok_or_else(|| invalid_data("logical directory byte count overflow"))?;
        Ok(DirectoryLookup {
            entries,
            logical_bytes,
            fetched_bytes: planned_ranges.iter().map(|range| range.len()).sum(),
            fetched_ranges: usize_to_u64(planned_ranges.len())?,
            selected_pages: usize_to_u64(selected_pages.len())?,
            cache_hits,
        })
    }

    fn insert_directory_cache(&mut self, key: (usize, u64), entries: Vec<ArchiveEntry>) {
        if self.directory_cache_limit < DIRECTORY_PAGE_BYTES {
            return;
        }
        self.cache_clock = self.cache_clock.wrapping_add(1);
        if self
            .directory_cache
            .insert(
                key,
                CachedDirectoryPage {
                    entries,
                    last_used: self.cache_clock,
                },
            )
            .is_none()
        {
            self.directory_cache_bytes = self
                .directory_cache_bytes
                .saturating_add(DIRECTORY_PAGE_BYTES);
        }
        self.evict_directory_cache();
    }

    fn evict_directory_cache(&mut self) {
        while self.directory_cache_bytes > self.directory_cache_limit {
            let Some((&oldest, _)) = self
                .directory_cache
                .iter()
                .min_by_key(|(_, page)| page.last_used)
            else {
                self.directory_cache_bytes = 0;
                break;
            };
            self.directory_cache.remove(&oldest);
            self.directory_cache_bytes = self
                .directory_cache_bytes
                .saturating_sub(DIRECTORY_PAGE_BYTES);
        }
    }
}

impl RegionalGraph {
    fn topology_from_subgraph(subgraph: &Subgraph) -> ExperimentResult<Self> {
        let mut result = Self::default();
        for node_id in subgraph.node_iter() {
            let sequence = subgraph.sequence(node_id).ok_or_else(|| {
                invalid_data(format!("missing local sequence for node {node_id}"))
            })?;
            result
                .nodes
                .insert(usize_to_u64(node_id)?, sequence.to_vec());
            for orientation in [Orientation::Forward, Orientation::Reverse] {
                let successors = subgraph
                    .supergraph_successors(node_id, orientation)
                    .ok_or_else(|| invalid_data(format!("missing local node {node_id}")))?;
                for (next_id, next_orientation) in successors {
                    if support::edge_is_canonical(
                        (node_id, orientation),
                        (next_id, next_orientation),
                    ) {
                        result.edges.insert(Edge {
                            from: oriented(node_id, orientation)?,
                            to: oriented(next_id, next_orientation)?,
                        });
                    }
                }
            }
        }
        Ok(result)
    }

    fn merge(&mut self, other: Self) -> io::Result<()> {
        if self.semantics != other.semantics {
            return Err(invalid_data(
                "cannot merge regional payloads with different semantics",
            ));
        }
        for (id, sequence) in other.nodes {
            if let Some(existing) = self.nodes.get(&id) {
                if existing != &sequence {
                    return Err(invalid_data(format!("conflicting sequence for node {id}")));
                }
            } else {
                self.nodes.insert(id, sequence);
            }
        }
        self.edges.extend(other.edges);
        self.reference_paths.extend(other.reference_paths);
        self.haplotype_tiles.extend(other.haplotype_tiles);
        self.reference_visits.extend(other.reference_visits);
        for (path_id, path) in other.paths {
            if let Some(existing) = self.paths.get_mut(&path_id) {
                if existing.sample != path.sample
                    || existing.contig != path.contig
                    || existing.haplotype != path.haplotype
                    || existing.fragment != path.fragment
                {
                    return Err(invalid_data(format!(
                        "conflicting metadata for path {path_id}"
                    )));
                }
                for (index, node) in path.visits {
                    if let Some(previous) = existing.visits.insert(index, node)
                        && previous != node
                    {
                        return Err(invalid_data(format!(
                            "conflicting visit {index} for path {path_id}"
                        )));
                    }
                }
            } else {
                self.paths.insert(path_id, path);
            }
        }
        Ok(())
    }

    fn select_nodes(&self, query: &QuerySpec) -> ExperimentResult<BTreeSet<u64>> {
        if !self.reference_paths.is_empty() {
            return self.select_nodes_v4(query);
        }
        let path_ids = self
            .paths
            .values()
            .filter(|path| path.sample == query.sample && path.contig == query.contig)
            .map(|path| path.path_id)
            .collect::<BTreeSet<_>>();
        if path_ids.is_empty() {
            return Err(invalid_data(format!(
                "reference path {}#{} is absent from fetched chunks",
                query.sample, query.contig
            ))
            .into());
        }

        let mut active: BinaryHeap<Reverse<(u64, (u64, bool))>> = BinaryHeap::new();
        for visit in self.reference_visits.iter().filter(|visit| {
            path_ids.contains(&visit.path_id) && visit.start < query.end && visit.end > query.start
        }) {
            let overlap_start = visit.start.max(query.start);
            let overlap_end = visit.end.min(query.end);
            let offset = overlap_start.saturating_sub(visit.start);
            let entry_is_right = visit.node.reverse;
            active.push(Reverse((offset, (visit.node.id, entry_is_right))));
            let end_distance = if overlap_end == visit.end {
                0
            } else {
                visit.end.saturating_sub(overlap_end).saturating_sub(1)
            };
            active.push(Reverse((end_distance, (visit.node.id, !entry_is_right))));
        }
        if active.is_empty() {
            return Err(
                invalid_data(format!("no reference visits overlap query {}", query.id)).into(),
            );
        }

        let adjacency = self.adjacency();
        let mut visited_sides = BTreeSet::new();
        let mut selected = BTreeSet::new();
        while let Some(Reverse((distance, side))) = active.pop() {
            if !visited_sides.insert(side) {
                continue;
            }
            selected.insert(side.0);
            let other = (side.0, !side.1);
            if !visited_sides.contains(&other) {
                let node_len = self
                    .nodes
                    .get(&side.0)
                    .ok_or_else(|| invalid_data(format!("missing node {}", side.0)))?
                    .len();
                let next_distance = distance
                    .checked_add(usize_to_u64(node_len)?.saturating_sub(1))
                    .ok_or_else(|| invalid_data("context distance overflow"))?;
                if next_distance <= query.context {
                    active.push(Reverse((next_distance, other)));
                }
            }

            let edge_distance = distance.saturating_add(1);
            if edge_distance <= query.context {
                let exit_orientation_reverse = !side.1;
                let handle = OrientedNode {
                    id: side.0,
                    reverse: exit_orientation_reverse,
                };
                if let Some(successors) = adjacency.get(&handle) {
                    for successor in successors {
                        let next_side = (successor.id, successor.reverse);
                        if !visited_sides.contains(&next_side) {
                            active.push(Reverse((edge_distance, next_side)));
                        }
                    }
                }
            }
        }
        Ok(selected)
    }

    fn select_nodes_v4(&self, query: &QuerySpec) -> ExperimentResult<BTreeSet<u64>> {
        let mut active: BinaryHeap<Reverse<(u64, (u64, bool))>> = BinaryHeap::new();
        for path in self.reference_paths.iter().filter(|path| {
            path.sample == query.sample
                && path.contig == query.contig
                && path.start < query.end
                && path.end > query.start
        }) {
            let mut coordinate = path.start;
            for &node in &path.traversal {
                let node_len = usize_to_u64(
                    self.nodes
                        .get(&node.id)
                        .ok_or_else(|| invalid_data(format!("missing node {}", node.id)))?
                        .len(),
                )?;
                let visit_end = coordinate
                    .checked_add(node_len)
                    .ok_or_else(|| invalid_data("reference visit coordinate overflow"))?;
                if coordinate < query.end && visit_end > query.start {
                    let overlap_start = coordinate.max(query.start);
                    let overlap_end = visit_end.min(query.end);
                    let offset = overlap_start.saturating_sub(coordinate);
                    active.push(Reverse((offset, (node.id, node.reverse))));
                    let end_distance = if overlap_end == visit_end {
                        0
                    } else {
                        visit_end.saturating_sub(overlap_end).saturating_sub(1)
                    };
                    active.push(Reverse((end_distance, (node.id, !node.reverse))));
                }
                coordinate = visit_end;
            }
            if coordinate != path.end {
                return Err(
                    invalid_data("reference traversal length does not match its interval").into(),
                );
            }
        }
        if active.is_empty() {
            return Err(invalid_data(format!(
                "no v4 reference traversal overlaps query {}",
                query.id
            ))
            .into());
        }
        self.expand_context(active, query.context)
    }

    fn expand_context(
        &self,
        mut active: BinaryHeap<Reverse<(u64, (u64, bool))>>,
        context: u64,
    ) -> ExperimentResult<BTreeSet<u64>> {
        let adjacency = self.adjacency();
        let mut visited_sides = BTreeSet::new();
        let mut selected = BTreeSet::new();
        while let Some(Reverse((distance, side))) = active.pop() {
            if !visited_sides.insert(side) {
                continue;
            }
            selected.insert(side.0);
            let other = (side.0, !side.1);
            if !visited_sides.contains(&other) {
                let node_len = self
                    .nodes
                    .get(&side.0)
                    .ok_or_else(|| invalid_data(format!("missing node {}", side.0)))?
                    .len();
                let next_distance = distance
                    .checked_add(usize_to_u64(node_len)?.saturating_sub(1))
                    .ok_or_else(|| invalid_data("context distance overflow"))?;
                if next_distance <= context {
                    active.push(Reverse((next_distance, other)));
                }
            }
            let edge_distance = distance.saturating_add(1);
            if edge_distance <= context {
                let handle = OrientedNode {
                    id: side.0,
                    reverse: !side.1,
                };
                if let Some(successors) = adjacency.get(&handle) {
                    for successor in successors {
                        let next_side = (successor.id, successor.reverse);
                        if !visited_sides.contains(&next_side) {
                            active.push(Reverse((edge_distance, next_side)));
                        }
                    }
                }
            }
        }
        Ok(selected)
    }

    fn adjacency(&self) -> BTreeMap<OrientedNode, BTreeSet<OrientedNode>> {
        let mut result: BTreeMap<OrientedNode, BTreeSet<OrientedNode>> = BTreeMap::new();
        for edge in &self.edges {
            // A regional chunk retains boundary edges even when the neighboring
            // endpoint belongs to another chunk. Activate the edge for context
            // traversal only after both endpoint payloads have been assembled.
            if !self.nodes.contains_key(&edge.from.id) || !self.nodes.contains_key(&edge.to.id) {
                continue;
            }
            result.entry(edge.from).or_default().insert(edge.to);
            result
                .entry(flip(edge.to))
                .or_default()
                .insert(flip(edge.from));
        }
        result
    }

    fn canonical(
        &self,
        selected: &BTreeSet<u64>,
        query: &QuerySpec,
    ) -> ExperimentResult<CanonicalSubgraph> {
        if !self.reference_paths.is_empty() {
            return self.canonical_v4(selected, query);
        }
        let mut result = CanonicalSubgraph::default();
        for &node_id in selected {
            let sequence = self
                .nodes
                .get(&node_id)
                .ok_or_else(|| invalid_data(format!("selected node {node_id} is absent")))?;
            result.nodes.insert(node_id, sequence.clone());
        }
        result.edges.extend(
            self.edges
                .iter()
                .filter(|edge| selected.contains(&edge.from.id) && selected.contains(&edge.to.id))
                .copied(),
        );

        for path in self.paths.values() {
            let mut segment = Vec::new();
            let mut previous_index = None;
            for (&index, &node) in &path.visits {
                if !selected.contains(&node.id) {
                    if !segment.is_empty() {
                        push_path_segment(&mut result.paths, path, std::mem::take(&mut segment));
                    }
                    previous_index = None;
                    continue;
                }
                if let Some(previous) = previous_index
                    && index != previous + 1
                    && !segment.is_empty()
                {
                    push_path_segment(&mut result.paths, path, std::mem::take(&mut segment));
                }
                segment.push(node);
                previous_index = Some(index);
            }
            if !segment.is_empty() {
                push_path_segment(&mut result.paths, path, segment);
            }
        }
        result.reference_intervals.insert(ReferenceInterval {
            sample: query.sample.clone(),
            contig: query.contig.clone(),
            start: query.start,
            end: query.end,
        });
        Ok(result)
    }

    fn canonical_v4(
        &self,
        selected: &BTreeSet<u64>,
        query: &QuerySpec,
    ) -> ExperimentResult<CanonicalSubgraph> {
        let mut result = CanonicalSubgraph::default();
        for &node_id in selected {
            result.nodes.insert(
                node_id,
                self.nodes
                    .get(&node_id)
                    .ok_or_else(|| invalid_data(format!("selected node {node_id} is absent")))?
                    .clone(),
            );
        }
        result.edges.extend(
            self.edges
                .iter()
                .filter(|edge| selected.contains(&edge.from.id) && selected.contains(&edge.to.id))
                .copied(),
        );
        let mut visits = BTreeMap::new();
        for path in self
            .reference_paths
            .iter()
            .filter(|path| path.sample == query.sample && path.contig == query.contig)
        {
            let mut coordinate = path.start;
            for &node in &path.traversal {
                let end = coordinate
                    .checked_add(usize_to_u64(
                        self.nodes
                            .get(&node.id)
                            .ok_or_else(|| invalid_data(format!("missing node {}", node.id)))?
                            .len(),
                    )?)
                    .ok_or_else(|| invalid_data("reference coordinate overflow"))?;
                let previous = visits.insert(coordinate, (end, node));
                if previous.is_some_and(|previous| previous != (end, node)) {
                    return Err(
                        invalid_data("conflicting reference visits at one coordinate").into(),
                    );
                }
                coordinate = end;
            }
        }
        let visits = visits.into_iter().collect::<Vec<_>>();
        let first_core = visits
            .iter()
            .position(|(start, (end, _))| *start < query.end && *end > query.start)
            .ok_or_else(|| invalid_data("reference walk does not overlap query"))?;
        let last_core = visits
            .iter()
            .rposition(|(start, (end, _))| *start < query.end && *end > query.start)
            .ok_or_else(|| invalid_data("reference walk does not overlap query"))?;
        let mut first = first_core;
        while first > 0 && selected.contains(&visits[first - 1].1.1.id) {
            first -= 1;
        }
        let mut last = last_core;
        while last + 1 < visits.len() && selected.contains(&visits[last + 1].1.1.id) {
            last += 1;
        }
        let traversal = visits[first..=last]
            .iter()
            .map(|(_, (_, node))| *node)
            .collect::<Vec<_>>();
        if traversal.is_empty() {
            return Err(invalid_data("selected graph has no reference traversal").into());
        }
        result.paths.push(CanonicalPath {
            sample: query.sample.clone(),
            contig: query.contig.clone(),
            haplotype: 0,
            fragment: query.start,
            is_reference: true,
            traversal,
        });
        result.reference_intervals.insert(ReferenceInterval {
            sample: query.sample.clone(),
            contig: query.contig.clone(),
            start: query.start,
            end: query.end,
        });
        Ok(result)
    }

    fn encode(&self) -> ExperimentResult<Vec<u8>> {
        if self.reference_paths.is_empty() {
            return self.encode_v3();
        }
        if self.reference_paths.len() != 1 || self.haplotype_tiles.len() != 1 {
            return Err(
                invalid_data("v4 payload must contain exactly one reference and tile").into(),
            );
        }
        let reference = &self.reference_paths[0];
        let tile = &self.haplotype_tiles[0];
        if reference.sample != tile.reference_sample || reference.contig != tile.reference_contig {
            return Err(invalid_data("v4 reference and tile provenance differ").into());
        }
        if traversal_end(&self.nodes, reference.start, &reference.traversal)? != reference.end {
            return Err(
                invalid_data("v4 reference traversal length does not match its interval").into(),
            );
        }
        let mut output = Vec::new();
        output.extend_from_slice(REGION_MAGIC_WEIGHTED_V3);
        put_u32(&mut output, 3);
        put_u32(&mut output, 1);
        output.push(semantics_code(self.semantics));
        output.extend_from_slice(&[0_u8; 7]);
        put_u64(&mut output, usize_to_u64(self.nodes.len())?);
        put_u64(&mut output, usize_to_u64(self.edges.len())?);
        put_u64(&mut output, tile.core_start);
        put_u64(&mut output, tile.core_end);
        put_u64(&mut output, CONSTRUCTION_CONTEXT);
        put_u64(&mut output, reference.start);
        put_u64(&mut output, reference.end);
        put_u64(&mut output, reference.haplotype);
        put_u64(&mut output, usize_to_u64(reference.traversal.len())?);
        put_u64(&mut output, usize_to_u64(tile.traversals.len())?);
        put_string(&mut output, &reference.sample)?;
        put_string(&mut output, &reference.contig)?;
        let mut previous_node_id = 0_u64;
        for (id, sequence) in &self.nodes {
            put_u64(
                &mut output,
                id.checked_sub(previous_node_id)
                    .ok_or_else(|| invalid_data("node identifiers are not sorted"))?,
            );
            put_bytes(&mut output, sequence)?;
            previous_node_id = *id;
        }
        for edge in &self.edges {
            put_u64(&mut output, pack_oriented(edge.from)?);
            put_u64(&mut output, pack_oriented(edge.to)?);
        }
        for &node in &reference.traversal {
            put_u64(&mut output, pack_oriented(node)?);
        }
        for item in &tile.traversals {
            if item.weight == 0 || item.traversal.is_empty() {
                return Err(invalid_data("v4 traversal has zero weight or no visits").into());
            }
            put_u64(&mut output, item.weight);
            put_u64(&mut output, usize_to_u64(item.traversal.len())?);
            for &node in &item.traversal {
                put_u64(&mut output, pack_oriented(node)?);
            }
        }
        Ok(output)
    }

    fn encode_v3(&self) -> ExperimentResult<Vec<u8>> {
        let samples = self
            .paths
            .values()
            .map(|path| path.sample.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let contigs = self
            .paths
            .values()
            .map(|path| path.contig.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let sample_ids = samples
            .iter()
            .enumerate()
            .map(|(id, sample)| (sample.as_str(), id))
            .collect::<HashMap<_, _>>();
        let contig_ids = contigs
            .iter()
            .enumerate()
            .map(|(id, contig)| (contig.as_str(), id))
            .collect::<HashMap<_, _>>();
        let mut output = Vec::new();
        output.extend_from_slice(REGION_MAGIC_NAMED_V2);
        put_u32(&mut output, 2);
        put_u32(&mut output, 1); // packed oriented handles and delta-coded identifiers
        put_u64(&mut output, usize_to_u64(self.nodes.len())?);
        put_u64(&mut output, usize_to_u64(self.edges.len())?);
        put_u64(&mut output, usize_to_u64(self.paths.len())?);
        put_u64(&mut output, usize_to_u64(self.reference_visits.len())?);
        put_u32(&mut output, usize_to_u32(samples.len())?);
        put_u32(&mut output, usize_to_u32(contigs.len())?);
        let mut previous_node_id = 0_u64;
        for (id, sequence) in &self.nodes {
            put_u64(
                &mut output,
                id.checked_sub(previous_node_id)
                    .ok_or_else(|| invalid_data("node identifiers are not sorted"))?,
            );
            put_bytes(&mut output, sequence)?;
            previous_node_id = *id;
        }
        for edge in &self.edges {
            put_u64(&mut output, pack_oriented(edge.from)?);
            put_u64(&mut output, pack_oriented(edge.to)?);
        }
        for sample in &samples {
            put_string(&mut output, sample)?;
        }
        for contig in &contigs {
            put_string(&mut output, contig)?;
        }
        for path in self.paths.values() {
            put_u64(&mut output, path.path_id);
            put_u64(&mut output, path.haplotype);
            put_u64(&mut output, path.fragment);
            put_u32(
                &mut output,
                usize_to_u32(*sample_ids.get(path.sample.as_str()).ok_or_else(|| {
                    invalid_data("path sample is absent from the local dictionary")
                })?)?,
            );
            put_u32(
                &mut output,
                usize_to_u32(*contig_ids.get(path.contig.as_str()).ok_or_else(|| {
                    invalid_data("path contig is absent from the local dictionary")
                })?)?,
            );
            output.push(u8::from(path.is_reference));
            output.extend_from_slice(&[0_u8; 7]);
            put_u64(&mut output, usize_to_u64(path.visits.len())?);
            let mut previous_visit = 0_u64;
            for (&index, &node) in &path.visits {
                put_u64(
                    &mut output,
                    index
                        .checked_sub(previous_visit)
                        .ok_or_else(|| invalid_data("path visits are not sorted"))?,
                );
                put_u64(&mut output, pack_oriented(node)?);
                previous_visit = index;
            }
        }
        for visit in &self.reference_visits {
            put_u64(&mut output, visit.path_id);
            put_u64(&mut output, visit.visit_index);
            put_u64(&mut output, visit.start);
            put_u64(&mut output, visit.end);
            put_u64(&mut output, pack_oriented(visit.node)?);
        }
        Ok(output)
    }

    fn decode(bytes: &[u8]) -> ExperimentResult<Self> {
        if bytes.get(..8) == Some(REGION_MAGIC.as_slice()) {
            return RecordRegionalPayload::decode(bytes)?.into_regional_graph();
        }
        if bytes.get(..8) == Some(REGION_MAGIC_WEIGHTED_V3.as_slice()) {
            return Self::decode_v4(bytes);
        }
        Self::decode_v3(bytes)
    }

    #[allow(clippy::too_many_lines)]
    fn decode_v4(bytes: &[u8]) -> ExperimentResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        if reader.take(8)? != REGION_MAGIC_WEIGHTED_V3 {
            return Err(invalid_data("invalid v4 regional chunk magic").into());
        }
        let version = reader.u32()?;
        if version != 3 {
            return Err(invalid_data(format!("unsupported regional version {version}")).into());
        }
        if reader.u32()? != 1 {
            return Err(invalid_data("unsupported v4 regional flags").into());
        }
        let semantics = semantics_from_code(reader.u8()?)?;
        let _reserved = reader.take(7)?;
        let node_count = reader.u64()?;
        let edge_count = reader.u64()?;
        let core_start = reader.u64()?;
        let core_end = reader.u64()?;
        let context = reader.u64()?;
        let reference_start = reader.u64()?;
        let reference_end = reader.u64()?;
        let haplotype = reader.u64()?;
        let reference_count = reader.u64()?;
        let traversal_count = reader.u64()?;
        if core_start >= core_end || reference_start >= reference_end {
            return Err(invalid_data("v4 payload has an invalid interval").into());
        }
        if context != CONSTRUCTION_CONTEXT {
            return Err(
                invalid_data(format!("unsupported v4 construction context {context}")).into(),
            );
        }
        let sample = reader.string()?;
        let contig = reader.string()?;
        let mut result = Self {
            semantics,
            ..Self::default()
        };
        let mut previous_node_id = 0_u64;
        for _ in 0..node_count {
            let node_id = previous_node_id
                .checked_add(reader.u64()?)
                .ok_or_else(|| invalid_data("node identifier delta overflow"))?;
            if result.nodes.insert(node_id, reader.bytes()?).is_some() {
                return Err(invalid_data("duplicate node identifier in v4 payload").into());
            }
            previous_node_id = node_id;
        }
        for _ in 0..edge_count {
            result.edges.insert(Edge {
                from: unpack_oriented(reader.u64()?),
                to: unpack_oriented(reader.u64()?),
            });
        }
        let mut reference_traversal = Vec::with_capacity(count_bounded_by_bytes(
            reference_count,
            reader.remaining(),
            8,
            "reference traversal",
        )?);
        for _ in 0..reference_count {
            reference_traversal.push(unpack_oriented(reader.u64()?));
        }
        if traversal_end(&result.nodes, reference_start, &reference_traversal)? != reference_end {
            return Err(
                invalid_data("v4 reference traversal length does not match its interval").into(),
            );
        }
        let mut traversals = Vec::with_capacity(count_bounded_by_bytes(
            traversal_count,
            reader.remaining(),
            16,
            "anonymous traversal",
        )?);
        for _ in 0..traversal_count {
            let weight = reader.u64()?;
            let visit_count = reader.u64()?;
            if weight == 0 || visit_count == 0 {
                return Err(invalid_data("v4 traversal has zero weight or no visits").into());
            }
            let mut traversal = Vec::with_capacity(count_bounded_by_bytes(
                visit_count,
                reader.remaining(),
                8,
                "anonymous traversal visits",
            )?);
            for _ in 0..visit_count {
                traversal.push(unpack_oriented(reader.u64()?));
            }
            if traversal
                .iter()
                .any(|node| !result.nodes.contains_key(&node.id))
            {
                return Err(invalid_data("v4 anonymous traversal refers to an absent node").into());
            }
            traversals.push(WeightedTraversal { weight, traversal });
        }
        reader.finish()?;
        result.reference_paths.push(RegionalReferencePath {
            sample: sample.clone(),
            contig: contig.clone(),
            haplotype,
            start: reference_start,
            end: reference_end,
            traversal: reference_traversal,
        });
        result.haplotype_tiles.push(CanonicalHaplotypeTile {
            reference_sample: sample,
            reference_contig: contig,
            core_start,
            core_end,
            traversals,
        });
        Ok(result)
    }

    fn decode_v3(bytes: &[u8]) -> ExperimentResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        if reader.take(8)? != REGION_MAGIC_NAMED_V2 {
            return Err(invalid_data("invalid regional chunk magic").into());
        }
        let version = reader.u32()?;
        if version != 2 {
            return Err(invalid_data(format!("unsupported regional version {version}")).into());
        }
        let flags = reader.u32()?;
        if flags != 1 {
            return Err(invalid_data(format!("unsupported regional flags {flags}")).into());
        }
        let node_count = reader.u64()?;
        let edge_count = reader.u64()?;
        let path_count = reader.u64()?;
        let reference_count = reader.u64()?;
        let sample_count = reader.u32()?;
        let contig_count = reader.u32()?;
        let mut result = Self {
            semantics: HaplotypeSemantics::NamedPathsV3,
            ..Self::default()
        };
        let mut previous_node_id = 0_u64;
        for _ in 0..node_count {
            let node_id = previous_node_id
                .checked_add(reader.u64()?)
                .ok_or_else(|| invalid_data("node identifier delta overflow"))?;
            result.nodes.insert(node_id, reader.bytes()?);
            previous_node_id = node_id;
        }
        for _ in 0..edge_count {
            result.edges.insert(Edge {
                from: unpack_oriented(reader.u64()?),
                to: unpack_oriented(reader.u64()?),
            });
        }
        let samples = (0..sample_count)
            .map(|_| reader.string())
            .collect::<Result<Vec<_>, _>>()?;
        let contigs = (0..contig_count)
            .map(|_| reader.string())
            .collect::<Result<Vec<_>, _>>()?;
        for _ in 0..path_count {
            let path_id = reader.u64()?;
            let haplotype = reader.u64()?;
            let fragment = reader.u64()?;
            let sample_id = u32_to_usize(reader.u32()?)?;
            let contig_id = u32_to_usize(reader.u32()?)?;
            let is_reference = reader.u8()? != 0;
            let _reserved = reader.take(7)?;
            let sample = samples
                .get(sample_id)
                .ok_or_else(|| invalid_data("path sample dictionary index is out of range"))?
                .clone();
            let contig = contigs
                .get(contig_id)
                .ok_or_else(|| invalid_data("path contig dictionary index is out of range"))?
                .clone();
            let visit_count = reader.u64()?;
            let mut visits = BTreeMap::new();
            let mut previous_visit = 0_u64;
            for _ in 0..visit_count {
                let index = previous_visit
                    .checked_add(reader.u64()?)
                    .ok_or_else(|| invalid_data("path visit delta overflow"))?;
                visits.insert(index, unpack_oriented(reader.u64()?));
                previous_visit = index;
            }
            result.paths.insert(
                path_id,
                RegionalPath {
                    path_id,
                    sample,
                    contig,
                    haplotype,
                    fragment,
                    is_reference,
                    visits,
                },
            );
        }
        for _ in 0..reference_count {
            result.reference_visits.insert(ReferenceVisit {
                path_id: reader.u64()?,
                visit_index: reader.u64()?,
                start: reader.u64()?,
                end: reader.u64()?,
                node: unpack_oriented(reader.u64()?),
            });
        }
        reader.finish()?;
        Ok(result)
    }
}

fn push_path_segment(
    paths: &mut Vec<CanonicalPath>,
    path: &RegionalPath,
    traversal: Vec<OrientedNode>,
) {
    paths.push(CanonicalPath {
        sample: path.sample.clone(),
        contig: path.contig.clone(),
        haplotype: path.haplotype,
        fragment: path.fragment,
        is_reference: path.is_reference,
        traversal,
    });
}

fn canonical_mismatch_summary(candidate: &CanonicalSubgraph, oracle: &CanonicalSubgraph) -> String {
    let missing_nodes = oracle
        .nodes
        .keys()
        .filter(|id| !candidate.nodes.contains_key(id))
        .copied()
        .collect::<Vec<_>>();
    let extra_nodes = candidate
        .nodes
        .keys()
        .filter(|id| !oracle.nodes.contains_key(id))
        .copied()
        .collect::<Vec<_>>();
    let conflicting_sequences = candidate
        .nodes
        .iter()
        .filter_map(|(id, sequence)| {
            oracle
                .nodes
                .get(id)
                .is_some_and(|expected| expected != sequence)
                .then_some(*id)
        })
        .collect::<Vec<_>>();
    let missing_edges = oracle
        .edges
        .difference(&candidate.edges)
        .copied()
        .collect::<Vec<_>>();
    let extra_edges = candidate
        .edges
        .difference(&oracle.edges)
        .copied()
        .collect::<Vec<_>>();
    let candidate = candidate.normalized();
    let oracle = oracle.normalized();
    let first_path_mismatch = candidate
        .paths
        .iter()
        .zip(&oracle.paths)
        .position(|(left, right)| left != right)
        .or_else(|| {
            (candidate.paths.len() != oracle.paths.len())
                .then_some(candidate.paths.len().min(oracle.paths.len()))
        });
    let path_detail = first_path_mismatch.map_or_else(
        || "none".into(),
        |index| {
            format!(
                "index {index}: candidate {}, oracle {}",
                path_summary(candidate.paths.get(index)),
                path_summary(oracle.paths.get(index))
            )
        },
    );
    format!(
        "nodes {}/{} (missing {} {:?}, extra {} {:?}, conflicting sequences {} {:?}); edges {}/{} (missing {} {:?}, extra {} {:?}); paths {}/{} (first mismatch {}); reference intervals match={}",
        candidate.nodes.len(),
        oracle.nodes.len(),
        missing_nodes.len(),
        missing_nodes.iter().take(8).collect::<Vec<_>>(),
        extra_nodes.len(),
        extra_nodes.iter().take(8).collect::<Vec<_>>(),
        conflicting_sequences.len(),
        conflicting_sequences.iter().take(8).collect::<Vec<_>>(),
        candidate.edges.len(),
        oracle.edges.len(),
        missing_edges.len(),
        missing_edges.iter().take(4).collect::<Vec<_>>(),
        extra_edges.len(),
        extra_edges.iter().take(4).collect::<Vec<_>>(),
        candidate.paths.len(),
        oracle.paths.len(),
        path_detail,
        candidate.reference_intervals == oracle.reference_intervals,
    )
}

fn path_summary(path: Option<&CanonicalPath>) -> String {
    path.map_or_else(
        || "<absent>".into(),
        |path| {
            format!(
                "{}#{} haplotype={} fragment={} reference={} visits={} first={:?} last={:?}",
                path.sample,
                path.contig,
                path.haplotype,
                path.fragment,
                path.is_reference,
                path.traversal.len(),
                path.traversal.first(),
                path.traversal.last(),
            )
        },
    )
}

fn load_bootstrap(source: &impl RangeSource) -> ExperimentResult<Bootstrap> {
    let source_len = source.len()?;
    let first_len = source_len.min(usize_to_u64(BOOTSTRAP_LEN)?);
    let mut bytes = source.read_range(0, u64_to_usize(first_len)?)?;
    if bytes.len() < HEADER_LEN {
        return Err(invalid_data("archive is shorter than its header").into());
    }
    let header = decode_header(&bytes[..HEADER_LEN])?;
    let root_end = usize_to_u64(HEADER_LEN)?
        .checked_add(header.root_len)
        .ok_or_else(|| invalid_data("root index end overflow"))?;
    if root_end > header.data_offset || header.data_offset > source_len {
        return Err(invalid_data("archive directory offsets are inconsistent").into());
    }
    let mut dependency_rounds = 1;
    if root_end > first_len {
        let remainder = source.read_range(first_len, u64_to_usize(root_end - first_len)?)?;
        bytes.extend_from_slice(&remainder);
        dependency_rounds += 1;
    }
    let root = decode_root_index(&bytes[HEADER_LEN..u64_to_usize(root_end)?], header)?;
    Ok(Bootstrap {
        bytes,
        root,
        dependency_rounds,
    })
}

fn directory_page_offset(manifest: &ReferenceManifest, bucket_index: u64) -> io::Result<u64> {
    if bucket_index >= manifest.page_count {
        return Err(invalid_data("directory bucket index is out of range"));
    }
    manifest
        .first_page_offset
        .checked_add(
            bucket_index
                .checked_mul(usize_to_u64(DIRECTORY_PAGE_BYTES)?)
                .ok_or_else(|| invalid_data("directory page offset overflow"))?,
        )
        .ok_or_else(|| invalid_data("directory page offset overflow"))
}

#[derive(Clone, Copy, Debug)]
struct Header {
    version: u32,
    root_len: u64,
    entry_count: u64,
    data_offset: u64,
}

fn encode_header(root_len: u64, entry_count: u64, data_offset: u64) -> [u8; HEADER_LEN] {
    let mut output = [0_u8; HEADER_LEN];
    output[..8].copy_from_slice(ARCHIVE_MAGIC);
    output[8..12].copy_from_slice(&ARCHIVE_VERSION.to_le_bytes());
    output[12..16].copy_from_slice(&64_u32.to_le_bytes());
    output[16..24].copy_from_slice(&64_u64.to_le_bytes());
    output[24..32].copy_from_slice(&root_len.to_le_bytes());
    output[32..40].copy_from_slice(&entry_count.to_le_bytes());
    output[40..48].copy_from_slice(&data_offset.to_le_bytes());
    output
}

fn decode_header(bytes: &[u8]) -> io::Result<Header> {
    if bytes.len() != HEADER_LEN {
        return Err(invalid_data("invalid archive header"));
    }
    let magic = &bytes[..8];
    let version = u32::from_le_bytes(bytes[8..12].try_into().expect("fixed header slice"));
    let header_len = u32::from_le_bytes(bytes[12..16].try_into().expect("fixed header slice"));
    let root_offset = u64::from_le_bytes(bytes[16..24].try_into().expect("fixed header slice"));
    let supported_version = (magic == ARCHIVE_MAGIC && version == ARCHIVE_VERSION)
        || (magic == ARCHIVE_MAGIC_V3 && version == 3);
    if !supported_version
        || usize::try_from(header_len).ok() != Some(HEADER_LEN)
        || root_offset != 64
        || bytes[48..].iter().any(|&value| value != 0)
    {
        return Err(invalid_data(format!(
            "unsupported archive version {version}, header length {header_len}, or root offset {root_offset}"
        )));
    }
    Ok(Header {
        version,
        root_len: u64::from_le_bytes(bytes[24..32].try_into().expect("fixed header slice")),
        entry_count: u64::from_le_bytes(bytes[32..40].try_into().expect("fixed header slice")),
        data_offset: u64::from_le_bytes(bytes[40..48].try_into().expect("fixed header slice")),
    })
}

fn encode_root_index(manifests: &[ReferenceManifest]) -> ExperimentResult<Vec<u8>> {
    let mut output = Vec::new();
    put_u64(&mut output, usize_to_u64(manifests.len())?);
    for manifest in manifests {
        put_string(&mut output, &manifest.sample)?;
        put_string(&mut output, &manifest.contig)?;
        put_u64(&mut output, manifest.start);
        put_u64(&mut output, manifest.end);
        put_u64(&mut output, manifest.grid_start);
        put_u64(&mut output, manifest.window_size);
        put_u64(&mut output, manifest.bucket_span);
        put_u64(&mut output, manifest.first_page_offset);
        put_u64(&mut output, manifest.page_count);
        put_u64(&mut output, manifest.entry_count);
        output.push(manifest.codec.code());
        output.extend_from_slice(&[0_u8; 7]);
    }
    Ok(output)
}

fn decode_root_index(bytes: &[u8], header: Header) -> ExperimentResult<RootIndex> {
    let mut reader = BinaryReader::new(bytes);
    let count = reader.u64()?;
    let mut manifests = Vec::with_capacity(u64_to_usize(count)?);
    let root_end = usize_to_u64(HEADER_LEN)?
        .checked_add(header.root_len)
        .ok_or_else(|| invalid_data("root index end overflow"))?;
    let mut previous_page_end = root_end;
    let mut total_entries = 0_u64;
    for _ in 0..count {
        let manifest = ReferenceManifest {
            sample: reader.string()?,
            contig: reader.string()?,
            start: reader.u64()?,
            end: reader.u64()?,
            grid_start: reader.u64()?,
            window_size: reader.u64()?,
            bucket_span: reader.u64()?,
            first_page_offset: reader.u64()?,
            page_count: reader.u64()?,
            entry_count: reader.u64()?,
            codec: ChunkCodec::from_code(reader.u8()?)?,
        };
        if reader.take(7)? != [0_u8; 7] {
            return Err(invalid_data("reference manifest reserved bytes are nonzero").into());
        }
        let page_end = manifest
            .first_page_offset
            .checked_add(
                manifest
                    .page_count
                    .checked_mul(usize_to_u64(DIRECTORY_PAGE_BYTES)?)
                    .ok_or_else(|| invalid_data("manifest page range overflow"))?,
            )
            .ok_or_else(|| invalid_data("manifest page range overflow"))?;
        let expected_pages = manifest
            .end
            .checked_sub(manifest.grid_start)
            .and_then(|span| span.checked_add(manifest.bucket_span - 1))
            .map(|span| span / manifest.bucket_span);
        if manifest.start >= manifest.end
            || manifest.grid_start > manifest.start
            || manifest.window_size == 0
            || manifest.bucket_span == 0
            || manifest.page_count == 0
            || Some(manifest.page_count) != expected_pages
            || manifest.first_page_offset != previous_page_end
            || page_end > header.data_offset
        {
            return Err(invalid_data("invalid arithmetic reference manifest").into());
        }
        previous_page_end = page_end;
        total_entries = total_entries
            .checked_add(manifest.entry_count)
            .ok_or_else(|| invalid_data("directory entry count overflow"))?;
        manifests.push(manifest);
    }
    reader.finish()?;
    if total_entries != header.entry_count || previous_page_end != header.data_offset {
        return Err(invalid_data(format!(
            "header entry count {} or data offset does not match root manifest",
            header.entry_count
        ))
        .into());
    }
    Ok(RootIndex {
        logical_bytes: root_end,
        manifests,
    })
}

fn encode_directory_page(
    entries: &[ArchiveEntry],
    bucket_start: u64,
) -> ExperimentResult<[u8; DIRECTORY_PAGE_BYTES]> {
    if entries.len() > DIRECTORY_ENTRIES_PER_PAGE {
        return Err(invalid_data(format!(
            "directory bucket contains {} adaptive chunks; fixed page capacity is {DIRECTORY_ENTRIES_PER_PAGE}",
            entries.len()
        ))
        .into());
    }
    let mut encoded = Vec::with_capacity(DIRECTORY_PAGE_BYTES);
    put_u32(&mut encoded, usize_to_u32(entries.len())?);
    put_u32(&mut encoded, usize_to_u32(DIRECTORY_ENTRY_BYTES)?);
    put_u64(&mut encoded, bucket_start);
    for entry in entries {
        if entry.start >= entry.end || entry.start < bucket_start {
            return Err(invalid_data("directory entry is outside its bucket").into());
        }
        put_u64(&mut encoded, entry.start);
        put_u64(&mut encoded, entry.end);
        put_u64(&mut encoded, entry.offset);
        put_u64(&mut encoded, entry.compressed_len);
        put_u64(&mut encoded, entry.uncompressed_len);
    }
    let mut output = [0_u8; DIRECTORY_PAGE_BYTES];
    output[..encoded.len()].copy_from_slice(&encoded);
    Ok(output)
}

fn decode_directory_page(
    bytes: &[u8],
    manifest: &ReferenceManifest,
    bucket_index: u64,
) -> ExperimentResult<ArchiveIndex> {
    if bytes.len() != DIRECTORY_PAGE_BYTES {
        return Err(invalid_data("directory page has the wrong fixed size").into());
    }
    let mut reader = BinaryReader::new(bytes);
    let count = u32_to_usize(reader.u32()?)?;
    let entry_bytes = u32_to_usize(reader.u32()?)?;
    let expected_bucket_start = manifest
        .grid_start
        .checked_add(
            bucket_index
                .checked_mul(manifest.bucket_span)
                .ok_or_else(|| invalid_data("directory bucket coordinate overflow"))?,
        )
        .ok_or_else(|| invalid_data("directory bucket coordinate overflow"))?;
    let bucket_start = reader.u64()?;
    if count > DIRECTORY_ENTRIES_PER_PAGE
        || entry_bytes != DIRECTORY_ENTRY_BYTES
        || bucket_start != expected_bucket_start
    {
        return Err(invalid_data("invalid fixed directory page header").into());
    }
    let bucket_end = bucket_start
        .checked_add(manifest.bucket_span)
        .ok_or_else(|| invalid_data("directory bucket end overflow"))?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let start = reader.u64()?;
        let end = reader.u64()?;
        let offset = reader.u64()?;
        let compressed_len = reader.u64()?;
        let uncompressed_len = reader.u64()?;
        if start >= end
            || start < bucket_start
            || end > bucket_end.min(manifest.end)
            || compressed_len == 0
            || uncompressed_len == 0
        {
            return Err(invalid_data("invalid fixed directory entry").into());
        }
        entries.push(ArchiveEntry {
            start,
            end,
            offset,
            compressed_len,
            uncompressed_len,
            codec: manifest.codec,
        });
    }
    Ok(ArchiveIndex { entries })
}

fn collect_chunk(
    entry: &ArchiveEntry,
    bootstrap: &[u8],
    fetched: &[(ByteRange, Vec<u8>)],
) -> ExperimentResult<Vec<u8>> {
    let chunk = ByteRange {
        start: entry.offset,
        end: entry
            .offset
            .checked_add(entry.compressed_len)
            .ok_or_else(|| invalid_data("chunk end overflow"))?,
    };
    collect_stored_range(chunk, bootstrap, fetched)
}

fn collect_stored_range(
    stored: ByteRange,
    bootstrap: &[u8],
    fetched: &[(ByteRange, Vec<u8>)],
) -> ExperimentResult<Vec<u8>> {
    let mut output = vec![0_u8; u64_to_usize(stored.len())?];
    let bootstrap_end = usize_to_u64(bootstrap.len())?;
    copy_intersection(
        stored,
        ByteRange {
            start: 0,
            end: bootstrap_end,
        },
        bootstrap,
        &mut output,
    )?;
    for (range, bytes) in fetched {
        copy_intersection(stored, *range, bytes, &mut output)?;
    }
    Ok(output)
}

fn copy_intersection(
    destination_range: ByteRange,
    source_range: ByteRange,
    source: &[u8],
    destination: &mut [u8],
) -> ExperimentResult<()> {
    let start = destination_range.start.max(source_range.start);
    let end = destination_range.end.min(source_range.end);
    if start >= end {
        return Ok(());
    }
    let source_start = u64_to_usize(start - source_range.start)?;
    let source_end = u64_to_usize(end - source_range.start)?;
    let destination_start = u64_to_usize(start - destination_range.start)?;
    let destination_end = u64_to_usize(end - destination_range.start)?;
    destination[destination_start..destination_end]
        .copy_from_slice(&source[source_start..source_end]);
    Ok(())
}

fn coalesce_ranges(mut ranges: Vec<ByteRange>, gap: u64) -> Vec<ByteRange> {
    ranges.sort_unstable();
    let mut result: Vec<ByteRange> = Vec::new();
    for range in ranges {
        if let Some(previous) = result.last_mut()
            && range.start <= previous.end.saturating_add(gap)
        {
            previous.end = previous.end.max(range.end);
            continue;
        }
        result.push(range);
    }
    result
}

fn compress(codec: ChunkCodec, bytes: &[u8]) -> io::Result<Vec<u8>> {
    if let Some(level) = codec.level() {
        zstd::bulk::compress(bytes, level)
    } else {
        Ok(bytes.to_vec())
    }
}

fn find_identical_archive_chunk(
    archive: &mut File,
    chunks: &[StoredChunk],
    candidates: Option<&Vec<usize>>,
    codec: ChunkCodec,
    raw: &[u8],
) -> ExperimentResult<Option<usize>> {
    let Some(candidates) = candidates else {
        return Ok(None);
    };
    for &candidate_id in candidates {
        let candidate = chunks
            .get(candidate_id)
            .ok_or_else(|| invalid_data("deduplication hash refers to an absent chunk"))?;
        archive.seek(SeekFrom::Start(candidate.archive_offset))?;
        let mut compressed = vec![0_u8; u64_to_usize(candidate.compressed_len)?];
        archive.read_exact(&mut compressed)?;
        let candidate_raw = decompress(codec, &compressed, candidate.uncompressed_len)?;
        if candidate_raw == raw {
            archive.seek(SeekFrom::End(0))?;
            return Ok(Some(candidate_id));
        }
    }
    archive.seek(SeekFrom::End(0))?;
    Ok(None)
}

fn decompress(codec: ChunkCodec, bytes: &[u8], expected_len: u64) -> io::Result<Vec<u8>> {
    let result = if codec.level().is_some() {
        zstd::bulk::decompress(bytes, u64_to_usize(expected_len)?)?
    } else {
        bytes.to_vec()
    };
    if usize_to_u64(result.len())? != expected_len {
        return Err(invalid_data(format!(
            "decoded chunk length {} does not match {expected_len}",
            result.len()
        )));
    }
    Ok(result)
}

/// Decompresses and structurally validates every physical payload in an archive.
///
/// # Errors
///
/// Returns an error for malformed metadata, invalid ranges, decompression
/// failure, corrupt payload bytes, or file I/O failure.
pub fn validate_fixed_archive(path: &Path) -> ExperimentResult<ArchiveValidationSummary> {
    let started = Instant::now();
    let source = FileRangeSource::open(path)?;
    let bootstrap = load_bootstrap(&source)?;
    let header = decode_header(&bootstrap.bytes[..HEADER_LEN])?;
    let source_len = source.len()?;
    let mut entry_count = 0_u64;
    let mut decoded_payloads = BTreeSet::new();
    let mut compressed_payload_bytes = 0_u64;
    let mut uncompressed_payload_bytes = 0_u64;
    let mut directory_pages = 0_u64;
    for manifest in &bootstrap.root.manifests {
        directory_pages = directory_pages
            .checked_add(manifest.page_count)
            .ok_or_else(|| invalid_data("validated directory page count overflow"))?;
        for bucket_index in 0..manifest.page_count {
            let page_offset = directory_page_offset(manifest, bucket_index)?;
            let page = source.read_range(page_offset, DIRECTORY_PAGE_BYTES)?;
            let entries = decode_directory_page(&page, manifest, bucket_index)?.entries;
            entry_count = entry_count
                .checked_add(usize_to_u64(entries.len())?)
                .ok_or_else(|| invalid_data("validated directory entry count overflow"))?;
            for entry in entries {
                let payload_end = entry
                    .offset
                    .checked_add(entry.compressed_len)
                    .ok_or_else(|| invalid_data("payload range overflow during validation"))?;
                if entry.offset < header.data_offset || payload_end > source_len {
                    return Err(invalid_data("payload range is outside the archive").into());
                }
                if !decoded_payloads.insert((
                    entry.offset,
                    entry.compressed_len,
                    entry.uncompressed_len,
                    entry.codec.code(),
                )) {
                    continue;
                }
                compressed_payload_bytes = compressed_payload_bytes
                    .checked_add(entry.compressed_len)
                    .ok_or_else(|| invalid_data("validated compressed byte count overflow"))?;
                uncompressed_payload_bytes = uncompressed_payload_bytes
                    .checked_add(entry.uncompressed_len)
                    .ok_or_else(|| invalid_data("validated uncompressed byte count overflow"))?;
                let compressed =
                    source.read_range(entry.offset, u64_to_usize(entry.compressed_len)?)?;
                let raw = decompress(entry.codec, &compressed, entry.uncompressed_len)?;
                let (core_start, core_end) = if raw.get(..8) == Some(REGION_MAGIC.as_slice()) {
                    let payload = RecordRegionalPayload::decode(&raw)?;
                    (payload.core_start, payload.core_end)
                } else {
                    let regional = RegionalGraph::decode(&raw)?;
                    regional
                        .haplotype_tiles
                        .first()
                        .map_or((entry.start, entry.end), |tile| {
                            (tile.core_start, tile.core_end)
                        })
                };
                if core_start != entry.start || core_end != entry.end {
                    return Err(invalid_data(
                        "validated payload provenance differs from its directory entry",
                    )
                    .into());
                }
            }
        }
    }
    if entry_count != header.entry_count {
        return Err(invalid_data("validated directory count differs from archive header").into());
    }
    Ok(ArchiveValidationSummary {
        schema_version: 1,
        archive_version: header.version,
        archive_path: path.to_path_buf(),
        archive_bytes: source_len,
        reference_manifests: usize_to_u64(bootstrap.root.manifests.len())?,
        directory_pages,
        directory_entries: entry_count,
        physical_payloads: usize_to_u64(decoded_payloads.len())?,
        compressed_payload_bytes,
        uncompressed_payload_bytes,
        validation_wall_ms: started.elapsed().as_secs_f64() * 1_000.0,
    })
}

fn encode_canonical(graph: &CanonicalSubgraph) -> ExperimentResult<Vec<u8>> {
    let normalized = graph.normalized();
    let mut output = Vec::new();
    output.extend_from_slice(b"PNGCAN04");
    put_u64(&mut output, usize_to_u64(normalized.nodes.len())?);
    for (id, sequence) in &normalized.nodes {
        put_u64(&mut output, *id);
        put_bytes(&mut output, sequence)?;
    }
    put_u64(&mut output, usize_to_u64(normalized.edges.len())?);
    for edge in &normalized.edges {
        put_oriented(&mut output, edge.from);
        put_oriented(&mut output, edge.to);
    }
    put_u64(&mut output, usize_to_u64(normalized.paths.len())?);
    for path in &normalized.paths {
        put_string(&mut output, &path.sample)?;
        put_string(&mut output, &path.contig)?;
        put_u64(&mut output, path.haplotype);
        put_u64(&mut output, path.fragment);
        output.push(u8::from(path.is_reference));
        put_u64(&mut output, usize_to_u64(path.traversal.len())?);
        for node in &path.traversal {
            put_oriented(&mut output, *node);
        }
    }
    put_u64(
        &mut output,
        usize_to_u64(normalized.reference_intervals.len())?,
    );
    for interval in &normalized.reference_intervals {
        put_string(&mut output, &interval.sample)?;
        put_string(&mut output, &interval.contig)?;
        put_u64(&mut output, interval.start);
        put_u64(&mut output, interval.end);
    }
    Ok(output)
}

fn flip(node: OrientedNode) -> OrientedNode {
    OrientedNode {
        id: node.id,
        reverse: !node.reverse,
    }
}

const fn semantics_code(semantics: HaplotypeSemantics) -> u8 {
    match semantics {
        HaplotypeSemantics::NamedPathsV3 => 0,
        HaplotypeSemantics::AnonymousAllTilePaths => 1,
        HaplotypeSemantics::AnonymousDistinctWeightedTilePaths => 2,
    }
}

fn semantics_from_code(code: u8) -> io::Result<HaplotypeSemantics> {
    match code {
        1 => Ok(HaplotypeSemantics::AnonymousAllTilePaths),
        2 => Ok(HaplotypeSemantics::AnonymousDistinctWeightedTilePaths),
        _ => Err(invalid_data(format!(
            "unsupported v4 haplotype semantics code {code}"
        ))),
    }
}

fn traversal_end(
    nodes: &BTreeMap<u64, Vec<u8>>,
    start: u64,
    traversal: &[OrientedNode],
) -> io::Result<u64> {
    traversal.iter().try_fold(start, |coordinate, node| {
        let length = nodes
            .get(&node.id)
            .ok_or_else(|| invalid_data("v4 traversal refers to an absent node"))?
            .len();
        coordinate
            .checked_add(usize_to_u64(length)?)
            .ok_or_else(|| invalid_data("v4 traversal coordinate overflow"))
    })
}

fn count_bounded_by_bytes(
    count: u64,
    remaining: usize,
    minimum_bytes: usize,
    section: &str,
) -> io::Result<usize> {
    let count = u64_to_usize(count)?;
    if count > remaining / minimum_bytes {
        return Err(invalid_data(format!(
            "{section} count exceeds the remaining payload"
        )));
    }
    Ok(count)
}

fn pack_oriented(node: OrientedNode) -> io::Result<u64> {
    node.id
        .checked_mul(2)
        .and_then(|value| value.checked_add(u64::from(node.reverse)))
        .ok_or_else(|| invalid_data("oriented node identifier overflow"))
}

fn unpack_oriented(value: u64) -> OrientedNode {
    OrientedNode {
        id: value / 2,
        reverse: value % 2 != 0,
    }
}

fn oriented(node_id: usize, orientation: Orientation) -> io::Result<OrientedNode> {
    Ok(OrientedNode {
        id: usize_to_u64(node_id)?,
        reverse: orientation == Orientation::Reverse,
    })
}

struct BinaryReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> BinaryReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> io::Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid_data("unexpected end of binary data"))?;
        let result = &self.bytes[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("fixed integer slice"),
        ))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("fixed integer slice"),
        ))
    }

    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let length = u64_to_usize(self.u64()?)?;
        Ok(self.take(length)?.to_vec())
    }

    fn string(&mut self) -> io::Result<String> {
        String::from_utf8(self.bytes()?).map_err(|error| invalid_data(error.to_string()))
    }

    fn finish(self) -> io::Result<()> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(invalid_data(format!(
                "{} trailing bytes in binary data",
                self.bytes.len() - self.position
            )))
        }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    put_u64(output, usize_to_u64(bytes.len())?);
    output.extend_from_slice(bytes);
    Ok(())
}

fn put_string(output: &mut Vec<u8>, value: &str) -> io::Result<()> {
    put_bytes(output, value.as_bytes())
}

fn put_oriented(output: &mut Vec<u8>, node: OrientedNode) {
    put_u64(output, node.id);
    output.push(u8::from(node.reverse));
}

fn usize_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_data("usize does not fit in u64"))
}

fn usize_to_u32(value: usize) -> io::Result<u32> {
    u32::try_from(value).map_err(|_| invalid_data("usize does not fit in u32"))
}

fn u32_to_usize(value: u32) -> io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid_data("u32 does not fit in usize"))
}

fn u64_to_usize(value: u64) -> io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid_data("u64 does not fit in usize"))
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[allow(clippy::cast_precision_loss)]
fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        f64::INFINITY
    } else {
        numerator as f64 / denominator as f64
    }
}

#[allow(clippy::cast_precision_loss)]
fn mean(values: &[u64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<u64>() as f64 / values.len() as f64
    }
}

fn percentile_u64(values: &[u64], percentile: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    #[allow(clippy::cast_precision_loss)]
    let scaled = (values.len().saturating_sub(1)) as f64 * percentile;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = scaled.ceil() as usize;
    values[index.min(values.len() - 1)]
}

/// Executes one GBZ-base query for process-level I/O instrumentation.
///
/// # Errors
///
/// Returns an error if the database cannot be opened, coordinates cannot be
/// represented by the upstream API, or subgraph extraction fails.
pub fn internal_gbz_base_query(
    database: &Path,
    sample: &str,
    contig: &str,
    start: u64,
    end: u64,
    context: u64,
) -> ExperimentResult<()> {
    let database = gbz_base::GBZBase::open(database)?;
    let mut graph = gbz_base::GraphInterface::new(&database)?;
    let path = FullPathName::reference(sample, contig);
    let query = SubgraphQuery::path_interval(&path, u64_to_usize(start)?..u64_to_usize(end)?)
        .with_context(u64_to_usize(context)?)
        .with_haplotypes(HaplotypeOutput::All);
    let mut subgraph = Subgraph::new();
    subgraph.from_db(&mut graph, &query)?;
    std::hint::black_box(subgraph.nodes());
    Ok(())
}

#[derive(Debug)]
struct ConformanceArchiveParts {
    archive: Vec<u8>,
    header: Vec<u8>,
    root: Vec<u8>,
    directory: Vec<u8>,
    compressed: Vec<u8>,
}

fn conformance_record_payload() -> RecordRegionalPayload {
    let forward_1 = OrientedNode {
        id: 1,
        reverse: false,
    };
    let forward_2 = OrientedNode {
        id: 2,
        reverse: false,
    };
    RecordRegionalPayload {
        semantics: HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
        core_start: 100,
        core_end: 102,
        context: CONSTRUCTION_CONTEXT,
        reference_sample: "GRCh38".into(),
        reference_contig: "chr1".into(),
        reference_haplotype: 0,
        reference_fragment_start: 100,
        reference_query_offset: 0,
        reference_node_offset: 0,
        reference_position: (2, 0),
        nodes: BTreeMap::from([(1, b"A".to_vec()), (2, b"C".to_vec())]),
        edges: BTreeSet::from([Edge {
            from: forward_1,
            to: forward_2,
        }]),
        records: vec![
            PackedGbwtRecord {
                handle: 2,
                occurrence_count: 2,
                bytes: vec![1, 4, 0, 1],
            },
            PackedGbwtRecord {
                handle: 3,
                occurrence_count: 2,
                bytes: vec![1, 0, 0, 1],
            },
            PackedGbwtRecord {
                handle: 4,
                occurrence_count: 2,
                bytes: vec![1, 0, 0, 1],
            },
            PackedGbwtRecord {
                handle: 5,
                occurrence_count: 2,
                bytes: vec![1, 3, 0, 1],
            },
        ],
        total_occurrences: 8,
    }
}

fn conformance_weighted_graph() -> RegionalGraph {
    let forward_1 = OrientedNode {
        id: 1,
        reverse: false,
    };
    let forward_2 = OrientedNode {
        id: 2,
        reverse: false,
    };
    RegionalGraph {
        nodes: BTreeMap::from([(1, b"A".to_vec()), (2, b"C".to_vec())]),
        edges: BTreeSet::from([Edge {
            from: forward_1,
            to: forward_2,
        }]),
        semantics: HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
        reference_paths: vec![RegionalReferencePath {
            sample: "GRCh38".into(),
            contig: "chr1".into(),
            haplotype: 0,
            start: 100,
            end: 102,
            traversal: vec![forward_1, forward_2],
        }],
        haplotype_tiles: vec![CanonicalHaplotypeTile {
            reference_sample: "GRCh38".into(),
            reference_contig: "chr1".into(),
            core_start: 100,
            core_end: 102,
            traversals: vec![WeightedTraversal {
                weight: 1,
                traversal: vec![forward_1, forward_2],
            }],
        }],
        ..RegionalGraph::default()
    }
}

fn conformance_named_graph() -> RegionalGraph {
    let forward_1 = OrientedNode {
        id: 1,
        reverse: false,
    };
    let forward_2 = OrientedNode {
        id: 2,
        reverse: false,
    };
    RegionalGraph {
        nodes: BTreeMap::from([(1, b"A".to_vec()), (2, b"C".to_vec())]),
        edges: BTreeSet::from([Edge {
            from: forward_1,
            to: forward_2,
        }]),
        semantics: HaplotypeSemantics::NamedPathsV3,
        paths: BTreeMap::from([
            (
                7,
                RegionalPath {
                    path_id: 7,
                    sample: "GRCh38".into(),
                    contig: "chr1".into(),
                    haplotype: 0,
                    fragment: 100,
                    is_reference: true,
                    visits: BTreeMap::from([(0, forward_1), (1, forward_2)]),
                },
            ),
            (
                8,
                RegionalPath {
                    path_id: 8,
                    sample: "sample-1".into(),
                    contig: "chr1".into(),
                    haplotype: 1,
                    fragment: 100,
                    is_reference: false,
                    visits: BTreeMap::from([(0, forward_1), (1, forward_2)]),
                },
            ),
        ]),
        reference_visits: BTreeSet::from([
            ReferenceVisit {
                path_id: 7,
                visit_index: 0,
                start: 100,
                end: 101,
                node: forward_1,
            },
            ReferenceVisit {
                path_id: 7,
                visit_index: 1,
                start: 101,
                end: 102,
                node: forward_2,
            },
        ]),
        ..RegionalGraph::default()
    }
}

fn conformance_archive(
    archive_version: u32,
    raw: &[u8],
) -> ExperimentResult<ConformanceArchiveParts> {
    let codec = ChunkCodec::Zstd3;
    let compressed = compress(codec, raw)?;
    let mut manifest = ReferenceManifest {
        sample: "GRCh38".into(),
        contig: "chr1".into(),
        start: 100,
        end: 102,
        grid_start: 0,
        window_size: 16_384,
        bucket_span: 16_384 * DIRECTORY_BUCKET_WINDOWS,
        first_page_offset: 0,
        page_count: 1,
        entry_count: 1,
        codec,
    };
    let provisional_root = encode_root_index(std::slice::from_ref(&manifest))?;
    manifest.first_page_offset = usize_to_u64(HEADER_LEN + provisional_root.len())?;
    let root = encode_root_index(std::slice::from_ref(&manifest))?;
    if root.len() != provisional_root.len() {
        return Err(invalid_data("conformance root length changed after offset assignment").into());
    }
    let data_offset = manifest
        .first_page_offset
        .checked_add(usize_to_u64(DIRECTORY_PAGE_BYTES)?)
        .ok_or_else(|| invalid_data("conformance data offset overflow"))?;
    let entry = ArchiveEntry {
        start: 100,
        end: 102,
        offset: data_offset,
        compressed_len: usize_to_u64(compressed.len())?,
        uncompressed_len: usize_to_u64(raw.len())?,
        codec,
    };
    let directory = encode_directory_page(&[entry], 0)?.to_vec();
    let mut header = encode_header(usize_to_u64(root.len())?, 1, data_offset).to_vec();
    if archive_version == 3 {
        header[..8].copy_from_slice(ARCHIVE_MAGIC_V3);
        header[8..12].copy_from_slice(&3_u32.to_le_bytes());
    } else if archive_version != ARCHIVE_VERSION {
        return Err(invalid_input(format!(
            "unsupported conformance archive version {archive_version}"
        ))
        .into());
    }
    let mut archive =
        Vec::with_capacity(header.len() + root.len() + directory.len() + compressed.len());
    archive.extend_from_slice(&header);
    archive.extend_from_slice(&root);
    archive.extend_from_slice(&directory);
    archive.extend_from_slice(&compressed);
    Ok(ConformanceArchiveParts {
        archive,
        header,
        root,
        directory,
        compressed,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn conformance_expected(
    graph: &RegionalGraph,
    query: &QuerySpec,
) -> ExperimentResult<serde_json::Value> {
    let selected = graph.select_nodes(query)?;
    let canonical = graph.canonical(&selected, query)?;
    let reference_traversal = if let Some(reference) = graph.reference_paths.first() {
        reference
            .traversal
            .iter()
            .map(|&node| pack_oriented(node).map(|value| value.to_string()))
            .collect::<io::Result<Vec<_>>>()?
    } else {
        graph
            .paths
            .values()
            .find(|path| path.is_reference)
            .map_or(Ok(Vec::new()), |path| {
                path.visits
                    .values()
                    .map(|&node| pack_oriented(node).map(|value| value.to_string()))
                    .collect::<io::Result<Vec<_>>>()
            })?
    };
    let mut traversals = Vec::new();
    if let Some(tile) = graph.haplotype_tiles.first() {
        for item in &tile.traversals {
            let nodes = item
                .traversal
                .iter()
                .map(|&node| pack_oriented(node).map(|value| value.to_string()))
                .collect::<io::Result<Vec<_>>>()?;
            traversals.push(serde_json::json!({
                "weight": item.weight.to_string(),
                "nodes": nodes
            }));
        }
    }
    Ok(serde_json::json!({
        "references": [{"sample": "GRCh38", "contig": "chr1", "start": 100, "end": 102}],
        "query": {"sample": query.sample, "contig": query.contig, "start": query.start, "end": query.end, "context": query.context},
        "canonicalHash": canonical.canonical_hash().to_hex().to_string(),
        "tile": {
            "semantics": graph.semantics.label(),
            "coreStart": 100,
            "coreEnd": 102,
            "nodeIds": graph.nodes.keys().map(ToString::to_string).collect::<Vec<_>>(),
            "nodeSequences": graph.nodes.values().map(|value| String::from_utf8_lossy(value)).collect::<Vec<_>>(),
            "edges": graph.edges.iter().flat_map(|edge| [pack_oriented(edge.from), pack_oriented(edge.to)]).collect::<io::Result<Vec<_>>>()?.into_iter().map(|value| value.to_string()).collect::<Vec<_>>(),
            "referenceTraversal": reference_traversal,
            "weightedTraversals": traversals,
            "namedPathIds": graph.paths.keys().map(ToString::to_string).collect::<Vec<_>>()
        }
    }))
}

fn write_conformance_fixture(
    directory: &Path,
    id: &str,
    archive_version: u32,
    regional_version: u32,
    graph: &RegionalGraph,
    raw: &[u8],
    query: &QuerySpec,
) -> ExperimentResult<serde_json::Value> {
    let parts = conformance_archive(archive_version, raw)?;
    let files = [
        (format!("{id}.pngr"), parts.archive),
        (format!("{id}.header.bin"), parts.header),
        (format!("{id}.root.bin"), parts.root),
        (format!("{id}.directory.bin"), parts.directory),
        (format!("{id}.payload.raw"), raw.to_vec()),
        (
            format!("{id}.payload.zstd1"),
            compress(ChunkCodec::Zstd1, raw)?,
        ),
        (format!("{id}.payload.zstd3"), parts.compressed),
        (
            format!("{id}.payload.zstd6"),
            compress(ChunkCodec::Zstd6, raw)?,
        ),
    ];
    let mut file_metadata = serde_json::Map::new();
    for (name, bytes) in files {
        std::fs::write(directory.join(&name), &bytes)?;
        file_metadata.insert(
            name,
            serde_json::json!({"bytes": bytes.len(), "sha256": sha256_hex(&bytes)}),
        );
    }
    Ok(serde_json::json!({
        "id": id,
        "archiveVersion": archive_version,
        "regionalVersion": regional_version,
        "semantics": graph.semantics.label(),
        "files": file_metadata,
        "expected": conformance_expected(graph, query)?
    }))
}

/// Deterministically exports the tiny cross-language conformance matrix.
///
/// # Errors
///
/// Returns an error when fixture encoding, compression, hashing, or output
/// fails. The generated two-node graphs contain no third-party source data.
pub fn export_conformance_fixtures(directory: impl AsRef<Path>) -> ExperimentResult<()> {
    let directory = directory.as_ref();
    std::fs::create_dir_all(directory)?;
    let query = QuerySpec {
        id: "conformance-100-102".into(),
        class: "cross-language-conformance".into(),
        sample: "GRCh38".into(),
        contig: "chr1".into(),
        start: 100,
        end: 102,
        context: CONSTRUCTION_CONTEXT,
    };
    let named = conformance_named_graph();
    let weighted = conformance_weighted_graph();
    let record_payload = conformance_record_payload();
    let record = record_payload.clone().into_regional_graph()?;
    let fixtures = vec![
        write_conformance_fixture(
            directory,
            "archive-v3-named-v2",
            3,
            2,
            &named,
            &named.encode()?,
            &query,
        )?,
        write_conformance_fixture(
            directory,
            "archive-v4-weighted-v3",
            4,
            3,
            &weighted,
            &weighted.encode()?,
            &query,
        )?,
        write_conformance_fixture(
            directory,
            "archive-v4-record-v4",
            4,
            4,
            &record,
            &record_payload.encode()?,
            &query,
        )?,
    ];
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "provenance": "deterministic synthetic two-node graph generated by the Rust reference encoder; no external source data",
        "supportedArchiveVersions": [3, 4],
        "supportedRegionalVersions": [2, 3, 4],
        "fixtures": fixtures
    });
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_hex(value: &str) -> Vec<u8> {
        let value = value.trim();
        assert_eq!(value.len() % 2, 0);
        (0..value.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
            .collect()
    }

    fn record_regional_golden() -> RecordRegionalPayload {
        let forward_1 = OrientedNode {
            id: 1,
            reverse: false,
        };
        let forward_2 = OrientedNode {
            id: 2,
            reverse: false,
        };
        RecordRegionalPayload {
            semantics: HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
            core_start: 100,
            core_end: 102,
            context: CONSTRUCTION_CONTEXT,
            reference_sample: "GRCh38".into(),
            reference_contig: "chr1".into(),
            reference_haplotype: 0,
            reference_fragment_start: 100,
            reference_query_offset: 0,
            reference_node_offset: 0,
            reference_position: (2, 0),
            nodes: BTreeMap::from([(1, b"A".to_vec()), (2, b"C".to_vec())]),
            edges: BTreeSet::from([Edge {
                from: forward_1,
                to: forward_2,
            }]),
            records: vec![
                PackedGbwtRecord {
                    handle: 2,
                    occurrence_count: 2,
                    bytes: vec![1, 4, 0, 1],
                },
                PackedGbwtRecord {
                    handle: 3,
                    occurrence_count: 2,
                    bytes: vec![1, 0, 0, 1],
                },
                PackedGbwtRecord {
                    handle: 4,
                    occurrence_count: 2,
                    bytes: vec![1, 0, 0, 1],
                },
                PackedGbwtRecord {
                    handle: 5,
                    occurrence_count: 2,
                    bytes: vec![1, 3, 0, 1],
                },
            ],
            total_occurrences: 8,
        }
    }

    fn v4_regional_graph() -> RegionalGraph {
        let forward = OrientedNode {
            id: 1,
            reverse: false,
        };
        RegionalGraph {
            nodes: BTreeMap::from([(1, b"AC".to_vec())]),
            semantics: HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
            reference_paths: vec![RegionalReferencePath {
                sample: "GRCh38".into(),
                contig: "chr6".into(),
                haplotype: 0,
                start: 100,
                end: 102,
                traversal: vec![forward],
            }],
            haplotype_tiles: vec![CanonicalHaplotypeTile {
                reference_sample: "GRCh38".into(),
                reference_contig: "chr6".into(),
                core_start: 100,
                core_end: 102,
                traversals: vec![WeightedTraversal {
                    weight: 7,
                    traversal: vec![forward],
                }],
            }],
            ..RegionalGraph::default()
        }
    }

    #[test]
    fn coalescing_respects_gap_threshold() {
        let ranges = vec![
            ByteRange { start: 10, end: 20 },
            ByteRange { start: 24, end: 30 },
            ByteRange { start: 40, end: 45 },
        ];
        assert_eq!(
            coalesce_ranges(ranges.clone(), 3),
            vec![
                ByteRange { start: 10, end: 20 },
                ByteRange { start: 24, end: 30 },
                ByteRange { start: 40, end: 45 },
            ]
        );
        assert_eq!(
            coalesce_ranges(ranges, 4),
            vec![
                ByteRange { start: 10, end: 30 },
                ByteRange { start: 40, end: 45 }
            ]
        );
    }

    #[test]
    fn adaptive_split_enforces_raw_byte_cap_and_minimum_window() {
        assert_eq!(
            adaptive_split_interval(100, 200, 101, 100, 10).unwrap(),
            Some([(100, 150), (150, 200)])
        );
        assert_eq!(
            adaptive_split_interval(100, 200, 100, 100, 10).unwrap(),
            None
        );
        assert!(adaptive_split_interval(100, 110, 101, 100, 10).is_err());
    }

    #[test]
    fn temporary_archive_cleanup_is_default_and_retention_is_explicit() {
        let output = simple_sds::serialize::temp_file_name("pangenome-range-cleanup");
        let temporary_path = {
            let temporary = TemporaryFile::create_near(&output, "archive").unwrap();
            let path = temporary.path.clone();
            assert!(path.is_file());
            path
        };
        assert!(!temporary_path.exists());

        let retained_path = {
            let mut temporary = TemporaryFile::create_near(&output, "archive").unwrap();
            temporary.keep_on_failure();
            temporary.path.clone()
        };
        assert!(retained_path.is_file());
        std::fs::remove_file(retained_path).unwrap();
    }

    #[test]
    fn regional_graph_round_trip() {
        let mut graph = RegionalGraph::default();
        graph.nodes.insert(1, b"AC".to_vec());
        graph.paths.insert(
            7,
            RegionalPath {
                path_id: 7,
                sample: "sample".into(),
                contig: "chr1".into(),
                haplotype: 1,
                fragment: 10,
                is_reference: false,
                visits: BTreeMap::from([(
                    0,
                    OrientedNode {
                        id: 1,
                        reverse: false,
                    },
                )]),
            },
        );
        let encoded = graph.encode().unwrap();
        let decoded = RegionalGraph::decode(&encoded).unwrap();
        assert_eq!(decoded.nodes, graph.nodes);
        assert_eq!(decoded.paths[&7].visits, graph.paths[&7].visits);
    }

    #[test]
    fn regional_v4_round_trip_preserves_weighted_tile_semantics() {
        let graph = v4_regional_graph();
        let encoded = graph.encode().unwrap();
        assert_eq!(
            blake3::hash(&encoded).to_hex().as_str(),
            "47952df5707d5eca5cc33807215e54dab095226169e56fe2f802779d619e9663",
            "the embedded deterministic v4 golden payload changed",
        );
        assert_eq!(&encoded[..8], REGION_MAGIC_WEIGHTED_V3);
        let decoded = RegionalGraph::decode(&encoded).unwrap();
        assert_eq!(decoded.nodes, graph.nodes);
        assert_eq!(decoded.reference_paths, graph.reference_paths);
        assert_eq!(decoded.haplotype_tiles, graph.haplotype_tiles);
        assert_eq!(decoded.semantics, graph.semantics);
    }

    #[test]
    fn regional_v4_rejects_unknown_semantics_truncation_and_count_overflow() {
        let encoded = v4_regional_graph().encode().unwrap();

        let mut unknown_semantics = encoded.clone();
        unknown_semantics[16] = 99;
        assert!(RegionalGraph::decode(&unknown_semantics).is_err());

        assert!(RegionalGraph::decode(&encoded[..encoded.len() - 1]).is_err());

        let mut impossible_node_count = encoded;
        impossible_node_count[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(RegionalGraph::decode(&impossible_node_count).is_err());
    }

    #[test]
    fn record_regional_v4_golden_reconstructs_weighted_paths() {
        let encoded = record_regional_golden().encode().unwrap();
        let fixture = decode_hex(include_str!(
            "../../../test-data/golden/record-region-v4.hex"
        ));
        assert_eq!(encoded, fixture);
        assert_eq!(
            blake3::hash(&encoded).to_hex().as_str(),
            "46dbb25184ffadfb063a41665f6350b9b47f42a0021d46cf747ae72271db7dc6"
        );
        let compressed = decode_hex(include_str!(
            "../../../test-data/golden/record-region-v4.zstd3.hex"
        ));
        assert_eq!(
            zstd::bulk::decompress(&compressed, encoded.len()).unwrap(),
            encoded
        );
        assert_eq!(&encoded[..8], REGION_MAGIC);
        let decoded = RecordRegionalPayload::decode(&encoded)
            .unwrap()
            .into_regional_graph()
            .unwrap();
        assert_eq!(
            decoded.haplotype_tiles[0]
                .canonical_hash()
                .to_hex()
                .as_str(),
            "4802ea7b9f9821318089c07c4647702218f6076ea7d987be904eca7c4bde3b7b"
        );
        assert_eq!(
            decoded.nodes,
            BTreeMap::from([(1, b"A".to_vec()), (2, b"C".to_vec())])
        );
        assert_eq!(
            decoded.reference_paths,
            vec![RegionalReferencePath {
                sample: "GRCh38".into(),
                contig: "chr1".into(),
                haplotype: 0,
                start: 100,
                end: 102,
                traversal: vec![
                    OrientedNode {
                        id: 1,
                        reverse: false,
                    },
                    OrientedNode {
                        id: 2,
                        reverse: false,
                    },
                ],
            }]
        );
        assert_eq!(decoded.haplotype_tiles[0].traversals.len(), 1);
        assert_eq!(decoded.haplotype_tiles[0].traversals[0].weight, 1);
        assert_eq!(
            decoded.haplotype_tiles[0].traversals[0].traversal,
            decoded.reference_paths[0].traversal
        );
    }

    #[test]
    fn record_regional_v4_rejects_corrupt_counts_runs_and_reference_offsets() {
        let encoded = record_regional_golden().encode().unwrap();
        assert!(RecordRegionalPayload::decode(&encoded[..encoded.len() - 1]).is_err());

        let mut impossible_occurrences = encoded.clone();
        impossible_occurrences[48..56].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(RecordRegionalPayload::decode(&impossible_occurrences).is_err());

        let mut invalid_reference_offset = encoded.clone();
        invalid_reference_offset[120..128].copy_from_slice(&2_u64.to_le_bytes());
        assert!(RecordRegionalPayload::decode(&invalid_reference_offset).is_err());

        let mut invalid_run = encoded;
        *invalid_run.last_mut().unwrap() = 255;
        assert!(RecordRegionalPayload::decode(&invalid_run).is_err());
    }

    #[test]
    fn record_archive_v4_golden_is_deterministic_and_matches_source_oracle() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/micb-kir3dl1.gbz");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-data/golden/record-archive-v4.pngr");
        let graph: GBZ = simple_sds::serialize::load_from(&source).unwrap();
        let path_index = PathIndex::new(&graph, 1_000, false).unwrap();
        let config = FixedArchiveConfig {
            experiment_id: "record-archive-golden".into(),
            window_size: 16_384,
            codec: ChunkCodec::Zstd3,
            deduplicate_chunks: false,
            max_uncompressed_chunk_bytes: DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES,
            min_window_size: DEFAULT_MIN_WINDOW_SIZE,
        };
        let output = simple_sds::serialize::temp_file_name("pangenome-range-record-golden");
        build_fixed_archive_with_options(
            &graph,
            &path_index,
            std::fs::metadata(&source).unwrap().len(),
            &output,
            &config,
            &ArchiveBuildOptions {
                sample: Some("CHM13".into()),
                contig: Some("chr6".into()),
                start: Some(31_350_872),
                end: Some(31_351_896),
                threads: 1,
                ..ArchiveBuildOptions::default()
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read(&output).unwrap(),
            std::fs::read(&fixture).unwrap()
        );

        let query = QuerySpec {
            id: "record-archive-golden".into(),
            class: "golden".into(),
            sample: "CHM13".into(),
            contig: "chr6".into(),
            start: 31_351_000,
            end: 31_351_500,
            context: 100,
        };
        let oracle = source_oracle(&graph, &path_index, &query).unwrap();
        let measurement = query_fixed_archive(
            &fixture,
            &config,
            &query,
            65_536,
            &oracle,
            &graph,
            &path_index,
        )
        .unwrap();
        assert!(measurement.correctness);
        assert!(measurement.haplotype_tiles_correct);
        let validation = validate_fixed_archive(&fixture).unwrap();
        assert_eq!(validation.archive_version, 4);
        assert_eq!(validation.reference_manifests, 1);
        assert_eq!(validation.directory_entries, 1);
        assert_eq!(validation.physical_payloads, 1);

        let corrupt = simple_sds::serialize::temp_file_name("pangenome-range-reserved-header");
        let mut corrupt_bytes = std::fs::read(&fixture).unwrap();
        corrupt_bytes[48] = 1;
        std::fs::write(&corrupt, corrupt_bytes).unwrap();
        assert!(validate_fixed_archive(&corrupt).is_err());
        std::fs::remove_file(corrupt).unwrap();
        std::fs::remove_file(output).unwrap();
    }

    #[test]
    fn conformance_fixture_export_is_deterministic_and_rust_readable() {
        let directory =
            simple_sds::serialize::temp_file_name("pangenome-range-cross-language-conformance");
        export_conformance_fixtures(&directory).unwrap();
        let manifest_path = directory.join("manifest.json");
        let first_manifest = std::fs::read(&manifest_path).unwrap();
        let document: serde_json::Value = serde_json::from_slice(&first_manifest).unwrap();
        assert_eq!(
            document["supportedArchiveVersions"],
            serde_json::json!([3, 4])
        );
        assert_eq!(
            document["supportedRegionalVersions"],
            serde_json::json!([2, 3, 4])
        );
        for fixture in document["fixtures"].as_array().unwrap() {
            let archive_name = fixture["files"]
                .as_object()
                .unwrap()
                .keys()
                .find(|name| {
                    Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("pngr"))
                })
                .unwrap();
            let summary = validate_fixed_archive(&directory.join(archive_name)).unwrap();
            assert_eq!(summary.physical_payloads, 1);
            assert_eq!(summary.directory_entries, 1);
        }
        export_conformance_fixtures(&directory).unwrap();
        assert_eq!(std::fs::read(&manifest_path).unwrap(), first_manifest);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn mhc_all_and_distinct_local_traversals_are_exactly_equivalent() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mhc-10.gbz");
        if !path.is_file() {
            eprintln!(
                "skipping MHC adapter test because {} is absent",
                path.display()
            );
            return;
        }
        let graph: GBZ = simple_sds::serialize::load_from(&path).unwrap();
        let path_index = PathIndex::new(&graph, 1_000, false).unwrap();
        let reference = reference_paths(&graph).unwrap().remove(0);
        let core_end = reference.end.min(reference.start + 1_000);
        let evidence =
            compare_haplotype_outputs(&graph, &path_index, &reference, reference.start, core_end)
                .unwrap();
        eprintln!("MHC All-vs-Distinct evidence: {evidence:?}");
        assert!(evidence.exact_multiset_equivalent);
        assert_eq!(evidence.all.total_weight, evidence.distinct.total_weight);
        assert_eq!(
            evidence.all.weighted_traversal_nodes,
            evidence.distinct.weighted_traversal_nodes
        );
    }

    #[test]
    fn mhc_direct_writer_is_byte_identical_across_thread_counts() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mhc-10.gbz");
        if !source.is_file() {
            eprintln!(
                "skipping direct-writer determinism test because {} is absent",
                source.display()
            );
            return;
        }
        let graph: GBZ = simple_sds::serialize::load_from(&source).unwrap();
        let path_index = PathIndex::new(&graph, 1_000, false).unwrap();
        let reference = reference_paths(&graph).unwrap().remove(0);
        let first = simple_sds::serialize::temp_file_name("pangenome-range-direct-one");
        let parallel = simple_sds::serialize::temp_file_name("pangenome-range-direct-parallel");
        let config = FixedArchiveConfig {
            experiment_id: "direct-determinism".into(),
            window_size: 16_384,
            codec: ChunkCodec::Zstd3,
            deduplicate_chunks: false,
            max_uncompressed_chunk_bytes: DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES,
            min_window_size: DEFAULT_MIN_WINDOW_SIZE,
        };
        let base_options = ArchiveBuildOptions {
            sample: Some(reference.name.sample),
            contig: Some(reference.name.contig),
            max_chunks: Some(4),
            progress: BuildProgressMode::Plain,
            progress_interval_ms: 0,
            ..ArchiveBuildOptions::default()
        };
        let single_thread = build_fixed_archive_with_options(
            &graph,
            &path_index,
            std::fs::metadata(&source).unwrap().len(),
            &first,
            &config,
            &base_options,
        )
        .unwrap();
        assert_eq!(single_thread.reference_bases_processed, 4 * 16_384);
        assert_eq!(single_thread.total_reference_bases, 4 * 16_384);
        assert_eq!(single_thread.progress_events_emitted, 4);
        let parallel_build = build_fixed_archive_with_options(
            &graph,
            &path_index,
            std::fs::metadata(&source).unwrap().len(),
            &parallel,
            &config,
            &ArchiveBuildOptions {
                threads: 4,
                ..base_options
            },
        )
        .unwrap();
        assert_eq!(parallel_build.reference_bases_processed, 4 * 16_384);
        assert_eq!(parallel_build.total_reference_bases, 4 * 16_384);
        assert_eq!(parallel_build.progress_events_emitted, 4);
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&parallel).unwrap()
        );
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(parallel).unwrap();
    }

    #[test]
    fn mhc_direct_typed_extraction_matches_json_oracle() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mhc-10.gbz");
        if !source.is_file() {
            eprintln!(
                "skipping direct typed extraction comparison because {} is absent",
                source.display()
            );
            return;
        }
        let graph: GBZ = simple_sds::serialize::load_from(&source).unwrap();
        let path_index = PathIndex::new(&graph, 1_000, false).unwrap();
        let reference = reference_paths(&graph).unwrap().remove(0);
        for offset in [0, 16_384, 131_072, 524_288] {
            let start = reference.start + offset;
            let end = (start + 16_384).min(reference.end);
            if start >= end {
                continue;
            }
            let mut oracle_subgraph = Subgraph::new();
            let (oracle, _) = extract_local_region_with_subgraph(
                &graph,
                &path_index,
                &mut oracle_subgraph,
                &reference.name,
                start,
                end,
                CONSTRUCTION_CONTEXT,
                HaplotypeOutput::Distinct,
            )
            .unwrap();
            let mut direct_subgraph = Subgraph::new();
            let (direct, _) = extract_local_region_direct(
                &graph,
                &path_index,
                &mut direct_subgraph,
                &reference.name,
                start,
                end,
                CONSTRUCTION_CONTEXT,
            )
            .unwrap();
            assert_eq!(
                direct.encode().unwrap(),
                oracle.encode().unwrap(),
                "direct typed extraction differs at {start}-{end}"
            );
        }
    }

    #[test]
    fn mhc_record_payload_matches_json_oracle() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mhc-10.gbz");
        if !source.is_file() {
            eprintln!(
                "skipping record payload comparison because {} is absent",
                source.display()
            );
            return;
        }
        let graph: GBZ = simple_sds::serialize::load_from(&source).unwrap();
        let path_index = PathIndex::new(&graph, 1_000, false).unwrap();
        let reference = reference_paths(&graph).unwrap().remove(0);
        for offset in [0, 16_384, 131_072, 524_288] {
            let start = reference.start + offset;
            let end = (start + 16_384).min(reference.end);
            if start >= end {
                continue;
            }
            let query_name = FullPathName {
                sample: reference.name.sample.clone(),
                contig: reference.name.contig.clone(),
                haplotype: reference.name.haplotype,
                fragment: 0,
            };
            let RecordChunkOutcome::Accepted(chunk) =
                construct_record_chunk(&graph, &path_index, &query_name, start, end, u64::MAX)
                    .unwrap()
            else {
                panic!("test record payload unexpectedly requested an adaptive split");
            };
            let decoded = RegionalGraph::decode(&chunk.raw).unwrap();
            let mut oracle_subgraph = Subgraph::new();
            let (oracle, _) = extract_local_region_with_subgraph(
                &graph,
                &path_index,
                &mut oracle_subgraph,
                &reference.name,
                start,
                end,
                CONSTRUCTION_CONTEXT,
                HaplotypeOutput::Distinct,
            )
            .unwrap();
            assert_eq!(decoded.nodes, oracle.nodes, "nodes differ at {start}-{end}");
            assert_eq!(decoded.edges, oracle.edges, "edges differ at {start}-{end}");
            assert_eq!(
                decoded.reference_paths, oracle.reference_paths,
                "reference traversal differs at {start}-{end}"
            );
            assert_eq!(
                decoded.haplotype_tiles, oracle.haplotype_tiles,
                "weighted local paths differ at {start}-{end}"
            );
        }
    }

    #[test]
    fn mhc_sliding_subgraph_experiment_preserves_local_extraction() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/mhc-10.gbz");
        if !source.is_file() {
            eprintln!(
                "skipping sliding Subgraph experiment because {} is absent",
                source.display()
            );
            return;
        }
        let graph: GBZ = simple_sds::serialize::load_from(&source).unwrap();
        let path_index = PathIndex::new(&graph, 1_000, false).unwrap();
        let reference = reference_paths(&graph).unwrap().remove(0);
        let intervals = (0..32_u64)
            .filter_map(|index| {
                let start = reference.start + index * 16_384;
                let end = (start + 16_384).min(reference.end);
                (start < end).then_some((start, end))
            })
            .collect::<Vec<_>>();
        let query_name = FullPathName {
            sample: reference.name.sample,
            contig: reference.name.contig,
            haplotype: reference.name.haplotype,
            fragment: 0,
        };
        let extract = |subgraph: &mut Subgraph, start, end| {
            let query = SubgraphQuery::path_interval(
                &query_name,
                usize::try_from(start).unwrap()..usize::try_from(end).unwrap(),
            )
            .with_context(usize::try_from(CONSTRUCTION_CONTEXT).unwrap())
            .with_haplotypes(HaplotypeOutput::Distinct);
            subgraph
                .from_gbz(&graph, Some(&path_index), None, &query)
                .unwrap();
            let mut json = Vec::new();
            subgraph.write_json(&mut json, false).unwrap();
            blake3::hash(&json)
        };
        let fresh_started = Instant::now();
        let fresh = intervals
            .iter()
            .map(|&(start, end)| extract(&mut Subgraph::new(), start, end))
            .collect::<Vec<_>>();
        let fresh_ms = fresh_started.elapsed().as_secs_f64() * 1_000.0;
        let reused_started = Instant::now();
        let mut reused_subgraph = Subgraph::new();
        let reused = intervals
            .iter()
            .map(|&(start, end)| extract(&mut reused_subgraph, start, end))
            .collect::<Vec<_>>();
        let reused_ms = reused_started.elapsed().as_secs_f64() * 1_000.0;
        assert_eq!(fresh, reused);
        eprintln!(
            "sliding Subgraph experiment: chunks={} fresh_ms={fresh_ms:.3} reused_ms={reused_ms:.3} speedup={:.3}",
            intervals.len(),
            fresh_ms / reused_ms
        );
    }

    #[test]
    fn boundary_edge_activates_after_both_endpoint_chunks_are_assembled() {
        let mut graph = RegionalGraph::default();
        let from = OrientedNode {
            id: 1,
            reverse: false,
        };
        let to = OrientedNode {
            id: 2,
            reverse: false,
        };
        graph.nodes.insert(from.id, b"A".to_vec());
        graph.edges.insert(Edge { from, to });

        assert!(graph.adjacency().is_empty());

        graph.nodes.insert(to.id, b"C".to_vec());
        assert_eq!(graph.adjacency().get(&from), Some(&BTreeSet::from([to])));
    }

    #[test]
    fn fixed_directory_page_round_trip_preserves_adaptive_entries() {
        let entries = (0..64_u64)
            .map(|index| ArchiveEntry {
                start: index * 8_192,
                end: (index + 1) * 8_192,
                offset: 1_000_000 + index * 100,
                compressed_len: 100,
                uncompressed_len: 200,
                codec: ChunkCodec::Zstd3,
            })
            .collect::<Vec<_>>();
        let manifest = ReferenceManifest {
            sample: "sample".into(),
            contig: "chr1".into(),
            start: 0,
            end: 32 * 16_384,
            grid_start: 0,
            window_size: 16_384,
            bucket_span: 32 * 16_384,
            first_page_offset: 512,
            page_count: 1,
            entry_count: 64,
            codec: ChunkCodec::Zstd3,
        };
        let encoded = encode_directory_page(&entries, 0).unwrap();
        assert_eq!(encoded.len(), DIRECTORY_PAGE_BYTES);
        let decoded = decode_directory_page(&encoded, &manifest, 0).unwrap();
        assert_eq!(decoded.entries.len(), entries.len());
        assert_eq!(decoded.entries[31].start, entries[31].start);
        assert_eq!(decoded.entries[31].offset, entries[31].offset);
    }

    #[test]
    fn root_index_round_trip_preserves_arithmetic_manifests() {
        let mut manifests = vec![
            ReferenceManifest {
                sample: "sample".into(),
                contig: "chr1".into(),
                start: 0,
                end: 2 * 32 * 16_384,
                grid_start: 0,
                window_size: 16_384,
                bucket_span: 32 * 16_384,
                first_page_offset: 0,
                page_count: 2,
                entry_count: 2,
                codec: ChunkCodec::Zstd3,
            },
            ReferenceManifest {
                sample: "sample-2".into(),
                contig: "chr2".into(),
                start: 0,
                end: 32 * 16_384,
                grid_start: 0,
                window_size: 16_384,
                bucket_span: 32 * 16_384,
                first_page_offset: 0,
                page_count: 1,
                entry_count: 3,
                codec: ChunkCodec::Zstd3,
            },
        ];
        let provisional = encode_root_index(&manifests).unwrap();
        let root_end = u64::try_from(HEADER_LEN + provisional.len()).unwrap();
        manifests[0].first_page_offset = root_end;
        manifests[1].first_page_offset = root_end + 2 * DIRECTORY_PAGE_BYTES as u64;
        let encoded = encode_root_index(&manifests).unwrap();
        let header = Header {
            version: ARCHIVE_VERSION,
            root_len: u64::try_from(encoded.len()).unwrap(),
            entry_count: 5,
            data_offset: root_end + 3 * DIRECTORY_PAGE_BYTES as u64,
        };

        let decoded = decode_root_index(&encoded, header).unwrap();
        assert_eq!(decoded.manifests.len(), manifests.len());
        assert_eq!(
            decoded.manifests[0].first_page_offset,
            manifests[0].first_page_offset
        );
        assert_eq!(decoded.manifests[1].entry_count, 3);
        assert_eq!(
            directory_page_offset(&decoded.manifests[0], 1).unwrap(),
            root_end + DIRECTORY_PAGE_BYTES as u64
        );
        assert_eq!(decoded.logical_bytes, root_end);
    }

    #[test]
    fn reusable_reader_caches_fixed_directory_pages() {
        let path = simple_sds::serialize::temp_file_name("pangenome-range-v3-cache");
        let mut manifests = vec![ReferenceManifest {
            sample: "sample".into(),
            contig: "chr1".into(),
            start: 0,
            end: 32 * 16_384,
            grid_start: 0,
            window_size: 16_384,
            bucket_span: 32 * 16_384,
            first_page_offset: 0,
            page_count: 1,
            entry_count: 1,
            codec: ChunkCodec::None,
        }];
        let provisional = encode_root_index(&manifests).unwrap();
        let root_end = u64::try_from(HEADER_LEN + provisional.len()).unwrap();
        manifests[0].first_page_offset = root_end;
        let root = encode_root_index(&manifests).unwrap();
        let data_offset = root_end + DIRECTORY_PAGE_BYTES as u64;
        let page = encode_directory_page(
            &[ArchiveEntry {
                start: 0,
                end: 16_384,
                offset: data_offset,
                compressed_len: 1,
                uncompressed_len: 1,
                codec: ChunkCodec::None,
            }],
            0,
        )
        .unwrap();
        let mut file = File::create(&path).unwrap();
        file.write_all(&encode_header(
            u64::try_from(root.len()).unwrap(),
            1,
            data_offset,
        ))
        .unwrap();
        file.write_all(&root).unwrap();
        file.write_all(&page).unwrap();
        file.write_all(&[0]).unwrap();
        file.flush().unwrap();
        drop(file);

        let query = QuerySpec {
            id: "cache".into(),
            class: "test".into(),
            sample: "sample".into(),
            contig: "chr1".into(),
            start: 1,
            end: 100,
            context: 0,
        };
        let mut reader = FixedArchiveReader::open(&path).unwrap();
        let first = reader.lookup_directory_cached(&query).unwrap();
        let second = reader.lookup_directory_cached(&query).unwrap();
        assert_eq!(first.cache_hits, 0);
        assert_eq!(second.cache_hits, 1);
        assert_eq!(second.entries.len(), 1);

        drop(reader);
        std::fs::remove_file(path).unwrap();
    }
}
