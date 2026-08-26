use gbz::bwt::{BWT, Record};
use gbz::support;
use gbz::{FullPathName, GBWT, GBZ, Orientation, Pos};
use gbz_base::{HaplotypeOutput, PathIndex, Subgraph, SubgraphQuery};
use pangenome_range_format::{
    ARCHIVE_VERSION, ArchiveEntry, ArchiveValidationProgress, Bootstrap, DIRECTORY_BUCKET_WINDOWS,
    DIRECTORY_ENTRIES_PER_PAGE, DIRECTORY_PAGE_BYTES, FileRangeSource, HEADER_LEN, NetworkProfile,
    PackedEdge, PackedGbwtRecord, REGION_VERSION, RangeSource, RecordRegionalPayload,
    ReferenceManifest, TracingRangeSource, bootstrap as format_bootstrap,
    compress as format_compress, decode_directory_page as format_decode_directory_page,
    decompress as format_decompress, directory_page_offset as format_directory_page_offset,
    encode_directory_page as format_encode_directory_page,
    encode_extension_directory as format_encode_extension_directory,
    encode_header as format_encode_header,
    encode_header_with_extensions as format_encode_header_with_extensions,
    encode_root_index as format_encode_root_index,
    validate_archive_with_options as format_validate_archive_with_options,
    validate_archive_with_progress as format_validate_archive_with_progress,
};
use pangenome_range_format::{ExtensionEntry, ValidationMode, ValidationOptions};
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

use crate::source::{LoadedGbzSource, PangenomeSource};

pub type ExperimentResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub use pangenome_range_format::ArchiveValidationSummary;
pub use pangenome_range_format::ChunkCodec;
pub const CONSTRUCTION_CONTEXT: u64 = 100;
pub const DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_MIN_WINDOW_SIZE: u64 = 1024;
pub const DEFAULT_MAX_QUEUED_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_PROGRESS_INTERVAL_MS: u64 = 5_000;
const MAX_DECODED_OCCURRENCES_PER_TILE: u64 = 16 * 1024 * 1024;
const DEFAULT_DIRECTORY_CACHE_BYTES: usize = 1024 * 1024;

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
        BuildProgressMode::Plain => eprintln!("[{phase}] {message}"),
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

fn format_integer(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    let first_group = digits.len() % 3;
    for (index, byte) in digits.bytes().enumerate() {
        if index > 0 && index % 3 == first_group {
            formatted.push(',');
        }
        formatted.push(char::from(byte));
    }
    formatted
}

#[allow(clippy::cast_precision_loss)]
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[allow(clippy::cast_precision_loss)]
fn format_bases(bases: u64) -> String {
    if bases >= 1_000_000_000 {
        format!("{:.3} Gbp", bases as f64 / 1_000_000_000.0)
    } else if bases >= 1_000_000 {
        format!("{:.2} Mbp", bases as f64 / 1_000_000.0)
    } else if bases >= 1_000 {
        format!("{:.1} kbp", bases as f64 / 1_000.0)
    } else {
        format!("{bases} bp")
    }
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown".into();
    }
    let rounded = seconds.round() as u64;
    let hours = rounded / 3_600;
    let minutes = (rounded % 3_600) / 60;
    let seconds = rounded % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn emit_progress_snapshot(mode: BuildProgressMode, snapshot: &BuildProgressSnapshot) {
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!(
            "[encode] {:6.2}% | {}/{} | {} chunks | {:.2} Mbp/s | ETA {} | elapsed {} | output {} | ref {}/{} {}#{}:{}-{}",
            snapshot.percent_complete,
            format_bases(snapshot.processed_reference_bases),
            format_bases(snapshot.total_reference_bases),
            format_integer(snapshot.physical_chunks),
            snapshot.reference_bp_per_second / 1_000_000.0,
            snapshot
                .estimated_seconds_remaining
                .map_or_else(|| "unknown".into(), format_duration),
            format_duration(snapshot.processing_elapsed_seconds),
            format_bytes(snapshot.temporary_archive_bytes),
            snapshot.reference_ordinal,
            snapshot.reference_count,
            snapshot.sample,
            snapshot.contig,
            format_integer(snapshot.current_chunk_start),
            format_integer(snapshot.current_chunk_end),
        ),
        BuildProgressMode::Json => eprintln!(
            "{}",
            serde_json::to_string(snapshot).expect("progress snapshot is JSON-serializable")
        ),
    }
}

fn emit_validation_progress(mode: BuildProgressMode, snapshot: &ArchiveValidationProgress) {
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!(
            "[validate] {:6.2}% | {}/{} entries | {} payloads | {}/{} pages | {:.0} entries/s | ETA {} | elapsed {} | read {}",
            snapshot.percent_complete,
            format_integer(snapshot.directory_entries_validated),
            format_integer(snapshot.directory_entries_total),
            format_integer(snapshot.physical_payloads_validated),
            format_integer(snapshot.directory_pages_validated),
            format_integer(snapshot.directory_pages_total),
            snapshot.entries_per_second,
            snapshot
                .estimated_seconds_remaining
                .map_or_else(|| "unknown".into(), format_duration),
            format_duration(snapshot.elapsed_seconds),
            format_bytes(snapshot.compressed_payload_bytes_validated),
        ),
        BuildProgressMode::Json => eprintln!(
            "{}",
            serde_json::to_string(snapshot).expect("validation progress is JSON-serializable")
        ),
    }
}

#[allow(clippy::cast_precision_loss)]
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
            "[encode] plan: {} references, {}, {} base windows, {} index prefix; progress every {}",
            format_integer(reference_count),
            format_bases(total_reference_bases),
            format_integer(planned_base_windows),
            format_bytes(temporary_archive_prefix_bytes),
            format_duration(progress_interval_ms as f64 / 1_000.0),
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
        BuildProgressMode::Off | BuildProgressMode::Plain => {}
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

#[derive(Clone, Debug)]
pub struct OracleResult {
    pub canonical: CanonicalSubgraph,
    pub encoded: Vec<u8>,
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
struct ArchiveIndex {
    entries: Vec<ArchiveEntry>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct RegionalGraph {
    nodes: BTreeMap<u64, Vec<u8>>,
    edges: BTreeSet<Edge>,
    semantics: HaplotypeSemantics,
    reference_paths: Vec<RegionalReferencePath>,
    haplotype_tiles: Vec<CanonicalHaplotypeTile>,
}

impl Default for RegionalGraph {
    fn default() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeSet::new(),
            semantics: HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
            reference_paths: Vec::new(),
            haplotype_tiles: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct StoredChunk {
    archive_offset: u64,
    compressed_len: u64,
    uncompressed_len: u64,
    integrity: [u8; 16],
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
    LoadedGbzSource::new(graph, None)
        .references(sample_filter, contig_filter)
        .map(|references| {
            references
                .into_iter()
                .map(|reference| ReferencePathSpec {
                    name: reference.name,
                    start: reference.start,
                    end: reference.end,
                })
                .collect()
        })
        .map_err(Into::into)
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

#[cfg(test)]
#[derive(Debug)]
struct DirectPathRecord {
    handle: usize,
    successors: Vec<Pos>,
    has_predecessor: Vec<bool>,
    sequence_len: usize,
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
    let source = LoadedGbzSource::new(graph, Some(path_index));
    let position = source.reference_position(query_pos)?;
    Ok(DirectReferencePosition {
        query_offset: position.query_offset,
        node_offset: position.node_offset,
        gbwt_pos: position.position,
        path_name: position.path_name,
    })
}

#[cfg(test)]
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

fn record_payload_from_subgraph(
    graph: &GBZ,
    subgraph: &Subgraph,
    reference: &DirectReferencePosition,
    core_start: u64,
    core_end: u64,
) -> ExperimentResult<RecordRegionalPayload> {
    let mut nodes = BTreeMap::new();
    let mut edges = BTreeSet::new();
    for node_id in subgraph.node_iter() {
        let sequence = subgraph
            .sequence(node_id)
            .ok_or_else(|| invalid_data(format!("missing local sequence for node {node_id}")))?;
        nodes.insert(usize_to_u64(node_id)?, sequence.to_vec());
        for orientation in [Orientation::Forward, Orientation::Reverse] {
            for (next_id, next_orientation) in subgraph
                .supergraph_successors(node_id, orientation)
                .ok_or_else(|| invalid_data(format!("missing local node {node_id}")))?
            {
                if support::edge_is_canonical((node_id, orientation), (next_id, next_orientation)) {
                    edges.insert(PackedEdge {
                        from: pack_oriented(oriented(node_id, orientation)?)?,
                        to: pack_oriented(oriented(next_id, next_orientation)?)?,
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
                .ok_or_else(|| invalid_data(format!("missing GBWT record for handle {handle}")))?
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
    Ok(RecordRegionalPayload {
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
    let payload = record_payload_from_subgraph(graph, &subgraph, &reference_position, start, end)?;
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

trait RecordRegionalPayloadExt {
    fn into_regional_graph(self) -> ExperimentResult<RegionalGraph>;
}

impl RecordRegionalPayloadExt for RecordRegionalPayload {
    fn into_regional_graph(self) -> ExperimentResult<RegionalGraph> {
        let reconstructed = self.reconstruct_traversals()?;
        let traversals = reconstructed
            .anonymous
            .into_iter()
            .map(|item| WeightedTraversal {
                weight: item.weight,
                traversal: item.handles.into_iter().map(unpack_oriented).collect(),
            })
            .collect();
        let edges = self
            .edges
            .into_iter()
            .map(|edge| Edge {
                from: unpack_oriented(edge.from),
                to: unpack_oriented(edge.to),
            })
            .collect();
        let mut result = RegionalGraph {
            nodes: self.nodes,
            edges,
            semantics: HaplotypeSemantics::AnonymousDistinctWeightedTilePaths,
            ..RegionalGraph::default()
        };
        result.reference_paths.push(RegionalReferencePath {
            sample: self.reference_sample.clone(),
            contig: self.reference_contig.clone(),
            haplotype: self.reference_haplotype,
            start: reconstructed.reference_start,
            end: reconstructed.reference_end,
            traversal: reconstructed
                .reference_handles
                .into_iter()
                .map(unpack_oriented)
                .collect(),
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
                    integrity: blake3::hash(&compressed).as_bytes()[..16]
                        .try_into()
                        .expect("fixed digest"),
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
            integrity: stored.integrity,
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
/// Builds a filtered v1 archive through the bounded direct-write pipeline.
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
    let validation = validate_fixed_archive_with_options(
        &archive_temp.path,
        options.progress,
        ValidationOptions {
            mode: ValidationMode::Standard,
            workers: options.threads,
            max_queued_bytes: options.max_queued_bytes,
            progress_interval_ms: options.progress_interval_ms,
        },
    )?;
    let archive_validation_wall_ms = validation_started.elapsed().as_secs_f64() * 1_000.0;
    emit_progress(
        options.progress,
        "archive_validation_complete",
        &format!(
            "validated {} directory pages and {} physical payloads in {}",
            format_integer(validation.directory_pages),
            format_integer(validation.physical_payloads),
            format_duration(validation.validation_wall_ms / 1_000.0),
        ),
    );
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
                    .ok_or_else(|| invalid_data("v1 query chunk has no haplotype tile"))?;
                if tile.core_start != entry.start || tile.core_end != entry.end {
                    return Err(invalid_data(
                        "v1 tile provenance does not match its directory entry",
                    )
                    .into());
                }
                let reference = chunk
                    .reference_paths
                    .first()
                    .ok_or_else(|| invalid_data("v1 query chunk has no reference path"))?;
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
                canonical.mismatch_summary(&oracle.canonical)
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
        Ok(())
    }

    fn select_nodes(&self, query: &QuerySpec) -> ExperimentResult<BTreeSet<u64>> {
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
                "no reference traversal overlaps query {}",
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

    fn decode(bytes: &[u8]) -> ExperimentResult<Self> {
        RecordRegionalPayload::decode(bytes)?.into_regional_graph()
    }
}

fn load_bootstrap(source: &impl RangeSource) -> ExperimentResult<Bootstrap> {
    Ok(format_bootstrap(source)?)
}

fn directory_page_offset(manifest: &ReferenceManifest, bucket_index: u64) -> io::Result<u64> {
    format_directory_page_offset(manifest, bucket_index)
}

#[cfg(test)]
type Header = pangenome_range_format::ArchiveHeader;

fn encode_header(root_len: u64, entry_count: u64, data_offset: u64) -> [u8; HEADER_LEN] {
    format_encode_header(root_len, entry_count, data_offset)
}

#[cfg(test)]
fn decode_header(bytes: &[u8]) -> io::Result<Header> {
    pangenome_range_format::decode_header(bytes)
}

fn encode_root_index(manifests: &[ReferenceManifest]) -> ExperimentResult<Vec<u8>> {
    Ok(format_encode_root_index(manifests)?)
}

#[cfg(test)]
fn decode_root_index(
    bytes: &[u8],
    header: Header,
) -> ExperimentResult<pangenome_range_format::RootIndex> {
    Ok(pangenome_range_format::decode_root_index(bytes, header)?)
}

fn encode_directory_page(
    entries: &[ArchiveEntry],
    bucket_start: u64,
) -> ExperimentResult<[u8; DIRECTORY_PAGE_BYTES]> {
    Ok(format_encode_directory_page(entries, bucket_start)?)
}

fn decode_directory_page(
    bytes: &[u8],
    manifest: &ReferenceManifest,
    bucket_index: u64,
) -> ExperimentResult<ArchiveIndex> {
    Ok(ArchiveIndex {
        entries: format_decode_directory_page(bytes, manifest, bucket_index)?,
    })
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
    format_compress(codec, bytes)
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
    format_decompress(codec, bytes, expected_len)
}

/// Decompresses and structurally validates every physical payload in an archive.
///
/// # Errors
///
/// Returns an error for malformed metadata, invalid ranges, decompression
/// failure, corrupt payload bytes, or file I/O failure.
pub fn validate_fixed_archive(path: &Path) -> ExperimentResult<ArchiveValidationSummary> {
    validate_fixed_archive_with_progress(path, BuildProgressMode::Off, DEFAULT_PROGRESS_INTERVAL_MS)
}

/// Decompresses and structurally validates every physical payload while
/// periodically reporting validation progress.
///
/// # Errors
///
/// Returns an error for malformed metadata, invalid ranges, decompression
/// failure, corrupt payload bytes, or file I/O failure.
pub fn validate_fixed_archive_with_progress(
    path: &Path,
    progress: BuildProgressMode,
    progress_interval_ms: u64,
) -> ExperimentResult<ArchiveValidationSummary> {
    Ok(format_validate_archive_with_progress(
        path,
        progress_interval_ms,
        |snapshot| {
            emit_validation_progress(progress, snapshot);
        },
    )?)
}

/// Validates an archive with explicit mode, worker, and memory bounds while
/// preserving the build/CLI progress renderer.
///
/// # Errors
///
/// Returns an error for malformed data, invalid limits, or worker failure.
pub fn validate_fixed_archive_with_options(
    path: &Path,
    progress: BuildProgressMode,
    options: ValidationOptions,
) -> ExperimentResult<ArchiveValidationSummary> {
    Ok(format_validate_archive_with_options(
        path,
        options,
        |snapshot| emit_validation_progress(progress, snapshot),
    )?)
}

fn encode_canonical(graph: &CanonicalSubgraph) -> ExperimentResult<Vec<u8>> {
    let normalized = graph.normalized();
    let mut output = Vec::new();
    output.extend_from_slice(b"PNGCAN01");
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
    extension_directory: Option<Vec<u8>>,
    extension_payload: Option<Vec<u8>>,
}

fn conformance_record_payload() -> RecordRegionalPayload {
    RecordRegionalPayload {
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
        edges: BTreeSet::from([PackedEdge { from: 2, to: 4 }]),
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

#[allow(clippy::too_many_lines)]
fn conformance_archive(
    raw: &[u8],
    extension_payload: Option<&[u8]>,
) -> ExperimentResult<ConformanceArchiveParts> {
    let codec = ChunkCodec::Zstd3;
    let compressed = compress(codec, raw)?;
    let extension_directory_len = if extension_payload.is_some() {
        pangenome_range_format::EXTENSION_DIRECTORY_HEADER_BYTES
            + pangenome_range_format::EXTENSION_ENTRY_BYTES
    } else {
        0
    };
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
    manifest.first_page_offset =
        usize_to_u64(HEADER_LEN + provisional_root.len() + extension_directory_len)?;
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
        integrity: blake3::hash(&compressed).as_bytes()[..16]
            .try_into()
            .expect("fixed digest"),
        codec,
    };
    let directory = encode_directory_page(&[entry], 0)?.to_vec();
    let (extension_directory, extension_payload) =
        if let Some(extension_payload) = extension_payload {
            let extension_offset = data_offset
                .checked_add(usize_to_u64(compressed.len())?)
                .ok_or_else(|| invalid_data("conformance extension offset overflow"))?;
            let digest = blake3::hash(extension_payload);
            let extension_entry = ExtensionEntry {
                type_id: *b"provenance-v1---",
                required: false,
                codec: ChunkCodec::None,
                offset: extension_offset,
                encoded_len: usize_to_u64(extension_payload.len())?,
                decoded_len: usize_to_u64(extension_payload.len())?,
                integrity: digest.as_bytes()[..16].try_into().expect("fixed digest"),
            };
            (
                Some(format_encode_extension_directory(&[extension_entry])?),
                Some(extension_payload.to_vec()),
            )
        } else {
            (None, None)
        };
    let extension_directory_offset = extension_directory
        .as_ref()
        .map_or(0, |_| usize_to_u64(HEADER_LEN + root.len()).unwrap());
    let extension_directory_length = extension_directory
        .as_ref()
        .map_or(0, |bytes| usize_to_u64(bytes.len()).unwrap());
    let header = if extension_directory.is_some() {
        format_encode_header_with_extensions(
            usize_to_u64(root.len())?,
            1,
            data_offset,
            extension_directory_offset,
            extension_directory_length,
        )
        .to_vec()
    } else {
        encode_header(usize_to_u64(root.len())?, 1, data_offset).to_vec()
    };
    let mut archive = Vec::with_capacity(
        header.len()
            + root.len()
            + extension_directory_len
            + directory.len()
            + compressed.len()
            + extension_payload.as_ref().map_or(0, Vec::len),
    );
    archive.extend_from_slice(&header);
    archive.extend_from_slice(&root);
    if let Some(bytes) = &extension_directory {
        archive.extend_from_slice(bytes);
    }
    archive.extend_from_slice(&directory);
    archive.extend_from_slice(&compressed);
    if let Some(bytes) = &extension_payload {
        archive.extend_from_slice(bytes);
    }
    Ok(ConformanceArchiveParts {
        archive,
        header,
        root,
        directory,
        compressed,
        extension_directory,
        extension_payload,
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
    let reference_traversal = graph
        .reference_paths
        .first()
        .ok_or_else(|| invalid_data("conformance graph has no reference traversal"))?
        .traversal
        .iter()
        .map(|&node| pack_oriented(node).map(|value| value.to_string()))
        .collect::<io::Result<Vec<_>>>()?;
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
        "graphHash": canonical.canonical_hash().to_hex().to_string(),
        "tileLocalHaplotypeHash": graph.haplotype_tiles.first()
            .ok_or_else(|| invalid_data("conformance graph has no haplotype tile"))?
            .canonical_hash().to_hex().to_string(),
        "tile": {
            "semantics": graph.semantics.label(),
            "coreStart": 100,
            "coreEnd": 102,
            "nodeIds": graph.nodes.keys().map(ToString::to_string).collect::<Vec<_>>(),
            "nodeSequences": graph.nodes.values().map(|value| String::from_utf8_lossy(value)).collect::<Vec<_>>(),
            "edges": graph.edges.iter().flat_map(|edge| [pack_oriented(edge.from), pack_oriented(edge.to)]).collect::<io::Result<Vec<_>>>()?.into_iter().map(|value| value.to_string()).collect::<Vec<_>>(),
            "referenceTraversal": reference_traversal,
            "weightedTraversals": traversals
        }
    }))
}

fn write_conformance_fixture(
    directory: &Path,
    id: &str,
    graph: &RegionalGraph,
    raw: &[u8],
    query: &QuerySpec,
    extension_payload: Option<&[u8]>,
) -> ExperimentResult<serde_json::Value> {
    let parts = conformance_archive(raw, extension_payload)?;
    let root_offset = usize_to_u64(parts.header.len())?;
    let root_end = root_offset
        .checked_add(usize_to_u64(parts.root.len())?)
        .ok_or_else(|| invalid_data("conformance root offset overflow"))?;
    let directory_offset = root_end
        .checked_add(usize_to_u64(
            parts.extension_directory.as_ref().map_or(0, Vec::len),
        )?)
        .ok_or_else(|| invalid_data("conformance directory offset overflow"))?;
    let payload_offset = directory_offset
        .checked_add(usize_to_u64(parts.directory.len())?)
        .ok_or_else(|| invalid_data("conformance payload offset overflow"))?;
    let header_len = parts.header.len();
    let root_len = parts.root.len();
    let directory_len = parts.directory.len();
    let compressed_len = parts.compressed.len();
    let mut files = vec![
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
    if let Some(bytes) = parts.extension_directory.clone() {
        files.push((format!("{id}.extensions.bin"), bytes));
    }
    if let Some(bytes) = parts.extension_payload.clone() {
        files.push((format!("{id}.extension-provenance.json"), bytes));
    }
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
        "archiveVersion": ARCHIVE_VERSION,
        "regionalVersion": REGION_VERSION,
        "semantics": graph.semantics.label(),
        "files": file_metadata,
        "sections": {
            "archiveHeader": {"offset": 0, "length": header_len},
            "rootIndex": {"offset": root_offset, "length": root_len},
            "directoryPages": {"offset": directory_offset, "length": directory_len, "pageCount": 1},
            "regionalPayload": {
                "offset": payload_offset,
                "encodedLength": compressed_len,
                "decodedLength": raw.len(),
                "codec": "zstd-3"
            },
            "extensionDirectory": parts.extension_directory.as_ref().map(|bytes| serde_json::json!({
                "offset": root_end,
                "length": bytes.len(),
                "entryCount": 1
            })),
            "extensionPayload": parts.extension_payload.as_ref().map(|bytes| serde_json::json!({
                "offset": payload_offset + compressed_len as u64,
                "encodedLength": bytes.len(),
                "decodedLength": bytes.len(),
                "codec": "none",
                "required": false
            }))
        },
        "directoryEntries": [{
            "bucketStart": 0,
            "coreStart": 100,
            "coreEnd": 102,
            "payloadOffset": payload_offset,
            "encodedLength": compressed_len,
            "decodedLength": raw.len()
        }],
        "expected": conformance_expected(graph, query)?
    }))
}

fn write_conformance_failure(
    directory: &Path,
    id: &str,
    extension: &str,
    input_kind: &str,
    rejection_stage: &str,
    bytes: &[u8],
) -> ExperimentResult<serde_json::Value> {
    let name = format!("{id}.{extension}");
    std::fs::write(directory.join(&name), bytes)?;
    Ok(serde_json::json!({
        "id": id,
        "file": name,
        "inputKind": input_kind,
        "expected": "reject",
        "rejectionStage": rejection_stage,
        "bytes": bytes.len(),
        "sha256": sha256_hex(bytes)
    }))
}

#[allow(clippy::too_many_lines)]
fn write_conformance_failures(
    directory: &Path,
    archive: &[u8],
    raw: &[u8],
) -> ExperimentResult<Vec<serde_json::Value>> {
    let mut failures = Vec::new();
    failures.push(write_conformance_failure(
        directory,
        "corrupt-archive-truncated-header",
        "pngr",
        "archive",
        "archive-open",
        &archive[..HEADER_LEN - 1],
    )?);

    let mut bad_magic = archive.to_vec();
    bad_magic[0] = 0;
    failures.push(write_conformance_failure(
        directory,
        "corrupt-archive-bad-magic",
        "pngr",
        "archive",
        "archive-open",
        &bad_magic,
    )?);

    let mut unsupported_version = archive.to_vec();
    unsupported_version[8..12].copy_from_slice(&99_u32.to_le_bytes());
    failures.push(write_conformance_failure(
        directory,
        "corrupt-archive-unsupported-version",
        "pngr",
        "archive",
        "archive-open",
        &unsupported_version,
    )?);

    let mut nonzero_reserved = archive.to_vec();
    nonzero_reserved[48] = 1;
    failures.push(write_conformance_failure(
        directory,
        "corrupt-archive-header-reserved",
        "pngr",
        "archive",
        "archive-open",
        &nonzero_reserved,
    )?);

    let mut root_length_overflow = archive.to_vec();
    root_length_overflow[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
    failures.push(write_conformance_failure(
        directory,
        "corrupt-archive-root-length-overflow",
        "pngr",
        "archive",
        "archive-open",
        &root_length_overflow,
    )?);

    let mut invalid_utf8 = archive.to_vec();
    invalid_utf8[80] = 0xff;
    failures.push(write_conformance_failure(
        directory,
        "corrupt-root-invalid-utf8",
        "pngr",
        "archive",
        "root-decode",
        &invalid_utf8,
    )?);

    let mut unknown_codec = archive.to_vec();
    unknown_codec[64 + 98] = 99;
    failures.push(write_conformance_failure(
        directory,
        "corrupt-root-unknown-codec",
        "pngr",
        "archive",
        "root-decode",
        &unknown_codec,
    )?);

    let mut payload_out_of_file = archive.to_vec();
    let directory_offset = HEADER_LEN + 106;
    payload_out_of_file[directory_offset + 32..directory_offset + 40]
        .copy_from_slice(&(u64::MAX - 15).to_le_bytes());
    failures.push(write_conformance_failure(
        directory,
        "corrupt-directory-payload-out-of-file",
        "pngr",
        "archive",
        "directory-decode",
        &payload_out_of_file,
    )?);

    let mut corrupt_payload = archive.to_vec();
    *corrupt_payload
        .last_mut()
        .ok_or_else(|| invalid_data("conformance archive is empty"))? ^= 0xff;
    failures.push(write_conformance_failure(
        directory,
        "corrupt-payload-zstd",
        "pngr",
        "archive",
        "payload-decompression",
        &corrupt_payload,
    )?);

    failures.push(write_conformance_failure(
        directory,
        "corrupt-regional-truncated",
        "bin",
        "regional-payload",
        "regional-decode",
        &raw[..raw.len() - 1],
    )?);

    let mut huge_node_count = raw.to_vec();
    huge_node_count[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
    failures.push(write_conformance_failure(
        directory,
        "corrupt-regional-huge-node-count",
        "bin",
        "regional-payload",
        "regional-decode",
        &huge_node_count,
    )?);

    let mut malformed_varint = raw.to_vec();
    malformed_varint[220..224].fill(0x80);
    failures.push(write_conformance_failure(
        directory,
        "corrupt-regional-malformed-varint",
        "bin",
        "regional-payload",
        "packed-record-decode",
        &malformed_varint,
    )?);

    let mut nonminimal_varint = raw.to_vec();
    let record_length_offset = nonminimal_varint
        .len()
        .checked_sub(12)
        .ok_or_else(|| invalid_data("conformance payload is too short"))?;
    nonminimal_varint[record_length_offset..record_length_offset + 8]
        .copy_from_slice(&5_u64.to_le_bytes());
    *nonminimal_varint
        .last_mut()
        .ok_or_else(|| invalid_data("conformance payload is empty"))? |= 0x80;
    nonminimal_varint.push(0);
    failures.push(write_conformance_failure(
        directory,
        "corrupt-regional-nonminimal-varint",
        "bin",
        "regional-payload",
        "packed-record-decode",
        &nonminimal_varint,
    )?);
    Ok(failures)
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
    let record_payload = conformance_record_payload();
    let record = record_payload.clone().into_regional_graph()?;
    let fixtures = vec![
        write_conformance_fixture(
            directory,
            "format-v1",
            &record,
            &record_payload.encode()?,
            &query,
            None,
        )?,
        write_conformance_fixture(
            directory,
            "format-v1-optional-extension",
            &record,
            &record_payload.encode()?,
            &query,
            Some(b"{ \"title\": \"Synthetic conformance archive\" }\n"),
        )?,
    ];
    let archive = std::fs::read(directory.join("format-v1.pngr"))?;
    let raw = std::fs::read(directory.join("format-v1.payload.raw"))?;
    let failures = write_conformance_failures(directory, &archive, &raw)?;
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "provenance": "deterministic synthetic two-node graph generated by the Rust reference encoder; no external source data",
        "format": {
            "archiveMagic": "PNGRNG01",
            "archiveVersion": ARCHIVE_VERSION,
            "regionalMagic": "PNGRGN01",
            "regionalVersion": REGION_VERSION,
            "headerBytes": HEADER_LEN,
            "directoryPageBytes": DIRECTORY_PAGE_BYTES,
            "directoryEntryBytes": pangenome_range_format::DIRECTORY_ENTRY_BYTES,
            "maximumDirectoryEntriesPerPage": DIRECTORY_ENTRIES_PER_PAGE,
            "maximumRootBytes": pangenome_range_format::MAX_ROOT_BYTES,
            "maximumDecodedOccurrencesPerTile": MAX_DECODED_OCCURRENCES_PER_TILE
        },
        "supportedArchiveVersions": [1],
        "supportedRegionalVersions": [1],
        "fixtures": fixtures,
        "expectedFailures": failures
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
        RecordRegionalPayload {
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
            edges: BTreeSet::from([PackedEdge { from: 2, to: 4 }]),
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
    fn record_regional_v1_golden_reconstructs_weighted_paths() {
        let encoded = record_regional_golden().encode().unwrap();
        let fixture = decode_hex(include_str!(
            "../../../test-data/golden/record-region-v1.hex"
        ));
        assert_eq!(encoded, fixture);
        assert_eq!(
            blake3::hash(&encoded).to_hex().as_str(),
            "3cb0198e12e5cbaccb81131b9d93094ecfe9b03fc6dd13cc88c44210b1a282e7"
        );
        let compressed = decode_hex(include_str!(
            "../../../test-data/golden/record-region-v1.zstd3.hex"
        ));
        assert_eq!(
            zstd::bulk::decompress(&compressed, encoded.len()).unwrap(),
            encoded
        );
        assert_eq!(&encoded[..8], pangenome_range_format::REGION_MAGIC);
        let decoded = RecordRegionalPayload::decode(&encoded)
            .unwrap()
            .into_regional_graph()
            .unwrap();
        assert_eq!(
            decoded.haplotype_tiles[0]
                .canonical_hash()
                .to_hex()
                .as_str(),
            "283a80ba58d2841b5dc39b91b6623698f84aeec100938793db7bb39f65f6aabb"
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
    fn record_regional_v1_rejects_corrupt_counts_runs_and_reference_offsets() {
        let encoded = record_regional_golden().encode().unwrap();
        assert!(RecordRegionalPayload::decode(&encoded[..encoded.len() - 1]).is_err());

        let mut invalid_magic = encoded.clone();
        invalid_magic[0] = 0;
        assert!(RecordRegionalPayload::decode(&invalid_magic).is_err());

        let mut invalid_version = encoded.clone();
        invalid_version[8..12].copy_from_slice(&3_u32.to_le_bytes());
        assert!(RecordRegionalPayload::decode(&invalid_version).is_err());

        let mut invalid_semantics = encoded.clone();
        invalid_semantics[16] = 1;
        assert!(RecordRegionalPayload::decode(&invalid_semantics).is_err());

        let mut invalid_reserved = encoded.clone();
        invalid_reserved[17] = 1;
        assert!(RecordRegionalPayload::decode(&invalid_reserved).is_err());

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
    fn record_archive_v1_golden_is_deterministic_and_matches_source_oracle() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/micb-kir3dl1.gbz");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-data/golden/record-archive-v1.pngr");
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
        assert_eq!(validation.archive_version, 1);
        assert_eq!(validation.reference_manifests, 1);
        assert_eq!(validation.directory_entries, 1);
        assert_eq!(validation.physical_payloads, 1);
        let mut validation_progress = Vec::new();
        let progress_validation = format_validate_archive_with_progress(&fixture, 0, |snapshot| {
            validation_progress.push(snapshot.clone());
        })
        .unwrap();
        assert_eq!(progress_validation.directory_entries, 1);
        let final_progress = validation_progress.last().unwrap();
        assert_eq!(final_progress.phase, "archive_validation_progress");
        assert!((final_progress.percent_complete - 100.0).abs() < f64::EPSILON);
        assert_eq!(final_progress.directory_pages_validated, 1);
        assert_eq!(final_progress.directory_pages_total, 1);
        assert_eq!(final_progress.directory_entries_validated, 1);
        assert_eq!(final_progress.directory_entries_total, 1);
        assert_eq!(final_progress.physical_payloads_validated, 1);

        let archive_bytes = std::fs::read(&fixture).unwrap();
        let mut invalid_magic = archive_bytes.clone();
        invalid_magic[0] = 0;
        assert!(decode_header(&invalid_magic[..HEADER_LEN]).is_err());
        let mut invalid_version = archive_bytes;
        invalid_version[8..12].copy_from_slice(&3_u32.to_le_bytes());
        assert!(decode_header(&invalid_version[..HEADER_LEN]).is_err());

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
        assert_eq!(document["schemaVersion"], 2);
        assert_eq!(document["format"]["archiveMagic"], "PNGRNG01");
        assert_eq!(document["format"]["regionalMagic"], "PNGRGN01");
        assert_eq!(document["format"]["headerBytes"], HEADER_LEN);
        assert_eq!(
            document["format"]["maximumDirectoryEntriesPerPage"],
            DIRECTORY_ENTRIES_PER_PAGE
        );
        assert_eq!(document["supportedArchiveVersions"], serde_json::json!([1]));
        assert_eq!(
            document["supportedRegionalVersions"],
            serde_json::json!([1])
        );
        for fixture in document["fixtures"].as_array().unwrap() {
            for (name, metadata) in fixture["files"].as_object().unwrap() {
                let bytes = std::fs::read(directory.join(name)).unwrap();
                assert_eq!(metadata["bytes"], bytes.len());
                assert_eq!(metadata["sha256"], sha256_hex(&bytes));
            }
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

            let raw = std::fs::read(
                directory.join(format!("{}.payload.raw", fixture["id"].as_str().unwrap())),
            )
            .unwrap();
            let graph = RecordRegionalPayload::decode(&raw)
                .unwrap()
                .into_regional_graph()
                .unwrap();
            let expected = &fixture["expected"];
            let query = &expected["query"];
            let query = QuerySpec {
                id: "manifest-conformance".into(),
                class: "cross-language-conformance".into(),
                sample: query["sample"].as_str().unwrap().into(),
                contig: query["contig"].as_str().unwrap().into(),
                start: query["start"].as_u64().unwrap(),
                end: query["end"].as_u64().unwrap(),
                context: query["context"].as_u64().unwrap(),
            };
            let selected = graph.select_nodes(&query).unwrap();
            let canonical = graph.canonical(&selected, &query).unwrap();
            assert_eq!(
                expected["graphHash"],
                canonical.canonical_hash().to_hex().to_string()
            );
            assert_eq!(
                expected["tileLocalHaplotypeHash"],
                graph.haplotype_tiles[0]
                    .canonical_hash()
                    .to_hex()
                    .to_string()
            );
        }
        for failure in document["expectedFailures"].as_array().unwrap() {
            let bytes = std::fs::read(directory.join(failure["file"].as_str().unwrap())).unwrap();
            assert_eq!(failure["bytes"], bytes.len());
            assert_eq!(failure["sha256"], sha256_hex(&bytes));
            match failure["inputKind"].as_str().unwrap() {
                "archive" => {
                    let path = directory.join(failure["file"].as_str().unwrap());
                    assert!(
                        validate_fixed_archive(&path).is_err(),
                        "{} unexpectedly validated",
                        failure["id"]
                    );
                }
                "regional-payload" => assert!(RecordRegionalPayload::decode(&bytes).is_err()),
                other => panic!("unsupported conformance failure input kind {other}"),
            }
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
                direct, oracle,
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
                integrity: [0; 16],
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
            extension_directory_offset: 0,
            extension_directory_len: 0,
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
        let path = simple_sds::serialize::temp_file_name("pangenome-range-v1-cache");
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
                integrity: [0; 16],
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
