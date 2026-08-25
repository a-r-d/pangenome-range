use gbz::support;
use gbz::{FullPathName, GBZ, Orientation};
use gbz_base::{HaplotypeOutput, PathIndex, Subgraph, SubgraphQuery};
use pangenome_range_format::{FileRangeSource, NetworkProfile, RangeSource, TracingRangeSource};
use pangenome_range_query::{
    CanonicalPath, CanonicalSubgraph, Edge, OrientedNode, ReferenceInterval,
};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::error::Error;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::ops::Range;
use std::path::Path;
use std::time::Instant;

pub type ExperimentResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const ARCHIVE_MAGIC: &[u8; 8] = b"PNGRNG02";
const REGION_MAGIC: &[u8; 8] = b"PNGRGN01";
const ARCHIVE_VERSION: u32 = 2;
const REGION_VERSION: u32 = 1;
const HEADER_LEN: usize = 64;
const DIRECTORY_PAGE_TARGET_BYTES: usize = 4 * 1024;
pub const BOOTSTRAP_LEN: usize = 16 * 1024;
pub const CONSTRUCTION_CONTEXT: u64 = 100;

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
    pub construction_wall_ms: f64,
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
struct ArchiveEntry {
    sample: String,
    contig: String,
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
struct DirectoryPageRef {
    sample: String,
    contig: String,
    start: u64,
    end: u64,
    offset: u64,
    length: u64,
    entry_count: u64,
}

#[derive(Clone, Debug)]
struct RootIndex {
    logical_bytes: u64,
    pages: Vec<DirectoryPageRef>,
}

#[derive(Clone, Debug)]
struct DirectoryLookup {
    entries: Vec<ArchiveEntry>,
    logical_bytes: u64,
    fetched_bytes: u64,
    fetched_ranges: u64,
    selected_pages: u64,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReferenceVisit {
    path_id: u64,
    visit_index: u64,
    start: u64,
    end: u64,
    node: OrientedNode,
}

#[derive(Clone, Debug, Default)]
struct RegionalGraph {
    nodes: BTreeMap<u64, Vec<u8>>,
    edges: BTreeSet<Edge>,
    paths: BTreeMap<u64, RegionalPath>,
    reference_visits: BTreeSet<ReferenceVisit>,
}

#[derive(Clone, Debug)]
struct ChunkBlob {
    compressed: Vec<u8>,
    uncompressed_len: u64,
    offset: u64,
}

#[derive(Clone, Debug)]
struct PendingEntry {
    sample: String,
    contig: String,
    start: u64,
    end: u64,
    chunk_id: usize,
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

#[derive(Clone, Debug)]
pub struct ReferencePathSpec {
    pub name: FullPathName,
    pub start: u64,
    pub end: u64,
}

pub fn reference_paths(graph: &GBZ) -> ExperimentResult<Vec<ReferencePathSpec>> {
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

#[allow(clippy::too_many_lines)]
pub fn build_fixed_archive(
    graph: &GBZ,
    path_index: &PathIndex,
    source_gbz_bytes: u64,
    output: &Path,
    config: &FixedArchiveConfig,
) -> ExperimentResult<ArchiveBuildMetrics> {
    if config.window_size == 0 {
        return Err(invalid_input("window size must be greater than zero").into());
    }
    let started = Instant::now();
    let references = reference_paths(graph)?;
    let mut chunks: Vec<ChunkBlob> = Vec::new();
    let mut pending_entries: Vec<PendingEntry> = Vec::new();
    let mut chunk_by_hash: HashMap<blake3::Hash, Vec<usize>> = HashMap::new();
    let mut raw_chunks: Vec<Vec<u8>> = Vec::new();
    let mut duplicate_payload_entries_observed = 0_u64;
    let mut avoidable_compressed_payload_bytes = 0_u64;

    for reference in &references {
        let first_boundary = (reference.start / config.window_size) * config.window_size;
        let mut boundary = first_boundary;
        while boundary < reference.end {
            let boundary_end = boundary
                .checked_add(config.window_size)
                .ok_or_else(|| invalid_data("window boundary overflow"))?;
            let start = boundary.max(reference.start);
            let end = boundary_end.min(reference.end);
            if start < end {
                let query_name = FullPathName {
                    sample: reference.name.sample.clone(),
                    contig: reference.name.contig.clone(),
                    haplotype: reference.name.haplotype,
                    fragment: 0,
                };
                let query = SubgraphQuery::path_interval(
                    &query_name,
                    u64_to_usize(start)?..u64_to_usize(end)?,
                )
                .with_context(u64_to_usize(CONSTRUCTION_CONTEXT)?)
                .with_haplotypes(HaplotypeOutput::None);
                let mut subgraph = Subgraph::new();
                subgraph.from_gbz(graph, Some(path_index), None, &query)?;
                let selected: BTreeSet<_> = subgraph.node_iter().collect();
                let regional = RegionalGraph::from_gbz(graph, &selected)?;
                let raw = regional.encode()?;
                let hash = blake3::hash(&raw);

                let identical = chunk_by_hash.get(&hash).and_then(|candidates| {
                    candidates
                        .iter()
                        .copied()
                        .find(|&candidate| raw_chunks[candidate] == raw)
                });
                if let Some(existing) = identical {
                    duplicate_payload_entries_observed += 1;
                    avoidable_compressed_payload_bytes = avoidable_compressed_payload_bytes
                        .checked_add(usize_to_u64(chunks[existing].compressed.len())?)
                        .ok_or_else(|| invalid_data("avoidable payload byte count overflow"))?;
                }
                let existing = config.deduplicate_chunks.then_some(identical).flatten();
                let chunk_id = if let Some(existing) = existing {
                    existing
                } else {
                    let compressed = compress(config.codec, &raw)?;
                    let chunk_id = chunks.len();
                    chunks.push(ChunkBlob {
                        compressed,
                        uncompressed_len: usize_to_u64(raw.len())?,
                        offset: 0,
                    });
                    raw_chunks.push(raw);
                    chunk_by_hash.entry(hash).or_default().push(chunk_id);
                    chunk_id
                };
                pending_entries.push(PendingEntry {
                    sample: reference.name.sample.clone(),
                    contig: reference.name.contig.clone(),
                    start,
                    end,
                    chunk_id,
                });
            }
            boundary = boundary_end;
        }
    }

    pending_entries.sort_by(|left, right| {
        (&left.sample, &left.contig, left.start, left.end).cmp(&(
            &right.sample,
            &right.contig,
            right.start,
            right.end,
        ))
    });
    let provisional_entries = archive_entries(&pending_entries, &chunks, config.codec)?;
    let page_ranges = partition_directory_pages(&provisional_entries)?;
    let provisional_page_payloads = page_ranges
        .iter()
        .map(|range| encode_index(&provisional_entries[range.clone()]))
        .collect::<ExperimentResult<Vec<_>>>()?;
    let mut directory_pages = Vec::with_capacity(page_ranges.len());
    for (range, payload) in page_ranges.iter().zip(&provisional_page_payloads) {
        directory_pages.push(DirectoryPageRef {
            sample: provisional_entries[range.start].sample.clone(),
            contig: provisional_entries[range.start].contig.clone(),
            start: provisional_entries[range.start].start,
            end: provisional_entries[range.end - 1].end,
            offset: 0,
            length: usize_to_u64(payload.len())?,
            entry_count: usize_to_u64(range.len())?,
        });
    }
    let provisional_root = encode_root_index(&directory_pages)?;
    let mut next_page_offset = usize_to_u64(HEADER_LEN)?
        .checked_add(usize_to_u64(provisional_root.len())?)
        .ok_or_else(|| invalid_data("directory page offset overflow"))?;
    for page in &mut directory_pages {
        page.offset = next_page_offset;
        next_page_offset = next_page_offset
            .checked_add(page.length)
            .ok_or_else(|| invalid_data("archive data offset overflow"))?;
    }
    let data_offset = next_page_offset;
    let mut next_offset = data_offset;
    for chunk in &mut chunks {
        chunk.offset = next_offset;
        next_offset = next_offset
            .checked_add(usize_to_u64(chunk.compressed.len())?)
            .ok_or_else(|| invalid_data("archive size overflow"))?;
    }
    let entries = archive_entries(&pending_entries, &chunks, config.codec)?;
    let page_payloads = page_ranges
        .iter()
        .map(|range| encode_index(&entries[range.clone()]))
        .collect::<ExperimentResult<Vec<_>>>()?;
    for (page, payload) in directory_pages.iter().zip(&page_payloads) {
        if page.length != usize_to_u64(payload.len())? {
            return Err(invalid_data("directory page size changed after assigning offsets").into());
        }
    }
    let root = encode_root_index(&directory_pages)?;
    if root.len() != provisional_root.len() {
        return Err(invalid_data("root index size changed after assigning offsets").into());
    }
    let header = encode_header(
        usize_to_u64(root.len())?,
        usize_to_u64(entries.len())?,
        data_offset,
    );
    let mut writer = BufWriter::new(File::create(output)?);
    writer.write_all(&header)?;
    writer.write_all(&root)?;
    for page in &page_payloads {
        writer.write_all(page)?;
    }
    for chunk in &chunks {
        writer.write_all(&chunk.compressed)?;
    }
    writer.flush()?;

    let archive_bytes = std::fs::metadata(output)?.len();
    let mut chunk_sizes = chunks
        .iter()
        .map(|chunk| usize_to_u64(chunk.compressed.len()))
        .collect::<Result<Vec<_>, _>>()?;
    chunk_sizes.sort_unstable();
    let index_bytes = data_offset;
    let root_index_bytes = usize_to_u64(HEADER_LEN + root.len())?;
    let directory_entries = usize_to_u64(entries.len())?;
    let physical_chunks = usize_to_u64(chunks.len())?;
    Ok(ArchiveBuildMetrics {
        experiment_id: config.experiment_id.clone(),
        source_gbz_bytes,
        archive_bytes,
        expansion_ratio: ratio(archive_bytes, source_gbz_bytes),
        index_bytes,
        index_ratio: ratio(index_bytes, archive_bytes),
        root_index_bytes,
        directory_pages: usize_to_u64(directory_pages.len())?,
        directory_entries,
        physical_chunks,
        deduplicated_entries: directory_entries.saturating_sub(physical_chunks),
        duplicate_payload_entries_observed,
        avoidable_compressed_payload_bytes,
        mean_chunk_bytes: mean(&chunk_sizes),
        median_chunk_bytes: percentile_u64(&chunk_sizes, 0.5),
        p95_chunk_bytes: percentile_u64(&chunk_sizes, 0.95),
        max_chunk_bytes: chunk_sizes.last().copied().unwrap_or(0),
        construction_wall_ms: started.elapsed().as_secs_f64() * 1_000.0,
    })
}

pub fn source_oracle(
    graph: &GBZ,
    path_index: &PathIndex,
    query: &QuerySpec,
) -> ExperimentResult<OracleResult> {
    query.validate()?;
    let query_name = FullPathName::reference(&query.sample, &query.contig);
    let upstream_query = SubgraphQuery::path_interval(
        &query_name,
        u64_to_usize(query.start)?..u64_to_usize(query.end)?,
    )
    .with_context(u64_to_usize(query.context)?)
    .with_haplotypes(HaplotypeOutput::None);
    let mut subgraph = Subgraph::new();
    subgraph.from_gbz(graph, Some(path_index), None, &upstream_query)?;
    let selected: BTreeSet<_> = subgraph.node_iter().collect();
    let regional = RegionalGraph::from_gbz(graph, &selected)?;
    let canonical_ids = selected
        .iter()
        .copied()
        .map(usize_to_u64)
        .collect::<Result<BTreeSet<_>, _>>()?;
    let canonical = regional.canonical(&canonical_ids, query)?;
    let encoded = encode_canonical(&canonical)?;
    Ok(OracleResult { canonical, encoded })
}

#[allow(clippy::too_many_lines)]
pub fn query_fixed_archive(
    archive: &Path,
    config: &FixedArchiveConfig,
    query: &QuerySpec,
    coalescing_gap: u64,
    oracle: &OracleResult,
) -> ExperimentResult<QueryMeasurement> {
    query.validate()?;
    let total_started = Instant::now();
    let source = TracingRangeSource::new(FileRangeSource::open(archive)?);
    let bootstrap = load_bootstrap(&source)?;

    let lookup_started = Instant::now();
    let DirectoryLookup {
        entries,
        logical_bytes: logical_index_bytes,
        fetched_bytes: directory_page_bytes_fetched,
        fetched_ranges: directory_page_ranges_fetched,
        selected_pages: directory_pages_selected,
    } = lookup_directory(&source, &bootstrap, query)?;
    let index_lookup_us = lookup_started.elapsed().as_secs_f64() * 1_000_000.0;

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
    let dependency_rounds =
        bootstrap.dependency_rounds + logical_directory_round + logical_data_round;
    let required_compressed_payload_bytes = usize_to_u64(required_compressed.len())?;
    let canonical_payload_bytes = usize_to_u64(oracle.encoded.len())?;
    let mut dependency_groups = vec![1_u64; u64_to_usize(bootstrap.dependency_rounds)?];
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
        bootstrap_bytes_fetched: usize_to_u64(bootstrap.bytes.len())?,
        logical_index_bytes,
        directory_page_bytes_fetched,
        directory_pages_selected,
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
        simulated_20ms_ms,
        simulated_50ms_ms,
        simulated_100ms_ms,
    })
}

impl RegionalGraph {
    fn from_gbz(graph: &GBZ, selected: &BTreeSet<usize>) -> ExperimentResult<Self> {
        let metadata = graph
            .metadata()
            .ok_or_else(|| invalid_data("GBZ metadata is required"))?;
        let reference_ids: BTreeSet<_> = graph.reference_sample_ids(true).into_iter().collect();
        let mut result = Self::default();
        for &node_id in selected {
            let sequence = graph
                .sequence(node_id)
                .ok_or_else(|| invalid_data(format!("missing sequence for node {node_id}")))?;
            result
                .nodes
                .insert(usize_to_u64(node_id)?, sequence.to_vec());
            for orientation in [Orientation::Forward, Orientation::Reverse] {
                let successors = graph
                    .successors(node_id, orientation)
                    .ok_or_else(|| invalid_data(format!("missing node {node_id}")))?;
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

        for path_id in 0..graph.paths() {
            let path_name = FullPathName::from_metadata(metadata, path_id)
                .ok_or_else(|| invalid_data(format!("missing metadata for path {path_id}")))?;
            let metadata_name = metadata
                .path(path_id)
                .ok_or_else(|| invalid_data(format!("missing path name {path_id}")))?;
            let is_reference = reference_ids.contains(&metadata_name.sample());
            let mut coordinate = usize_to_u64(path_name.fragment)?;
            let mut regional_path = RegionalPath {
                path_id: usize_to_u64(path_id)?,
                sample: path_name.sample,
                contig: path_name.contig,
                haplotype: usize_to_u64(path_name.haplotype)?,
                fragment: usize_to_u64(path_name.fragment)?,
                is_reference,
                visits: BTreeMap::new(),
            };
            let path = graph
                .path(path_id, Orientation::Forward)
                .ok_or_else(|| invalid_data(format!("missing path {path_id}")))?;
            for (visit_index, (node_id, orientation)) in path.enumerate() {
                let sequence_len = graph
                    .sequence_len(node_id)
                    .ok_or_else(|| invalid_data(format!("missing sequence for node {node_id}")))?;
                let visit_end = coordinate
                    .checked_add(usize_to_u64(sequence_len)?)
                    .ok_or_else(|| invalid_data("path coordinate overflow"))?;
                if selected.contains(&node_id) {
                    let visit_index = usize_to_u64(visit_index)?;
                    let node = oriented(node_id, orientation)?;
                    regional_path.visits.insert(visit_index, node);
                    if is_reference {
                        result.reference_visits.insert(ReferenceVisit {
                            path_id: regional_path.path_id,
                            visit_index,
                            start: coordinate,
                            end: visit_end,
                            node,
                        });
                    }
                }
                coordinate = visit_end;
            }
            if !regional_path.visits.is_empty() {
                result.paths.insert(regional_path.path_id, regional_path);
            }
        }
        Ok(result)
    }

    fn merge(&mut self, other: Self) -> io::Result<()> {
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

    fn encode(&self) -> ExperimentResult<Vec<u8>> {
        let mut output = Vec::new();
        output.extend_from_slice(REGION_MAGIC);
        put_u32(&mut output, REGION_VERSION);
        put_u32(&mut output, 0);
        put_u64(&mut output, usize_to_u64(self.nodes.len())?);
        put_u64(&mut output, usize_to_u64(self.edges.len())?);
        put_u64(&mut output, usize_to_u64(self.paths.len())?);
        put_u64(&mut output, usize_to_u64(self.reference_visits.len())?);
        for (id, sequence) in &self.nodes {
            put_u64(&mut output, *id);
            put_bytes(&mut output, sequence)?;
        }
        for edge in &self.edges {
            put_oriented(&mut output, edge.from);
            put_oriented(&mut output, edge.to);
        }
        for path in self.paths.values() {
            put_u64(&mut output, path.path_id);
            put_u64(&mut output, path.haplotype);
            put_u64(&mut output, path.fragment);
            output.push(u8::from(path.is_reference));
            put_string(&mut output, &path.sample)?;
            put_string(&mut output, &path.contig)?;
            put_u64(&mut output, usize_to_u64(path.visits.len())?);
            for (&index, &node) in &path.visits {
                put_u64(&mut output, index);
                put_oriented(&mut output, node);
            }
        }
        for visit in &self.reference_visits {
            put_u64(&mut output, visit.path_id);
            put_u64(&mut output, visit.visit_index);
            put_u64(&mut output, visit.start);
            put_u64(&mut output, visit.end);
            put_oriented(&mut output, visit.node);
        }
        Ok(output)
    }

    fn decode(bytes: &[u8]) -> ExperimentResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        if reader.take(8)? != REGION_MAGIC {
            return Err(invalid_data("invalid regional chunk magic").into());
        }
        let version = reader.u32()?;
        if version != REGION_VERSION {
            return Err(invalid_data(format!("unsupported regional version {version}")).into());
        }
        let _flags = reader.u32()?;
        let node_count = reader.u64()?;
        let edge_count = reader.u64()?;
        let path_count = reader.u64()?;
        let reference_count = reader.u64()?;
        let mut result = Self::default();
        for _ in 0..node_count {
            result.nodes.insert(reader.u64()?, reader.bytes()?);
        }
        for _ in 0..edge_count {
            result.edges.insert(Edge {
                from: reader.oriented()?,
                to: reader.oriented()?,
            });
        }
        for _ in 0..path_count {
            let path_id = reader.u64()?;
            let haplotype = reader.u64()?;
            let fragment = reader.u64()?;
            let is_reference = reader.u8()? != 0;
            let sample = reader.string()?;
            let contig = reader.string()?;
            let visit_count = reader.u64()?;
            let mut visits = BTreeMap::new();
            for _ in 0..visit_count {
                visits.insert(reader.u64()?, reader.oriented()?);
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
                node: reader.oriented()?,
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

fn lookup_directory(
    source: &impl RangeSource,
    bootstrap: &Bootstrap,
    query: &QuerySpec,
) -> ExperimentResult<DirectoryLookup> {
    let pages = bootstrap
        .root
        .pages
        .iter()
        .filter(|page| {
            page.sample == query.sample
                && page.contig == query.contig
                && page.start < query.end
                && page.end > query.start
        })
        .collect::<Vec<_>>();
    if pages.is_empty() {
        return Err(invalid_data(format!("no directory pages cover query {}", query.id)).into());
    }

    let bootstrap_end = usize_to_u64(bootstrap.bytes.len())?;
    let mut needed_ranges = BTreeSet::new();
    for page in &pages {
        let page_end = page
            .offset
            .checked_add(page.length)
            .ok_or_else(|| invalid_data("directory page range overflow"))?;
        let start = page.offset.max(bootstrap_end);
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
        let bytes = source.read_range(range.start, u64_to_usize(range.len())?)?;
        fetched.push((*range, bytes));
    }

    let mut entries = Vec::new();
    let mut logical_bytes = bootstrap.root.logical_bytes;
    for page in &pages {
        logical_bytes = logical_bytes
            .checked_add(page.length)
            .ok_or_else(|| invalid_data("logical directory byte count overflow"))?;
        let bytes = collect_stored_range(
            ByteRange {
                start: page.offset,
                end: page
                    .offset
                    .checked_add(page.length)
                    .ok_or_else(|| invalid_data("directory page range overflow"))?,
            },
            &bootstrap.bytes,
            &fetched,
        )?;
        let page_index = decode_index(&bytes, page.entry_count)?;
        for entry in page_index.entries {
            if entry.sample != page.sample
                || entry.contig != page.contig
                || entry.start < page.start
                || entry.end > page.end
            {
                return Err(
                    invalid_data("directory page entry is outside its root descriptor").into(),
                );
            }
            if entry.start < query.end && entry.end > query.start {
                entries.push(entry);
            }
        }
    }
    if entries.is_empty() {
        return Err(invalid_data(format!("no chunks cover query {}", query.id)).into());
    }

    Ok(DirectoryLookup {
        entries,
        logical_bytes,
        fetched_bytes: planned_ranges.iter().map(|range| range.len()).sum(),
        fetched_ranges: usize_to_u64(planned_ranges.len())?,
        selected_pages: usize_to_u64(pages.len())?,
    })
}

#[derive(Clone, Copy, Debug)]
struct Header {
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
    if bytes.len() != HEADER_LEN || &bytes[..8] != ARCHIVE_MAGIC {
        return Err(invalid_data("invalid archive header"));
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().expect("fixed header slice"));
    let header_len = u32::from_le_bytes(bytes[12..16].try_into().expect("fixed header slice"));
    let root_offset = u64::from_le_bytes(bytes[16..24].try_into().expect("fixed header slice"));
    if version != ARCHIVE_VERSION
        || usize::try_from(header_len).ok() != Some(HEADER_LEN)
        || root_offset != 64
    {
        return Err(invalid_data(format!(
            "unsupported archive version {version}, header length {header_len}, or root offset {root_offset}"
        )));
    }
    Ok(Header {
        root_len: u64::from_le_bytes(bytes[24..32].try_into().expect("fixed header slice")),
        entry_count: u64::from_le_bytes(bytes[32..40].try_into().expect("fixed header slice")),
        data_offset: u64::from_le_bytes(bytes[40..48].try_into().expect("fixed header slice")),
    })
}

fn archive_entries(
    pending_entries: &[PendingEntry],
    chunks: &[ChunkBlob],
    codec: ChunkCodec,
) -> ExperimentResult<Vec<ArchiveEntry>> {
    pending_entries
        .iter()
        .map(|entry| {
            let chunk = chunks
                .get(entry.chunk_id)
                .ok_or_else(|| invalid_data("directory entry refers to an absent chunk"))?;
            Ok(ArchiveEntry {
                sample: entry.sample.clone(),
                contig: entry.contig.clone(),
                start: entry.start,
                end: entry.end,
                offset: chunk.offset,
                compressed_len: usize_to_u64(chunk.compressed.len())?,
                uncompressed_len: chunk.uncompressed_len,
                codec,
            })
        })
        .collect()
}

fn partition_directory_pages(entries: &[ArchiveEntry]) -> ExperimentResult<Vec<Range<usize>>> {
    let mut result = Vec::new();
    let mut page_start = 0;
    let mut page_bytes = std::mem::size_of::<u64>();
    for (index, entry) in entries.iter().enumerate() {
        if entry.start >= entry.end {
            return Err(invalid_data("directory entry has an empty coordinate interval").into());
        }
        let entry_bytes = encoded_index_entry_len(entry)?;
        let starts_new_reference = index > page_start
            && (entry.sample != entries[page_start].sample
                || entry.contig != entries[page_start].contig);
        let exceeds_target = index > page_start
            && page_bytes
                .checked_add(entry_bytes)
                .is_none_or(|bytes| bytes > DIRECTORY_PAGE_TARGET_BYTES);
        if starts_new_reference || exceeds_target {
            result.push(page_start..index);
            page_start = index;
            page_bytes = std::mem::size_of::<u64>();
        }
        page_bytes = page_bytes
            .checked_add(entry_bytes)
            .ok_or_else(|| invalid_data("directory page length overflow"))?;
    }
    if page_start < entries.len() {
        result.push(page_start..entries.len());
    }
    Ok(result)
}

fn encoded_index_entry_len(entry: &ArchiveEntry) -> ExperimentResult<usize> {
    // Two string lengths, five numeric fields, and one codec/reserved word.
    let fixed_bytes = 8_usize
        .checked_mul(std::mem::size_of::<u64>())
        .ok_or_else(|| invalid_data("directory entry length overflow"))?;
    fixed_bytes
        .checked_add(entry.sample.len())
        .and_then(|bytes| bytes.checked_add(entry.contig.len()))
        .ok_or_else(|| invalid_data("directory entry length overflow").into())
}

fn encode_root_index(pages: &[DirectoryPageRef]) -> ExperimentResult<Vec<u8>> {
    let mut output = Vec::new();
    put_u64(&mut output, usize_to_u64(pages.len())?);
    for page in pages {
        put_string(&mut output, &page.sample)?;
        put_string(&mut output, &page.contig)?;
        put_u64(&mut output, page.start);
        put_u64(&mut output, page.end);
        put_u64(&mut output, page.offset);
        put_u64(&mut output, page.length);
        put_u64(&mut output, page.entry_count);
    }
    Ok(output)
}

fn decode_root_index(bytes: &[u8], header: Header) -> ExperimentResult<RootIndex> {
    let mut reader = BinaryReader::new(bytes);
    let count = reader.u64()?;
    let mut pages = Vec::with_capacity(u64_to_usize(count)?);
    let root_end = usize_to_u64(HEADER_LEN)?
        .checked_add(header.root_len)
        .ok_or_else(|| invalid_data("root index end overflow"))?;
    let mut previous_end = root_end;
    let mut total_entries = 0_u64;
    for _ in 0..count {
        let page = DirectoryPageRef {
            sample: reader.string()?,
            contig: reader.string()?,
            start: reader.u64()?,
            end: reader.u64()?,
            offset: reader.u64()?,
            length: reader.u64()?,
            entry_count: reader.u64()?,
        };
        let page_end = page
            .offset
            .checked_add(page.length)
            .ok_or_else(|| invalid_data("directory page end overflow"))?;
        if page.start >= page.end
            || page.length == 0
            || page.entry_count == 0
            || page.offset < previous_end
            || page_end > header.data_offset
        {
            return Err(invalid_data("invalid root directory page descriptor").into());
        }
        previous_end = page_end;
        total_entries = total_entries
            .checked_add(page.entry_count)
            .ok_or_else(|| invalid_data("directory entry count overflow"))?;
        pages.push(page);
    }
    reader.finish()?;
    if total_entries != header.entry_count {
        return Err(invalid_data(format!(
            "header entry count {} does not match root directory {total_entries}",
            header.entry_count
        ))
        .into());
    }
    Ok(RootIndex {
        logical_bytes: root_end,
        pages,
    })
}

fn encode_index(entries: &[ArchiveEntry]) -> ExperimentResult<Vec<u8>> {
    let mut output = Vec::new();
    put_u64(&mut output, usize_to_u64(entries.len())?);
    for entry in entries {
        put_string(&mut output, &entry.sample)?;
        put_string(&mut output, &entry.contig)?;
        put_u64(&mut output, entry.start);
        put_u64(&mut output, entry.end);
        put_u64(&mut output, entry.offset);
        put_u64(&mut output, entry.compressed_len);
        put_u64(&mut output, entry.uncompressed_len);
        output.push(entry.codec.code());
        output.extend_from_slice(&[0_u8; 7]);
    }
    Ok(output)
}

fn decode_index(bytes: &[u8], expected_count: u64) -> ExperimentResult<ArchiveIndex> {
    let mut reader = BinaryReader::new(bytes);
    let count = reader.u64()?;
    if count != expected_count {
        return Err(invalid_data(format!(
            "header entry count {expected_count} does not match index {count}"
        ))
        .into());
    }
    let mut entries = Vec::with_capacity(u64_to_usize(count)?);
    for _ in 0..count {
        let sample = reader.string()?;
        let contig = reader.string()?;
        let start = reader.u64()?;
        let end = reader.u64()?;
        let offset = reader.u64()?;
        let compressed_len = reader.u64()?;
        let uncompressed_len = reader.u64()?;
        let codec = ChunkCodec::from_code(reader.u8()?)?;
        let _reserved = reader.take(7)?;
        entries.push(ArchiveEntry {
            sample,
            contig,
            start,
            end,
            offset,
            compressed_len,
            uncompressed_len,
            codec,
        });
    }
    reader.finish()?;
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

    fn oriented(&mut self) -> io::Result<OrientedNode> {
        Ok(OrientedNode {
            id: self.u64()?,
            reverse: self.u8()? != 0,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn directory_pages_are_bounded_and_do_not_mix_references() {
        let entries = (0..100_u64)
            .map(|index| ArchiveEntry {
                sample: if index < 70 { "sample-a" } else { "sample-b" }.into(),
                contig: "chr1".into(),
                start: index * 16_384,
                end: (index + 1) * 16_384,
                offset: 0,
                compressed_len: 100,
                uncompressed_len: 200,
                codec: ChunkCodec::Zstd3,
            })
            .collect::<Vec<_>>();

        let pages = partition_directory_pages(&entries).unwrap();
        assert!(pages.len() > 2);
        for page in pages {
            let encoded = encode_index(&entries[page.clone()]).unwrap();
            assert!(encoded.len() <= DIRECTORY_PAGE_TARGET_BYTES);
            assert!(
                entries[page.clone()]
                    .iter()
                    .all(|entry| entry.sample == entries[page.start].sample)
            );
        }
    }

    #[test]
    fn root_index_round_trip_preserves_leaf_page_descriptors() {
        let mut pages = vec![
            DirectoryPageRef {
                sample: "sample".into(),
                contig: "chr1".into(),
                start: 0,
                end: 16_384,
                offset: 0,
                length: 256,
                entry_count: 2,
            },
            DirectoryPageRef {
                sample: "sample".into(),
                contig: "chr1".into(),
                start: 16_384,
                end: 32_768,
                offset: 0,
                length: 384,
                entry_count: 3,
            },
        ];
        let provisional = encode_root_index(&pages).unwrap();
        let root_end = u64::try_from(HEADER_LEN + provisional.len()).unwrap();
        pages[0].offset = root_end;
        pages[1].offset = root_end + pages[0].length;
        let encoded = encode_root_index(&pages).unwrap();
        let header = Header {
            root_len: u64::try_from(encoded.len()).unwrap(),
            entry_count: 5,
            data_offset: root_end + pages.iter().map(|page| page.length).sum::<u64>(),
        };

        let decoded = decode_root_index(&encoded, header).unwrap();
        assert_eq!(decoded.pages.len(), pages.len());
        assert_eq!(decoded.pages[0].offset, pages[0].offset);
        assert_eq!(decoded.pages[1].entry_count, pages[1].entry_count);
        assert_eq!(decoded.logical_bytes, root_end);
    }
}
