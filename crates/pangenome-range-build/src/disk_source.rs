use crate::gbwt_locate::GbwtLocate;
use crate::source::{
    PangenomeSource, SourceLocatedPosition, SourcePathCatalogRecord, SourcePathIndex,
    SourceReferenceSeed,
};
use gbz::bwt::Record;
use gbz::headers::{GBWTPayload, GBZPayload, Header, SequencesPayload};
use gbz::support::RLEIter;
use gbz::{
    FullPathName, GENERIC_HAPLOTYPE, GENERIC_SAMPLE, Metadata, Orientation, Pos,
    REFERENCE_SAMPLES_KEY, Tags,
};
use sha2::{Digest, Sha256};
use simple_sds::ops::{BitVec, Select};
use simple_sds::serialize::Serialize;
use simple_sds::sparse_vector::SparseVector;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static NEXT_CACHE_ID: AtomicU64 = AtomicU64::new(0);
const READ_BLOCK_BYTES: usize = 256 * 1024;
const READ_CACHE_BYTES_PER_FILE: usize = 16 * 1024 * 1024;
const READ_CACHE_SHARDS: usize = 16;
const SOURCE_CACHE_MAGIC: &str = "pangenome-range-source-cache";
const SOURCE_CACHE_VERSION: u32 = 2;
const SOURCE_CACHE_MANIFEST: &str = "manifest.json";
const SOURCE_PATH_INDEX_FILE: &str = "source-path-index.bin";
const SOURCE_DA_SAMPLES_FILE: &str = "gbwt-da-samples.bin";
const SOURCE_PATH_CATALOG_FILE: &str = "source-path-catalog.json";
const SOURCE_PATH_INDEX_INTERVAL: usize = 1_000;
const MAX_SOURCE_CACHE_MANIFEST_BYTES: u64 = 1024 * 1024;

fn follow_selected_offsets(
    record: Record<'_>,
    mut indices: Vec<usize>,
    current: &mut [Pos],
) -> io::Result<()> {
    indices.sort_unstable_by_key(|index| current[*index].offset);
    let (_, mut edges, bwt) = record.into_raw_parts();
    let mut next_index = 0_usize;
    let mut run_start = 0_usize;
    for run in RLEIter::with_sigma(bwt, edges.len()) {
        let run_end = run_start
            .checked_add(run.len)
            .ok_or_else(|| invalid_data("GBWT record run length overflow"))?;
        let edge = edges
            .get_mut(run.value)
            .ok_or_else(|| invalid_data("GBWT record run value is outside its edge list"))?;
        while let Some(&index) = indices.get(next_index) {
            let offset = current[index].offset;
            if offset >= run_end {
                break;
            }
            if offset < run_start {
                return Err(invalid_data("GBWT locate offsets are not monotone"));
            }
            let successor_offset = edge
                .offset
                .checked_add(offset - run_start)
                .ok_or_else(|| invalid_data("GBWT successor offset overflow"))?;
            current[index] = Pos::new(edge.node, successor_offset);
            next_index += 1;
        }
        edge.offset = edge
            .offset
            .checked_add(run.len)
            .ok_or_else(|| invalid_data("GBWT edge offset overflow"))?;
        run_start = run_end;
        if next_index == indices.len() {
            return Ok(());
        }
    }
    Err(invalid_data("GBWT locate offset is outside its record"))
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCacheManifest {
    pub magic: String,
    pub cache_format_version: u32,
    pub implementation_version: String,
    pub source_gbz_bytes: u64,
    pub source_gbz_sha256: String,
    pub gbz_serialization_version: u32,
    pub gbwt_serialization_version: u32,
    pub sequences_serialization_version: u32,
    pub alphabet_offset: u64,
    pub first_node: u64,
    pub record_count: u64,
    pub record_bytes: u64,
    pub record_offset_bytes: u64,
    pub sequence_count: u64,
    pub sequence_bytes: u64,
    pub sequence_offset_bytes: u64,
    pub reference_metadata_sha256: String,
    pub path_index_interval: u64,
    pub path_index_paths: u64,
    pub path_index_samples: u64,
    pub path_index_bytes: u64,
    pub path_index_sha256: String,
    pub da_samples_bytes: Option<u64>,
    pub da_samples_sha256: Option<String>,
    #[serde(default)]
    pub path_catalog_records: u64,
    #[serde(default)]
    pub path_catalog_bytes: u64,
    #[serde(default)]
    pub path_catalog_sha256: String,
}

pub struct PersistentSourceCache {
    pub source: DiskGbzSource,
    pub path_index: SourcePathIndex,
    pub manifest: SourceCacheManifest,
    pub open_metrics: SourceCacheOpenMetrics,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCacheOpenMetrics {
    pub manifest_validation_wall_ms: f64,
    pub path_index_deserialize_wall_ms: f64,
    pub component_open_wall_ms: f64,
    pub total_wall_ms: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DiskSourceStats {
    pub cache_directory: PathBuf,
    pub record_count: u64,
    pub record_bytes: u64,
    pub sequence_count: u64,
    pub sequence_bytes: u64,
    pub cache_bytes: u64,
    pub memory_cache_limit_bytes: u64,
}

/// A GBZ source backed by an ephemeral, directly indexed disk cache.
///
/// GBZ v3 stores the complete packed-record body and concatenated node
/// sequences as two monolithic zstd frames. Opening this source streams those
/// frames to disk once, then serves only requested records and sequences.
pub struct DiskGbzSource {
    record_offsets: BlockCache,
    records: BlockCache,
    sequence_offsets: BlockCache,
    sequences: BlockCache,
    alphabet_offset: usize,
    first_node: usize,
    record_count: usize,
    sequence_count: usize,
    reference_seeds: Vec<SourceReferenceSeed>,
    da_samples: Option<GbwtLocate>,
    path_catalog: Vec<SourcePathCatalogRecord>,
    stats: DiskSourceStats,
    serialization_versions: [u32; 3],
    persistent: bool,
    cache_guard: Option<CacheDirectoryGuard>,
}

impl DiskGbzSource {
    /// Builds an ephemeral disk cache by streaming the compressed GBZ bodies.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported/corrupt GBZ input, insufficient disk,
    /// or cache I/O failures.
    pub fn build(input: &Path, scratch_parent: &Path) -> io::Result<Self> {
        Self::build_with_digest(input, scratch_parent).map(|(source, _)| source)
    }

    /// Builds an ephemeral disk cache while using one real named sample/haplotype
    /// as the archive reference, even when the GBWT does not tag that sample as
    /// a reference.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported/corrupt input or when the exact named
    /// sample/haplotype has no nonempty paths.
    pub fn build_for_reference_haplotype(
        input: &Path,
        scratch_parent: &Path,
        sample: &str,
        haplotype: usize,
    ) -> io::Result<Self> {
        Self::build_with_digest_for_reference_haplotype(input, scratch_parent, sample, haplotype)
            .map(|(source, _)| source)
    }

    /// Builds the ephemeral cache and hashes the source in the same sequential pass.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported/corrupt GBZ input, insufficient disk,
    /// cache I/O failures, or source-digest failures.
    pub fn build_with_digest(input: &Path, scratch_parent: &Path) -> io::Result<(Self, [u8; 32])> {
        Self::build_with_digest_and_reference(input, scratch_parent, None)
    }

    /// Builds an ephemeral cache and hashes the source while explicitly
    /// selecting one real named sample/haplotype as the reference anchor.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported/corrupt input or when the exact named
    /// sample/haplotype has no nonempty paths.
    pub fn build_with_digest_for_reference_haplotype(
        input: &Path,
        scratch_parent: &Path,
        sample: &str,
        haplotype: usize,
    ) -> io::Result<(Self, [u8; 32])> {
        Self::build_with_digest_and_reference(input, scratch_parent, Some((sample, haplotype)))
    }

    fn build_with_digest_and_reference(
        input: &Path,
        scratch_parent: &Path,
        explicit_reference: Option<(&str, usize)>,
    ) -> io::Result<(Self, [u8; 32])> {
        fs::create_dir_all(scratch_parent)?;
        let cache_directory = create_cache_directory(scratch_parent)?;
        match Self::build_in(input, &cache_directory, explicit_reference) {
            Ok(source) => Ok(source),
            Err(error) => {
                let _ = fs::remove_dir_all(&cache_directory);
                Err(error)
            }
        }
    }

    fn build_in(
        input: &Path,
        cache_directory: &Path,
        explicit_reference: Option<(&str, usize)>,
    ) -> io::Result<(Self, [u8; 32])> {
        let digesting = DigestingReader::new(File::open(input)?);
        let mut reader = BufReader::with_capacity(1024 * 1024, digesting);

        let gbz_header = Header::<GBZPayload>::load(&mut reader)?;
        validate_header(&gbz_header)?;
        let _container_tags = Tags::load(&mut reader)?;

        let gbwt_header = Header::<GBWTPayload>::load(&mut reader)?;
        validate_header(&gbwt_header)?;
        if !gbwt_header.is_set(GBWTPayload::FLAG_BIDIRECTIONAL) {
            return Err(invalid_data("GBZ requires a bidirectional GBWT"));
        }
        let gbwt_tags = Tags::load(&mut reader)?;
        let record_index = SparseVector::load(&mut reader)?;
        let record_count = record_index.count_ones();
        if record_count == 0 {
            return Err(invalid_data("GBWT record index is empty"));
        }

        let record_offsets_path = cache_directory.join("records.offsets");
        write_sparse_offsets(&record_offsets_path, &record_index, record_index.len())?;
        let records_path = cache_directory.join("records.data");
        let expected_record_bytes = u64::try_from(record_index.len())
            .map_err(|_| invalid_data("GBWT record data length does not fit in u64"))?;
        let record_bytes = if gbwt_header.version() >= GBWTPayload::ZSTD_VERSION {
            stream_compressed_vector(&mut reader, &records_path, expected_record_bytes)?
        } else {
            stream_raw_byte_vector(&mut reader, &records_path, expected_record_bytes)?
        };
        drop(record_index);

        let da_samples = GbwtLocate::load(&mut reader)?;
        let metadata_elements = usize::load(&mut reader)?;
        if metadata_elements == 0 {
            return Err(invalid_data("GBZ metadata is required"));
        }
        let metadata = Metadata::load(&mut reader)?;

        let sequences_header = Header::<SequencesPayload>::load(&mut reader)?;
        validate_header(&sequences_header)?;
        let sequence_index = SparseVector::load(&mut reader)?;
        let sequence_count = sequence_index.count_ones();
        let sequence_offsets_path = cache_directory.join("sequences.offsets");
        let sequences_path = cache_directory.join("sequences.data");
        let sequence_bytes = if sequences_header.version() >= SequencesPayload::ZSTD_VERSION {
            let expected = usize::load(&mut reader)?;
            write_sparse_offsets(&sequence_offsets_path, &sequence_index, expected)?;
            stream_compressed_vector(&mut reader, &sequences_path, usize_to_u64(expected)?)?
        } else {
            let alphabet = Vec::<u8>::load(&mut reader)?;
            let sequence_bytes =
                stream_packed_byte_vector(&mut reader, &sequences_path, &alphabet)?;
            write_sparse_offsets(
                &sequence_offsets_path,
                &sequence_index,
                usize::try_from(sequence_bytes)
                    .map_err(|_| invalid_data("sequence data length does not fit in usize"))?,
            )?;
            sequence_bytes
        };
        drop(sequence_index);

        io::copy(&mut reader, &mut io::sink())?;
        let source_sha256 = reader.into_inner().finish();

        let alphabet_offset = gbwt_header.payload().offset;
        let first_node = alphabet_offset
            .checked_add(1)
            .ok_or_else(|| invalid_data("GBWT first-node overflow"))?;
        let path_catalog =
            build_path_catalog(&metadata, &gbwt_tags, gbwt_header.payload().sequences / 2)?;
        let mut source = Self {
            record_offsets: BlockCache::open(&record_offsets_path)?,
            records: BlockCache::open(&records_path)?,
            sequence_offsets: BlockCache::open(&sequence_offsets_path)?,
            sequences: BlockCache::open(&sequences_path)?,
            alphabet_offset,
            first_node,
            record_count,
            sequence_count,
            reference_seeds: Vec::new(),
            da_samples,
            path_catalog,
            stats: DiskSourceStats {
                cache_directory: cache_directory.to_path_buf(),
                record_count: usize_to_u64(record_count)?,
                record_bytes,
                sequence_count: usize_to_u64(sequence_count)?,
                sequence_bytes,
                cache_bytes: directory_bytes(cache_directory)?,
                memory_cache_limit_bytes: usize_to_u64(4 * READ_CACHE_BYTES_PER_FILE)?,
            },
            serialization_versions: [
                gbz_header.version(),
                gbwt_header.version(),
                sequences_header.version(),
            ],
            persistent: false,
            cache_guard: Some(CacheDirectoryGuard {
                path: Some(cache_directory.to_path_buf()),
            }),
        };
        source.reference_seeds =
            source.build_reference_seeds(&metadata, &gbwt_tags, explicit_reference)?;
        Ok((source, source_sha256))
    }

    #[must_use]
    pub fn stats(&self) -> &DiskSourceStats {
        &self.stats
    }

    fn release_ephemeral_guard(&mut self) {
        if let Some(guard) = &mut self.cache_guard {
            guard.path = None;
        }
        self.cache_guard = None;
    }

    fn build_reference_seeds(
        &self,
        metadata: &Metadata,
        gbwt_tags: &Tags,
        explicit_reference: Option<(&str, usize)>,
    ) -> io::Result<Vec<SourceReferenceSeed>> {
        let endmarker_bytes = self
            .record_by_id(0)?
            .ok_or_else(|| invalid_data("GBWT endmarker record is empty"))?;
        let endmarker = Record::new(0, &endmarker_bytes)
            .ok_or_else(|| invalid_data("GBWT endmarker record is invalid"))?
            .decompress();
        let reference_names = if explicit_reference.is_none() {
            let mut names = BTreeSet::new();
            if let Some(tagged) = gbwt_tags.get(REFERENCE_SAMPLES_KEY) {
                names.extend(tagged.split(' ').map(str::to_owned));
            }
            names.insert(GENERIC_SAMPLE.to_owned());
            Some(names)
        } else {
            None
        };
        let mut seeds = Vec::new();
        for (path_id, path) in metadata.path_iter().enumerate() {
            let sample = metadata.sample_name(path.sample());
            let selected = explicit_reference.map_or_else(
                || {
                    reference_names
                        .as_ref()
                        .is_some_and(|names| names.contains(&sample))
                },
                |(selected_sample, selected_haplotype)| {
                    sample == selected_sample && path.phase() == selected_haplotype
                },
            );
            if !selected {
                continue;
            }
            let name = FullPathName::from_metadata(metadata, path_id)
                .ok_or_else(|| invalid_data(format!("missing metadata for path {path_id}")))?;
            let sequence_id = gbz::support::encode_path(path_id, Orientation::Forward);
            let position = endmarker
                .get(sequence_id)
                .copied()
                .filter(|position| position.node != gbz::ENDMARKER)
                .ok_or_else(|| invalid_data(format!("reference path {path_id} is empty")))?;
            seeds.push(SourceReferenceSeed {
                path_id,
                name,
                position,
            });
        }
        if seeds.is_empty() {
            if let Some((sample, haplotype)) = explicit_reference {
                return Err(invalid_data(format!(
                    "GBZ has no nonempty paths for explicit reference sample '{sample}' haplotype {haplotype}"
                )));
            }
            return Err(invalid_data("GBZ has no reference paths"));
        }
        Ok(seeds)
    }

    fn record_by_id(&self, record_id: usize) -> io::Result<Option<Vec<u8>>> {
        if record_id >= self.record_count {
            return Ok(None);
        }
        read_indexed_bytes(
            &self.record_offsets,
            &self.records,
            record_id,
            self.record_count,
        )
    }

    fn sequence_by_id(&self, sequence_id: usize) -> io::Result<Option<Vec<u8>>> {
        if sequence_id >= self.sequence_count {
            return Ok(None);
        }
        read_indexed_bytes(
            &self.sequence_offsets,
            &self.sequences,
            sequence_id,
            self.sequence_count,
        )
    }

    fn position_record_id(&self, position: Pos) -> Option<usize> {
        if position.node == gbz::ENDMARKER {
            Some(0)
        } else if position.node >= self.first_node {
            let record_id = position.node.checked_sub(self.alphabet_offset)?;
            (record_id < self.record_count).then_some(record_id)
        } else {
            None
        }
    }

    fn locate_batch(
        &self,
        positions: &[Pos],
        max_lf_steps: usize,
    ) -> io::Result<Vec<SourceLocatedPosition>> {
        let da = self.da_samples.as_ref().ok_or_else(|| {
            invalid_data("GBWT has no parsed document-array samples for path locate")
        })?;
        let mut current = positions.to_vec();
        let mut result = vec![None; positions.len()];
        for lf_steps in 0..=max_lf_steps {
            let mut by_record = std::collections::BTreeMap::<usize, Vec<usize>>::new();
            for (index, position) in current.iter().copied().enumerate() {
                if result[index].is_some() {
                    continue;
                }
                let record_id = self.position_record_id(position).ok_or_else(|| {
                    invalid_data(format!(
                        "invalid GBWT locate position {}:{}",
                        position.node, position.offset
                    ))
                })?;
                if let Some(sequence_id) = da.try_locate(record_id, position.offset) {
                    let (path_id, orientation) = gbz::support::decode_path(sequence_id);
                    result[index] = Some(SourceLocatedPosition {
                        sequence_id: usize_to_u64(sequence_id)?,
                        path_id: usize_to_u64(path_id)?,
                        reversed: orientation == Orientation::Reverse,
                        lf_steps: usize_to_u64(lf_steps)?,
                    });
                } else {
                    by_record.entry(record_id).or_default().push(index);
                }
            }
            if by_record.is_empty() {
                return result
                    .into_iter()
                    .map(|item| item.ok_or_else(|| invalid_data("missing GBWT locate result")))
                    .collect();
            }
            if lf_steps == max_lf_steps {
                return Err(invalid_data(format!(
                    "GBWT locate exceeded the bounded limit of {max_lf_steps} LF steps"
                )));
            }
            for (record_id, indices) in by_record {
                if record_id == 0 {
                    return Err(invalid_data(
                        "unsampled GBWT endmarker reached during path locate",
                    ));
                }
                let bytes = self
                    .record_by_id(record_id)?
                    .ok_or_else(|| invalid_data("GBWT locate record is missing"))?;
                let node = record_id
                    .checked_add(self.alphabet_offset)
                    .ok_or_else(|| invalid_data("GBWT locate node overflow"))?;
                let record = Record::new(node, &bytes)
                    .ok_or_else(|| invalid_data("GBWT locate record is invalid"))?;
                follow_selected_offsets(record, indices, &mut current)?;
            }
        }
        unreachable!("bounded LF loop returns on success or limit")
    }
}

impl PangenomeSource for DiskGbzSource {
    fn access_mode(&self) -> &'static str {
        if self.persistent {
            "persistent-disk-backed-gbz"
        } else {
            "disk-backed-gbz"
        }
    }

    fn reference_seeds(&self) -> io::Result<Vec<SourceReferenceSeed>> {
        Ok(self.reference_seeds.clone())
    }

    fn sequence(&self, node_id: usize) -> io::Result<Option<Vec<u8>>> {
        let handle = gbz::support::encode_node(node_id, Orientation::Forward);
        if handle < self.first_node {
            return Ok(None);
        }
        let sequence_id = (handle - self.first_node) / 2;
        self.sequence_by_id(sequence_id)
    }

    fn sequence_len(&self, node_id: usize) -> io::Result<Option<usize>> {
        let handle = gbz::support::encode_node(node_id, Orientation::Forward);
        if handle < self.first_node {
            return Ok(None);
        }
        let sequence_id = (handle - self.first_node) / 2;
        indexed_length(&self.sequence_offsets, sequence_id, self.sequence_count)
    }

    fn packed_record(&self, packed_handle: usize) -> io::Result<Option<Vec<u8>>> {
        if packed_handle < self.alphabet_offset {
            return Ok(None);
        }
        let record_id = packed_handle - self.alphabet_offset;
        let bytes = self.record_by_id(record_id)?;
        Ok(bytes.filter(|bytes| Record::new(0, bytes).is_some()))
    }

    fn path_catalog(&self) -> io::Result<Option<&[SourcePathCatalogRecord]>> {
        Ok((!self.path_catalog.is_empty()).then_some(self.path_catalog.as_slice()))
    }

    fn locate_positions(
        &self,
        positions: &[Pos],
        max_lf_steps: usize,
    ) -> io::Result<Option<Vec<SourceLocatedPosition>>> {
        self.locate_batch(positions, max_lf_steps).map(Some)
    }
}

/// Reports conservative scratch capacity for a source-cache build.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SourceCacheDiskPreflight {
    pub required_bytes: u64,
    pub available_bytes: Option<u64>,
    pub sufficient: Option<bool>,
}

/// Uses the observed raw-cache ratio as a conservative preflight, with a 1 GiB floor.
///
/// # Errors
///
/// Returns an error when source metadata cannot be read or the conservative
/// byte estimate overflows.
pub fn source_cache_disk_preflight(
    input: &Path,
    parent: &Path,
) -> io::Result<SourceCacheDiskPreflight> {
    let source_bytes = fs::metadata(input)?.len();
    let required_bytes = source_bytes
        .checked_mul(3)
        .map(|bytes| bytes.max(1024 * 1024 * 1024))
        .ok_or_else(|| invalid_data("source cache scratch estimate overflow"))?;
    let available_bytes = available_space_bytes(parent);
    Ok(SourceCacheDiskPreflight {
        required_bytes,
        available_bytes,
        sufficient: available_bytes.map(|available| available >= required_bytes),
    })
}

/// Builds a versioned persistent cache into a temporary sibling and atomically promotes it.
///
/// # Errors
///
/// Returns an error for invalid paths, an active cache lock, insufficient
/// space, unsupported/corrupt GBZ input, cache I/O, or failed atomic promotion.
pub fn build_persistent_source_cache(
    input: &Path,
    target: &Path,
    rebuild: bool,
) -> io::Result<PersistentSourceCache> {
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            invalid_data("persistent source cache needs an explicit parent directory")
        })?;
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_data("persistent source cache needs a valid directory name"))?;
    fs::create_dir_all(parent)?;
    let preflight = source_cache_disk_preflight(input, parent)?;
    if preflight.sufficient == Some(false) {
        return Err(io::Error::new(
            io::ErrorKind::StorageFull,
            format!(
                "source cache requires approximately {} bytes but only {} bytes are available in {}",
                preflight.required_bytes,
                preflight.available_bytes.unwrap_or(0),
                parent.display()
            ),
        ));
    }
    let lock_path = parent.join(format!(".{name}.lock"));
    let _lock = SourceCacheLock::acquire(&lock_path)?;
    if target.exists() && !rebuild {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "persistent source cache {} already exists; inspect it, use it, or rebuild explicitly",
                target.display()
            ),
        ));
    }

    let (mut source, source_sha256) = DiskGbzSource::build_with_digest(input, parent)?;
    let temporary = source.stats.cache_directory.clone();
    let manifest = write_source_cache_manifest(input, &temporary, &source, source_sha256)?;

    source.release_ephemeral_guard();
    drop(source);
    let old = parent.join(format!(".{name}.old.{}", std::process::id()));
    if target.exists() {
        fs::rename(target, &old)?;
    }
    if let Err(error) = fs::rename(&temporary, target) {
        if old.exists() {
            let _ = fs::rename(&old, target);
        }
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    File::open(parent)?.sync_all()?;
    if old.exists() {
        fs::remove_dir_all(old)?;
    }
    open_persistent_source_cache_with_manifest(
        fs::metadata(input)?.len(),
        source_sha256,
        target,
        manifest,
    )
}

fn write_source_cache_manifest(
    input: &Path,
    temporary: &Path,
    source: &DiskGbzSource,
    source_sha256: [u8; 32],
) -> io::Result<SourceCacheManifest> {
    let path_index = SourcePathIndex::new(source, SOURCE_PATH_INDEX_INTERVAL)?;
    let path_index_bytes = path_index.encode()?;
    write_new_integrity_file(&temporary.join(SOURCE_PATH_INDEX_FILE), &path_index_bytes)?;
    let (da_samples_bytes, da_samples_sha256) = if let Some(da_samples) = &source.da_samples {
        let path = temporary.join(SOURCE_DA_SAMPLES_FILE);
        let mut writer = IntegrityFileWriter::create(&path)?;
        da_samples.save(&mut writer)?;
        writer.finish()?;
        (
            Some(fs::metadata(&path)?.len()),
            Some(hex_digest(&file_sha256(&path)?)),
        )
    } else {
        (None, None)
    };
    let path_catalog_path = temporary.join(SOURCE_PATH_CATALOG_FILE);
    let mut path_catalog_writer = IntegrityFileWriter::create(&path_catalog_path)?;
    serde_json::to_writer(&mut path_catalog_writer, &source.path_catalog)
        .map_err(|error| invalid_data(format!("cannot encode source path catalog: {error}")))?;
    path_catalog_writer.finish()?;
    let path_catalog_bytes = fs::metadata(&path_catalog_path)?.len();
    let stats = source.stats();
    let manifest = SourceCacheManifest {
        magic: SOURCE_CACHE_MAGIC.into(),
        cache_format_version: SOURCE_CACHE_VERSION,
        implementation_version: env!("CARGO_PKG_VERSION").into(),
        source_gbz_bytes: fs::metadata(input)?.len(),
        source_gbz_sha256: hex_digest(&source_sha256),
        gbz_serialization_version: source.serialization_versions[0],
        gbwt_serialization_version: source.serialization_versions[1],
        sequences_serialization_version: source.serialization_versions[2],
        alphabet_offset: usize_to_u64(source.alphabet_offset)?,
        first_node: usize_to_u64(source.first_node)?,
        record_count: stats.record_count,
        record_bytes: stats.record_bytes,
        record_offset_bytes: fs::metadata(temporary.join("records.offsets"))?.len(),
        sequence_count: stats.sequence_count,
        sequence_bytes: stats.sequence_bytes,
        sequence_offset_bytes: fs::metadata(temporary.join("sequences.offsets"))?.len(),
        reference_metadata_sha256: hex_digest(&path_index.reference_metadata_sha256()?),
        path_index_interval: usize_to_u64(path_index.interval())?,
        path_index_paths: usize_to_u64(path_index.path_count())?,
        path_index_samples: usize_to_u64(path_index.sample_count())?,
        path_index_bytes: usize_to_u64(path_index_bytes.len())?,
        path_index_sha256: hex_digest(&sha256_bytes(&path_index_bytes)),
        da_samples_bytes,
        da_samples_sha256,
        path_catalog_records: usize_to_u64(source.path_catalog.len())?,
        path_catalog_bytes,
        path_catalog_sha256: hex_digest(&file_sha256(&path_catalog_path)?),
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| invalid_data(format!("cannot encode source cache manifest: {error}")))?;
    if usize_to_u64(manifest_bytes.len())? > MAX_SOURCE_CACHE_MANIFEST_BYTES {
        return Err(invalid_data("source cache manifest exceeds its size limit"));
    }
    write_new_file(&temporary.join(SOURCE_CACHE_MANIFEST), &manifest_bytes)?;
    File::open(temporary)?.sync_all()?;
    Ok(manifest)
}

/// Opens and validates a persistent cache, including source identity and path-index checksum.
///
/// # Errors
///
/// Returns an error when the source cannot be hashed, the cache manifest or
/// components are corrupt/unsupported, or the source identity does not match.
pub fn open_persistent_source_cache(
    input: &Path,
    target: &Path,
) -> io::Result<PersistentSourceCache> {
    let manifest = read_source_cache_manifest(target)?;
    let source_gbz_bytes = fs::metadata(input)?.len();
    let source_sha256 = file_sha256(input)?;
    open_persistent_source_cache_with_manifest(source_gbz_bytes, source_sha256, target, manifest)
}

pub(crate) fn open_persistent_source_cache_with_digest(
    source_gbz_bytes: u64,
    source_sha256: [u8; 32],
    target: &Path,
) -> io::Result<PersistentSourceCache> {
    let manifest = read_source_cache_manifest(target)?;
    open_persistent_source_cache_with_manifest(source_gbz_bytes, source_sha256, target, manifest)
}

/// Inspects all bounded cache metadata and the serialized path index without opening GBZ data.
///
/// # Errors
///
/// Returns an error when the manifest, component lengths, path index, or
/// integrity metadata are missing, corrupt, or unsupported.
pub fn inspect_persistent_source_cache(target: &Path) -> io::Result<SourceCacheManifest> {
    let manifest = read_source_cache_manifest(target)?;
    let index_bytes = fs::read(target.join(SOURCE_PATH_INDEX_FILE))?;
    validate_path_index(&manifest, &index_bytes)?;
    Ok(manifest)
}

fn read_source_cache_manifest(target: &Path) -> io::Result<SourceCacheManifest> {
    let manifest_path = target.join(SOURCE_CACHE_MANIFEST);
    let manifest_len = fs::metadata(&manifest_path)?.len();
    if manifest_len == 0 || manifest_len > MAX_SOURCE_CACHE_MANIFEST_BYTES {
        return Err(invalid_data(
            "invalid persistent source cache manifest length",
        ));
    }
    let manifest: SourceCacheManifest = serde_json::from_slice(&fs::read(&manifest_path)?)
        .map_err(|error| invalid_data(format!("cannot decode source cache manifest: {error}")))?;
    if manifest.cache_format_version != SOURCE_CACHE_VERSION {
        return Err(invalid_data(format!(
            "source cache format {} is unsupported; rebuild it as format {}",
            manifest.cache_format_version, SOURCE_CACHE_VERSION
        )));
    }
    validate_manifest_files(target, &manifest)?;
    Ok(manifest)
}

/// Explicitly removes one validated persistent cache directory.
///
/// # Errors
///
/// Returns an error for unsafe paths, invalid caches, active locks, removal
/// failures, or failure to synchronize the parent directory.
pub fn prune_persistent_source_cache(target: &Path) -> io::Result<SourceCacheManifest> {
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            invalid_data("persistent source cache needs an explicit parent directory")
        })?;
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != "." && *value != "..")
        .ok_or_else(|| invalid_data("refusing to prune an unsafe source cache path"))?;
    let manifest = inspect_persistent_source_cache(target)?;
    let lock_path = parent.join(format!(".{name}.lock"));
    let _lock = SourceCacheLock::acquire(&lock_path)?;
    fs::remove_dir_all(target)?;
    File::open(parent)?.sync_all()?;
    Ok(manifest)
}

fn open_persistent_source_cache_with_manifest(
    source_bytes: u64,
    source_sha256: [u8; 32],
    target: &Path,
    manifest: SourceCacheManifest,
) -> io::Result<PersistentSourceCache> {
    let total_started = Instant::now();
    let manifest_started = Instant::now();
    validate_manifest_files(target, &manifest)?;
    if source_bytes != manifest.source_gbz_bytes {
        return Err(invalid_data(
            "persistent source cache GBZ byte length mismatch",
        ));
    }
    if hex_digest(&source_sha256) != manifest.source_gbz_sha256 {
        return Err(invalid_data(
            "persistent source cache GBZ checksum mismatch",
        ));
    }
    let manifest_validation_wall_ms = manifest_started.elapsed().as_secs_f64() * 1_000.0;
    let path_index_started = Instant::now();
    let path_index_bytes = fs::read(target.join(SOURCE_PATH_INDEX_FILE))?;
    let path_index = validate_path_index(&manifest, &path_index_bytes)?;
    let reference_seeds = path_index.reference_seeds()?;
    if hex_digest(&path_index.reference_metadata_sha256()?) != manifest.reference_metadata_sha256 {
        return Err(invalid_data(
            "persistent source reference metadata checksum mismatch",
        ));
    }
    let path_index_deserialize_wall_ms = path_index_started.elapsed().as_secs_f64() * 1_000.0;
    let record_count = usize::try_from(manifest.record_count)
        .map_err(|_| invalid_data("cached record count does not fit usize"))?;
    let sequence_count = usize::try_from(manifest.sequence_count)
        .map_err(|_| invalid_data("cached sequence count does not fit usize"))?;
    let da_samples = if manifest.da_samples_bytes.is_some() {
        let mut reader = BufReader::new(File::open(target.join(SOURCE_DA_SAMPLES_FILE))?);
        let locate = GbwtLocate::load_cache(&mut reader)?;
        let mut trailing = [0_u8; 1];
        if reader.read(&mut trailing)? != 0 {
            return Err(invalid_data(
                "cached GBWT document array has trailing bytes",
            ));
        }
        Some(locate)
    } else {
        None
    };
    let path_catalog: Vec<SourcePathCatalogRecord> = serde_json::from_reader(BufReader::new(
        File::open(target.join(SOURCE_PATH_CATALOG_FILE))?,
    ))
    .map_err(|error| invalid_data(format!("cannot decode cached source path catalog: {error}")))?;
    if usize_to_u64(path_catalog.len())? != manifest.path_catalog_records
        || path_catalog
            .iter()
            .enumerate()
            .any(|(index, record)| record.path_id != index as u64)
    {
        return Err(invalid_data(
            "cached source path catalog differs from its manifest",
        ));
    }
    let component_open_started = Instant::now();
    let source = DiskGbzSource {
        record_offsets: BlockCache::open(&target.join("records.offsets"))?,
        records: BlockCache::open(&target.join("records.data"))?,
        sequence_offsets: BlockCache::open(&target.join("sequences.offsets"))?,
        sequences: BlockCache::open(&target.join("sequences.data"))?,
        alphabet_offset: usize::try_from(manifest.alphabet_offset)
            .map_err(|_| invalid_data("cached alphabet offset does not fit usize"))?,
        first_node: usize::try_from(manifest.first_node)
            .map_err(|_| invalid_data("cached first node does not fit usize"))?,
        record_count,
        sequence_count,
        reference_seeds,
        da_samples,
        path_catalog,
        stats: DiskSourceStats {
            cache_directory: target.to_path_buf(),
            record_count: manifest.record_count,
            record_bytes: manifest.record_bytes,
            sequence_count: manifest.sequence_count,
            sequence_bytes: manifest.sequence_bytes,
            cache_bytes: directory_bytes(target)?,
            memory_cache_limit_bytes: usize_to_u64(4 * READ_CACHE_BYTES_PER_FILE)?,
        },
        serialization_versions: [
            manifest.gbz_serialization_version,
            manifest.gbwt_serialization_version,
            manifest.sequences_serialization_version,
        ],
        persistent: true,
        cache_guard: None,
    };
    let component_open_wall_ms = component_open_started.elapsed().as_secs_f64() * 1_000.0;
    Ok(PersistentSourceCache {
        source,
        path_index,
        manifest,
        open_metrics: SourceCacheOpenMetrics {
            manifest_validation_wall_ms,
            path_index_deserialize_wall_ms,
            component_open_wall_ms,
            total_wall_ms: total_started.elapsed().as_secs_f64() * 1_000.0,
        },
    })
}

fn validate_manifest_files(target: &Path, manifest: &SourceCacheManifest) -> io::Result<()> {
    if manifest.magic != SOURCE_CACHE_MAGIC
        || manifest.cache_format_version != SOURCE_CACHE_VERSION
        || manifest.implementation_version != env!("CARGO_PKG_VERSION")
        || manifest.source_gbz_bytes == 0
        || parse_hex_digest(&manifest.source_gbz_sha256).is_none()
        || manifest.record_count == 0
        || manifest.sequence_count == 0
        || manifest.path_index_interval == 0
        || manifest.path_index_paths == 0
        || manifest.path_catalog_bytes == 0
        || parse_hex_digest(&manifest.reference_metadata_sha256).is_none()
        || parse_hex_digest(&manifest.path_index_sha256).is_none()
        || parse_hex_digest(&manifest.path_catalog_sha256).is_none()
        || manifest.da_samples_bytes.is_some() != manifest.da_samples_sha256.is_some()
        || manifest
            .da_samples_sha256
            .as_deref()
            .is_some_and(|digest| parse_hex_digest(digest).is_none())
    {
        return Err(invalid_data("invalid persistent source cache manifest"));
    }
    for (name, expected) in [
        ("records.offsets", manifest.record_offset_bytes),
        ("records.data", manifest.record_bytes),
        ("sequences.offsets", manifest.sequence_offset_bytes),
        ("sequences.data", manifest.sequence_bytes),
        (SOURCE_PATH_INDEX_FILE, manifest.path_index_bytes),
        (SOURCE_PATH_CATALOG_FILE, manifest.path_catalog_bytes),
    ] {
        if fs::metadata(target.join(name))?.len() != expected {
            return Err(invalid_data(format!(
                "persistent source cache component {name} length mismatch"
            )));
        }
        let integrity_path = block_integrity_path(&target.join(name))?;
        let expected_integrity_bytes = expected
            .div_ceil(usize_to_u64(READ_BLOCK_BYTES)?)
            .checked_mul(16)
            .ok_or_else(|| invalid_data("source cache integrity length overflow"))?;
        if fs::metadata(integrity_path)?.len() != expected_integrity_bytes {
            return Err(invalid_data(format!(
                "persistent source cache component {name} integrity length mismatch"
            )));
        }
    }
    if let Some(expected) = manifest.da_samples_bytes {
        let path = target.join(SOURCE_DA_SAMPLES_FILE);
        if fs::metadata(&path)?.len() != expected {
            return Err(invalid_data(
                "persistent source cache DA-samples length mismatch",
            ));
        }
        let integrity_path = block_integrity_path(&path)?;
        let expected_integrity_bytes = expected
            .div_ceil(usize_to_u64(READ_BLOCK_BYTES)?)
            .checked_mul(16)
            .ok_or_else(|| invalid_data("source cache integrity length overflow"))?;
        if fs::metadata(integrity_path)?.len() != expected_integrity_bytes {
            return Err(invalid_data(
                "persistent source cache DA-samples integrity length mismatch",
            ));
        }
        if hex_digest(&file_sha256(&path)?) != manifest.da_samples_sha256.as_deref().unwrap() {
            return Err(invalid_data(
                "persistent source cache DA-samples checksum mismatch",
            ));
        }
    }
    if hex_digest(&file_sha256(&target.join(SOURCE_PATH_CATALOG_FILE))?)
        != manifest.path_catalog_sha256
    {
        return Err(invalid_data(
            "persistent source cache path-catalog checksum mismatch",
        ));
    }
    let expected_record_offsets = manifest
        .record_count
        .checked_add(1)
        .and_then(|count| count.checked_mul(8))
        .ok_or_else(|| invalid_data("cached record offset length overflow"))?;
    let expected_sequence_offsets = manifest
        .sequence_count
        .checked_add(1)
        .and_then(|count| count.checked_mul(8))
        .ok_or_else(|| invalid_data("cached sequence offset length overflow"))?;
    if manifest.record_offset_bytes != expected_record_offsets
        || manifest.sequence_offset_bytes != expected_sequence_offsets
    {
        return Err(invalid_data(
            "persistent source cache offset-table length mismatch",
        ));
    }
    Ok(())
}

fn validate_path_index(
    manifest: &SourceCacheManifest,
    bytes: &[u8],
) -> io::Result<SourcePathIndex> {
    if hex_digest(&sha256_bytes(bytes)) != manifest.path_index_sha256 {
        return Err(invalid_data(
            "persistent source path index checksum mismatch",
        ));
    }
    let index = SourcePathIndex::decode(bytes)?;
    if usize_to_u64(index.interval())? != manifest.path_index_interval
        || usize_to_u64(index.path_count())? != manifest.path_index_paths
        || usize_to_u64(index.sample_count())? != manifest.path_index_samples
    {
        return Err(invalid_data(
            "persistent source path index manifest mismatch",
        ));
    }
    Ok(index)
}

struct SourceCacheLock {
    path: PathBuf,
}

impl SourceCacheLock {
    fn acquire(path: &Path) -> io::Result<Self> {
        for attempt in 0..2 {
            match OpenOptions::new().create_new(true).write(true).open(path) {
                Ok(mut file) => {
                    writeln!(file, "{}", std::process::id())?;
                    file.sync_all()?;
                    return Ok(Self {
                        path: path.to_path_buf(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt == 0 => {
                    let stale = fs::read_to_string(path)
                        .ok()
                        .and_then(|value| value.trim().parse::<u32>().ok())
                        .is_some_and(|pid| !Path::new("/proc").join(pid.to_string()).exists());
                    if stale {
                        fs::remove_file(path)?;
                        continue;
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        format!("source cache lock {} is held", path.display()),
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "could not acquire source cache lock",
        ))
    }
}

impl Drop for SourceCacheLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct CacheDirectoryGuard {
    path: Option<PathBuf>,
}

impl Drop for CacheDirectoryGuard {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn create_cache_directory(parent: &Path) -> io::Result<PathBuf> {
    for _ in 0..1024 {
        let id = NEXT_CACHE_ID.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".pangenome-range-source.{}.{id}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique source cache directory",
    ))
}

fn validate_header<T: gbz::headers::Payload>(header: &Header<T>) -> io::Result<()> {
    header.validate().map_err(invalid_data)
}

fn write_sparse_offsets(path: &Path, index: &SparseVector, end: usize) -> io::Result<()> {
    let mut writer = IntegrityFileWriter::create(path)?;
    for (_, offset) in index.one_iter() {
        writer.write_all(&usize_to_u64(offset)?.to_le_bytes())?;
    }
    writer.write_all(&usize_to_u64(end)?.to_le_bytes())?;
    writer.finish()
}

fn stream_compressed_vector<R: Read>(
    reader: &mut BufReader<R>,
    output: &Path,
    expected_bytes: u64,
) -> io::Result<u64> {
    let compressed_bytes = usize::load(reader)?;
    let limited = reader.by_ref().take(usize_to_u64(compressed_bytes)?);
    let mut decoder = zstd::stream::read::Decoder::new(limited)?;
    let mut writer = IntegrityFileWriter::create(output)?;
    let limit = expected_bytes
        .checked_add(1)
        .ok_or_else(|| invalid_data("decompressed section length overflow"))?;
    let decoded_bytes = io::copy(&mut decoder.by_ref().take(limit), &mut writer)?;
    drop(decoder.finish());
    writer.finish()?;
    if decoded_bytes != expected_bytes {
        return Err(invalid_data(format!(
            "decompressed section length {decoded_bytes} differs from declared {expected_bytes}"
        )));
    }
    skip_padding(reader, compressed_bytes)?;
    Ok(decoded_bytes)
}

fn stream_raw_byte_vector<R: Read>(
    reader: &mut BufReader<R>,
    output: &Path,
    expected_bytes: u64,
) -> io::Result<u64> {
    let bytes = usize::load(reader)?;
    let bytes_u64 = usize_to_u64(bytes)?;
    if bytes_u64 != expected_bytes {
        return Err(invalid_data(format!(
            "raw section length {bytes_u64} differs from declared {expected_bytes}"
        )));
    }
    let mut writer = IntegrityFileWriter::create(output)?;
    let copied = io::copy(&mut reader.by_ref().take(bytes_u64), &mut writer)?;
    if copied != bytes_u64 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "raw source section is truncated",
        ));
    }
    writer.finish()?;
    skip_padding(reader, bytes)?;
    Ok(copied)
}

fn stream_packed_byte_vector<R: Read>(
    reader: &mut BufReader<R>,
    output: &Path,
    alphabet: &[u8],
) -> io::Result<u64> {
    let len = usize::load(reader)?;
    let width = usize::load(reader)?;
    if width == 0 || width > 64 {
        return Err(invalid_data("packed sequence width must be in 1..=64"));
    }
    let bit_len = usize::load(reader)?;
    let expected_bits = len
        .checked_mul(width)
        .ok_or_else(|| invalid_data("packed sequence bit length overflow"))?;
    if bit_len != expected_bits {
        return Err(invalid_data("packed sequence bit length mismatch"));
    }
    let word_count = usize::load(reader)?;
    let expected_words = bit_len
        .checked_add(63)
        .ok_or_else(|| invalid_data("packed sequence word count overflow"))?
        / 64;
    if word_count != expected_words {
        return Err(invalid_data("packed sequence word count mismatch"));
    }
    let mut writer = IntegrityFileWriter::create(output)?;
    let mask = if width == 64 {
        u128::from(u64::MAX)
    } else {
        (1_u128 << width) - 1
    };
    let mut buffer = 0_u128;
    let mut buffered_bits = 0_usize;
    let mut words_read = 0_usize;
    let mut output_buffer = vec![0_u8; 64 * 1024];
    let mut output_len = 0_usize;
    for _ in 0..len {
        while buffered_bits < width {
            let word = u64::load(reader)?;
            words_read += 1;
            buffer |= u128::from(word) << buffered_bits;
            buffered_bits += 64;
        }
        let rank = usize::try_from(buffer & mask)
            .map_err(|_| invalid_data("packed sequence alphabet rank overflow"))?;
        let byte = alphabet
            .get(rank)
            .copied()
            .ok_or_else(|| invalid_data("packed sequence alphabet rank is out of bounds"))?;
        output_buffer[output_len] = byte;
        output_len += 1;
        if output_len == output_buffer.len() {
            writer.write_all(&output_buffer)?;
            output_len = 0;
        }
        buffer >>= width;
        buffered_bits -= width;
    }
    if words_read != word_count {
        return Err(invalid_data("packed sequence did not consume all words"));
    }
    writer.write_all(&output_buffer[..output_len])?;
    writer.finish()?;
    usize_to_u64(len)
}

fn skip_padding<R: Read>(reader: &mut BufReader<R>, bytes: usize) -> io::Result<()> {
    let padding = (8 - (bytes % 8)) % 8;
    if padding > 0 {
        let mut buffer = [0_u8; 8];
        reader.read_exact(&mut buffer[..padding])?;
    }
    Ok(())
}

struct DigestingReader<R> {
    inner: R,
    digest: Sha256,
}

impl<R> DigestingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
        }
    }

    fn finish(self) -> [u8; 32] {
        self.digest.finalize().into()
    }
}

impl<R: Read> Read for DigestingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.digest.update(&buffer[..read]);
        Ok(read)
    }
}

struct IntegrityFileWriter {
    data: BufWriter<File>,
    integrity: BufWriter<File>,
    block_hasher: blake3::Hasher,
    block_bytes: usize,
}

impl IntegrityFileWriter {
    fn create(path: &Path) -> io::Result<Self> {
        let data = OpenOptions::new().create_new(true).write(true).open(path)?;
        let integrity_path = block_integrity_path(path)?;
        let integrity = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(integrity_path)?;
        Ok(Self {
            data: BufWriter::with_capacity(1024 * 1024, data),
            integrity: BufWriter::with_capacity(64 * 1024, integrity),
            block_hasher: blake3::Hasher::new(),
            block_bytes: 0,
        })
    }

    fn finish_block(&mut self) -> io::Result<()> {
        let digest = self.block_hasher.finalize();
        self.integrity.write_all(&digest.as_bytes()[..16])?;
        self.block_hasher = blake3::Hasher::new();
        self.block_bytes = 0;
        Ok(())
    }

    fn finish(mut self) -> io::Result<()> {
        if self.block_bytes > 0 {
            self.finish_block()?;
        }
        self.data.flush()?;
        self.integrity.flush()?;
        self.data.get_ref().sync_all()?;
        self.integrity.get_ref().sync_all()
    }
}

impl Write for IntegrityFileWriter {
    fn write(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
        let total = bytes.len();
        while !bytes.is_empty() {
            let remaining = READ_BLOCK_BYTES - self.block_bytes;
            let take = remaining.min(bytes.len());
            let chunk = &bytes[..take];
            self.data.write_all(chunk)?;
            self.block_hasher.update(chunk);
            self.block_bytes += take;
            bytes = &bytes[take..];
            if self.block_bytes == READ_BLOCK_BYTES {
                self.finish_block()?;
            }
        }
        Ok(total)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.data.flush()
    }
}

fn block_integrity_path(path: &Path) -> io::Result<PathBuf> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid_data("source cache component filename is not UTF-8"))?;
    Ok(path.with_file_name(format!("{filename}.blake3-128")))
}

struct CachedBlock {
    bytes: Vec<u8>,
    last_used: u64,
}

struct BlockCacheState {
    file: File,
    blocks: HashMap<u64, CachedBlock>,
    clock: u64,
}

struct BlockCache {
    states: Vec<Mutex<BlockCacheState>>,
    file_bytes: u64,
    block_bytes: usize,
    max_blocks_per_shard: usize,
    block_integrity: Vec<[u8; 16]>,
}

impl BlockCache {
    fn open(path: &Path) -> io::Result<Self> {
        let file_bytes = fs::metadata(path)?.len();
        let integrity_bytes = fs::read(block_integrity_path(path)?)?;
        let block_count = file_bytes.div_ceil(usize_to_u64(READ_BLOCK_BYTES)?);
        if usize_to_u64(integrity_bytes.len())? != block_count.saturating_mul(16) {
            return Err(invalid_data("source cache block-integrity length mismatch"));
        }
        let block_integrity = integrity_bytes
            .chunks_exact(16)
            .map(|bytes| bytes.try_into().expect("fixed integrity chunk"))
            .collect();
        let max_blocks = READ_CACHE_BYTES_PER_FILE / READ_BLOCK_BYTES;
        let max_blocks_per_shard = max_blocks.div_ceil(READ_CACHE_SHARDS);
        let mut states = Vec::with_capacity(READ_CACHE_SHARDS);
        for _ in 0..READ_CACHE_SHARDS {
            states.push(Mutex::new(BlockCacheState {
                file: File::open(path)?,
                blocks: HashMap::new(),
                clock: 0,
            }));
        }
        Ok(Self {
            states,
            file_bytes,
            block_bytes: READ_BLOCK_BYTES,
            max_blocks_per_shard,
            block_integrity,
        })
    }

    fn read(&self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        let mut result = vec![0_u8; length];
        self.read_into(offset, &mut result)?;
        Ok(result)
    }

    fn read_into(&self, offset: u64, result: &mut [u8]) -> io::Result<()> {
        let length_u64 = usize_to_u64(result.len())?;
        let end = offset
            .checked_add(length_u64)
            .ok_or_else(|| invalid_data("cached read interval overflow"))?;
        if end > self.file_bytes {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "cached read extends beyond its source file",
            ));
        }
        let block_bytes_u64 = usize_to_u64(self.block_bytes)?;
        let mut position = offset;
        let mut written = 0_usize;
        while position < end {
            let block_id = position / block_bytes_u64;
            let shard = usize::try_from(block_id % usize_to_u64(self.states.len())?)
                .map_err(|_| invalid_data("source cache shard does not fit in usize"))?;
            let mut state = self.states[shard]
                .lock()
                .map_err(|_| invalid_data("source block cache lock is poisoned"))?;
            if !state.blocks.contains_key(&block_id) {
                if state.blocks.len() >= self.max_blocks_per_shard
                    && let Some(oldest) = state
                        .blocks
                        .iter()
                        .min_by_key(|(_, block)| block.last_used)
                        .map(|(&id, _)| id)
                {
                    state.blocks.remove(&oldest);
                }
                let block_start = block_id
                    .checked_mul(block_bytes_u64)
                    .ok_or_else(|| invalid_data("source cache block offset overflow"))?;
                let remaining = self.file_bytes - block_start;
                let block_len = usize::try_from(remaining.min(block_bytes_u64))
                    .map_err(|_| invalid_data("source cache block length overflow"))?;
                let mut bytes = vec![0_u8; block_len];
                state.file.seek(SeekFrom::Start(block_start))?;
                state.file.read_exact(&mut bytes)?;
                let expected =
                    self.block_integrity
                        .get(usize::try_from(block_id).map_err(|_| {
                            invalid_data("source cache block id does not fit usize")
                        })?)
                        .ok_or_else(|| invalid_data("source cache block integrity is missing"))?;
                let actual = blake3::hash(&bytes);
                if actual.as_bytes()[..16] != *expected {
                    return Err(invalid_data(format!(
                        "source cache block {block_id} integrity mismatch"
                    )));
                }
                state.blocks.insert(
                    block_id,
                    CachedBlock {
                        bytes,
                        last_used: 0,
                    },
                );
            }
            state.clock = state.clock.wrapping_add(1);
            let clock = state.clock;
            let block = state
                .blocks
                .get_mut(&block_id)
                .expect("cache block was inserted");
            block.last_used = clock;
            let within = usize::try_from(position % block_bytes_u64)
                .map_err(|_| invalid_data("source cache within-block offset overflow"))?;
            let available = block.bytes.len() - within;
            let needed = usize::try_from(end - position)
                .map_err(|_| invalid_data("source cache remaining length overflow"))?;
            let take = available.min(needed);
            result[written..written + take].copy_from_slice(&block.bytes[within..within + take]);
            written += take;
            position = position
                .checked_add(usize_to_u64(take)?)
                .ok_or_else(|| invalid_data("source cache position overflow"))?;
        }
        Ok(())
    }
}

fn read_indexed_bytes(
    offsets: &BlockCache,
    data: &BlockCache,
    id: usize,
    count: usize,
) -> io::Result<Option<Vec<u8>>> {
    if id >= count {
        return Ok(None);
    }
    let offset_position = usize_to_u64(id)?
        .checked_mul(8)
        .ok_or_else(|| invalid_data("offset table position overflow"))?;
    let mut pair = [0_u8; 16];
    offsets.read_into(offset_position, &mut pair)?;
    let start = u64::from_le_bytes(pair[..8].try_into().expect("fixed slice"));
    let end = u64::from_le_bytes(pair[8..].try_into().expect("fixed slice"));
    let length = indexed_interval_length(start, end)?;
    Ok(Some(data.read(start, length)?))
}

fn indexed_length(offsets: &BlockCache, id: usize, count: usize) -> io::Result<Option<usize>> {
    if id >= count {
        return Ok(None);
    }
    let offset_position = usize_to_u64(id)?
        .checked_mul(8)
        .ok_or_else(|| invalid_data("offset table position overflow"))?;
    let mut pair = [0_u8; 16];
    offsets.read_into(offset_position, &mut pair)?;
    let start = u64::from_le_bytes(pair[..8].try_into().expect("fixed slice"));
    let end = u64::from_le_bytes(pair[8..].try_into().expect("fixed slice"));
    Ok(Some(indexed_interval_length(start, end)?))
}

fn indexed_interval_length(start: u64, end: u64) -> io::Result<usize> {
    let length = end
        .checked_sub(start)
        .ok_or_else(|| invalid_data("indexed byte interval is reversed"))?;
    usize::try_from(length)
        .map_err(|_| invalid_data("indexed byte interval does not fit in memory"))
}

fn directory_bytes(path: &Path) -> io::Result<u64> {
    fs::read_dir(path)?.try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry?.metadata()?.len())
            .ok_or_else(|| invalid_data("source cache byte count overflow"))
    })
}

fn write_new_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut writer = BufWriter::with_capacity(
        1024 * 1024,
        OpenOptions::new().create_new(true).write(true).open(path)?,
    );
    writer.write_all(bytes)?;
    writer.flush()?;
    writer.get_ref().sync_all()
}

fn write_new_integrity_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut writer = IntegrityFileWriter::create(path)?;
    writer.write_all(bytes)?;
    writer.finish()
}

fn file_sha256(path: &Path) -> io::Result<[u8; 32]> {
    let mut reader = BufReader::with_capacity(1024 * 1024, File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex_digest(digest: &[u8; 32]) -> String {
    digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        })
}

fn parse_hex_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(output)
}

fn available_space_bytes(path: &Path) -> Option<u64> {
    let output = Command::new("df").arg("-Pk").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let available_kib = stdout
        .lines()
        .last()?
        .split_ascii_whitespace()
        .nth(3)?
        .parse::<u64>()
        .ok()?;
    available_kib.checked_mul(1024)
}

fn usize_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_data("value does not fit in u64"))
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn build_path_catalog(
    metadata: &Metadata,
    tags: &Tags,
    path_count: usize,
) -> io::Result<Vec<SourcePathCatalogRecord>> {
    if !metadata.has_path_names()
        || !metadata.has_sample_names()
        || !metadata.has_contig_names()
        || metadata.paths() != path_count
    {
        return Ok(Vec::new());
    }
    let references = tags
        .get(REFERENCE_SAMPLES_KEY)
        .map_or("", String::as_str)
        .split_whitespace()
        .collect::<BTreeSet<_>>();
    let mut result = Vec::with_capacity(path_count);
    for path_id in 0..path_count {
        let name = metadata
            .path(path_id)
            .ok_or_else(|| invalid_data(format!("missing metadata for source path {path_id}")))?;
        let sample = metadata.sample_name(name.sample());
        let contig = metadata.contig_name(name.contig());
        let haplotype = if sample == GENERIC_SAMPLE {
            u64::from(GENERIC_HAPLOTYPE)
        } else {
            usize_to_u64(name.phase())?
        };
        let fragment = usize_to_u64(name.fragment())?;
        let mut canonical_name = format!("{sample}#{haplotype}#{contig}");
        if fragment != 0 {
            write!(&mut canonical_name, "#fragment={fragment}")
                .map_err(|_| invalid_data("cannot format source path name"))?;
        }
        let sense = if sample == GENERIC_SAMPLE {
            1
        } else if references.contains(sample.as_str()) {
            2
        } else {
            3
        };
        result.push(SourcePathCatalogRecord {
            path_id: usize_to_u64(path_id)?,
            canonical_name,
            sample,
            contig,
            haplotype,
            fragment,
            sense,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::{
        ArchiveBuildOptions, ChunkCodec, DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES,
        DEFAULT_MIN_WINDOW_SIZE, FixedArchiveConfig, build_fixed_archive_from_source_with_options,
    };
    use crate::source::{LoadedGbzSource, SourcePathIndex};
    use crate::test_support::tiny_gbz_fixture;
    use gbz::{GBWT, GBZ, MetadataBuilder};

    #[test]
    fn incomplete_path_metadata_does_not_create_biological_labels() {
        let metadata = Metadata::from(MetadataBuilder::new());
        assert!(
            build_path_catalog(&metadata, &Tags::new(), 1)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn disk_source_matches_loaded_records_sequences_and_references() {
        let input = tiny_gbz_fixture();
        let scratch = std::env::temp_dir();
        let (disk, fused_digest) = DiskGbzSource::build_with_digest(&input, &scratch).unwrap();
        assert_eq!(
            hex_digest(&fused_digest),
            "1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788"
        );
        let cache_directory = disk.stats().cache_directory.clone();
        let graph: GBZ = simple_sds::serialize::load_from(&input).unwrap();
        let loaded = LoadedGbzSource::new(&graph);
        for node_id in [1, 2, 10, 100] {
            assert_eq!(
                disk.sequence_len(node_id).unwrap(),
                loaded.sequence_len(node_id).unwrap()
            );
            assert_eq!(
                disk.sequence(node_id).unwrap(),
                loaded.sequence(node_id).unwrap()
            );
            for orientation in [Orientation::Forward, Orientation::Reverse] {
                let handle = gbz::support::encode_node(node_id, orientation);
                assert_eq!(
                    disk.packed_record(handle).unwrap(),
                    loaded.packed_record(handle).unwrap()
                );
            }
        }
        let disk_index = SourcePathIndex::new(&disk, 1_000).unwrap();
        let loaded_index = SourcePathIndex::new(&loaded, 1_000).unwrap();
        assert_eq!(
            disk_index.references(None, None),
            loaded_index.references(None, None)
        );
        drop(disk);
        assert!(!cache_directory.exists());
    }

    #[test]
    fn bounded_disk_locate_matches_brute_force_sequence_enumeration() {
        let input = tiny_gbz_fixture();
        let disk = DiskGbzSource::build(&input, &std::env::temp_dir()).unwrap();
        let graph: GBZ = simple_sds::serialize::load_from(&input).unwrap();
        let index: &GBWT = graph.as_ref();
        let mut positions = Vec::new();
        let mut expected = Vec::new();
        for sequence_id in 0..index.sequences() {
            let mut position = index.start(sequence_id);
            while let Some(value) = position {
                positions.push(value);
                let (path_id, orientation) = gbz::support::decode_path(sequence_id);
                expected.push(SourceLocatedPosition {
                    sequence_id: usize_to_u64(sequence_id).unwrap(),
                    path_id: usize_to_u64(path_id).unwrap(),
                    reversed: orientation == Orientation::Reverse,
                    lf_steps: 0,
                });
                position = index.forward(value);
            }
        }
        let located = disk.locate_batch(&positions, 1_000_000).unwrap();
        assert_eq!(located.len(), expected.len());
        for (actual, expected) in located.iter().zip(&expected) {
            assert_eq!(actual.sequence_id, expected.sequence_id);
            assert_eq!(actual.path_id, expected.path_id);
            assert_eq!(actual.reversed, expected.reversed);
            assert!(actual.lf_steps <= 1_000_000);
        }
        assert!(located.iter().any(|item| item.lf_steps > 0));
    }

    #[test]
    fn explicit_reference_haplotype_selects_real_untagged_paths() {
        let input = tiny_gbz_fixture();
        let graph: GBZ = simple_sds::serialize::load_from(&input).unwrap();
        let metadata = graph.metadata().unwrap();
        let tagged: BTreeSet<_> = graph.reference_sample_ids(true).into_iter().collect();
        let (sample, haplotype) = metadata
            .path_iter()
            .find_map(|path| {
                (!tagged.contains(&path.sample()))
                    .then(|| (metadata.sample_name(path.sample()), path.phase()))
            })
            .expect("fixture should contain a non-reference sample path");

        let disk = DiskGbzSource::build_for_reference_haplotype(
            &input,
            &std::env::temp_dir(),
            &sample,
            haplotype,
        )
        .unwrap();
        let seeds = disk.reference_seeds().unwrap();
        assert!(!seeds.is_empty());
        assert!(
            seeds
                .iter()
                .all(|seed| { seed.name.sample == sample && seed.name.haplotype == haplotype })
        );
        let index = SourcePathIndex::new(&disk, 1_000).unwrap();
        assert!(
            index
                .references(Some(&sample), None)
                .iter()
                .all(|path| { path.name.sample == sample && path.name.haplotype == haplotype })
        );
    }

    #[test]
    fn explicit_reference_haplotype_fails_closed_when_missing() {
        let input = tiny_gbz_fixture();
        let error = DiskGbzSource::build_for_reference_haplotype(
            &input,
            &std::env::temp_dir(),
            "missing-sample",
            0,
        )
        .err()
        .expect("missing reference should fail");
        assert!(error.to_string().contains("missing-sample"));
    }

    #[test]
    fn persistent_cache_is_atomic_reusable_and_integrity_bound() {
        let input = tiny_gbz_fixture();
        let target = simple_sds::serialize::temp_file_name("persistent-gbz-cache");
        let persistent = build_persistent_source_cache(&input, &target, false).unwrap();
        assert!(target.is_dir());
        assert_eq!(
            persistent.source.access_mode(),
            "persistent-disk-backed-gbz"
        );
        assert_eq!(persistent.path_index.interval(), SOURCE_PATH_INDEX_INTERVAL);
        drop(persistent);

        let reopened = open_persistent_source_cache(&input, &target).unwrap();
        assert_eq!(reopened.manifest.source_gbz_bytes, 73_920);
        assert!(reopened.manifest.da_samples_bytes.is_some());
        assert_eq!(reopened.manifest.path_catalog_records, 169);
        assert!(reopened.source.da_samples.is_some());
        assert_eq!(reopened.source.path_catalog().unwrap().unwrap().len(), 169);
        assert!(reopened.source.sequence(1).unwrap().is_some());
        drop(reopened);

        let index_path = target.join(SOURCE_PATH_INDEX_FILE);
        let mut index = fs::read(&index_path).unwrap();
        index[0] ^= 1;
        fs::write(&index_path, &index).unwrap();
        assert!(inspect_persistent_source_cache(&target).is_err());
        let rebuilt = build_persistent_source_cache(&input, &target, true).unwrap();
        drop(rebuilt);
        let records_path = target.join("records.data");
        let mut records = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&records_path)
            .unwrap();
        let mut byte = [0_u8; 1];
        records.read_exact(&mut byte).unwrap();
        byte[0] ^= 1;
        records.seek(SeekFrom::Start(0)).unwrap();
        records.write_all(&byte).unwrap();
        records.sync_all().unwrap();
        let corrupt = open_persistent_source_cache(&input, &target).unwrap();
        assert!(corrupt.source.record_by_id(0).is_err());
        drop(corrupt);
        let rebuilt = build_persistent_source_cache(&input, &target, true).unwrap();
        drop(rebuilt);
        let manifest = prune_persistent_source_cache(&target).unwrap();
        assert_eq!(manifest.cache_format_version, SOURCE_CACHE_VERSION);
        assert!(!target.exists());
    }

    #[test]
    fn disk_source_reads_current_zstd_gbz_sections() {
        let legacy = tiny_gbz_fixture();
        let graph: GBZ = simple_sds::serialize::load_from(&legacy).unwrap();
        let current = simple_sds::serialize::temp_file_name("current-zstd-gbz");
        simple_sds::serialize::serialize_to(&graph, &current).unwrap();
        let disk = DiskGbzSource::build(&current, &std::env::temp_dir()).unwrap();
        let cache_directory = disk.stats().cache_directory.clone();
        let loaded = LoadedGbzSource::new(&graph);
        for node_id in [1, 2, 10, 100] {
            assert_eq!(
                disk.sequence_len(node_id).unwrap(),
                loaded.sequence_len(node_id).unwrap()
            );
            assert_eq!(
                disk.sequence(node_id).unwrap(),
                loaded.sequence(node_id).unwrap()
            );
            for orientation in [Orientation::Forward, Orientation::Reverse] {
                let handle = gbz::support::encode_node(node_id, orientation);
                assert_eq!(
                    disk.packed_record(handle).unwrap(),
                    loaded.packed_record(handle).unwrap()
                );
            }
        }
        drop(disk);
        assert!(!cache_directory.exists());
        fs::remove_file(current).unwrap();
    }

    #[test]
    fn disk_source_archive_is_byte_identical_to_loaded_source() {
        let input = tiny_gbz_fixture();
        let disk = DiskGbzSource::build(&input, &std::env::temp_dir()).unwrap();
        let graph: GBZ = simple_sds::serialize::load_from(&input).unwrap();
        let loaded = LoadedGbzSource::new(&graph);
        let disk_index = SourcePathIndex::new(&disk, 1_000).unwrap();
        let loaded_index = SourcePathIndex::new(&loaded, 1_000).unwrap();
        let disk_output = simple_sds::serialize::temp_file_name("disk-source-archive");
        let loaded_output = simple_sds::serialize::temp_file_name("loaded-source-archive");
        let config = FixedArchiveConfig {
            experiment_id: "source-byte-identity".into(),
            window_size: 16_384,
            codec: ChunkCodec::Zstd3,
            deduplicate_chunks: false,
            max_uncompressed_chunk_bytes: DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES,
            min_window_size: DEFAULT_MIN_WINDOW_SIZE,
        };
        let options = ArchiveBuildOptions {
            threads: 1,
            ..ArchiveBuildOptions::default()
        };
        let source_bytes = fs::metadata(&input).unwrap().len();
        build_fixed_archive_from_source_with_options(
            &disk,
            &disk_index,
            source_bytes,
            &disk_output,
            &config,
            &options,
        )
        .unwrap();
        build_fixed_archive_from_source_with_options(
            &loaded,
            &loaded_index,
            source_bytes,
            &loaded_output,
            &config,
            &options,
        )
        .unwrap();
        assert_eq!(
            fs::read(&disk_output).unwrap(),
            fs::read(&loaded_output).unwrap()
        );
        fs::remove_file(disk_output).unwrap();
        fs::remove_file(loaded_output).unwrap();
    }
}
