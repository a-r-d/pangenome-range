use crate::source::{PangenomeSource, SourceReferenceSeed};
use gbz::bwt::Record;
use gbz::headers::{GBWTPayload, GBZPayload, Header, SequencesPayload};
use gbz::{FullPathName, GENERIC_SAMPLE, Metadata, Orientation, REFERENCE_SAMPLES_KEY, Tags};
use simple_sds::ops::{BitVec, Select};
use simple_sds::serialize::Serialize;
use simple_sds::sparse_vector::SparseVector;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CACHE_ID: AtomicU64 = AtomicU64::new(0);
const READ_BLOCK_BYTES: usize = 256 * 1024;
const READ_CACHE_BYTES_PER_FILE: usize = 16 * 1024 * 1024;
const READ_CACHE_SHARDS: usize = 16;

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
    stats: DiskSourceStats,
    _cache_guard: CacheDirectoryGuard,
}

impl DiskGbzSource {
    /// Builds an ephemeral disk cache by streaming the compressed GBZ bodies.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported/corrupt GBZ input, insufficient disk,
    /// or cache I/O failures.
    pub fn build(input: &Path, scratch_parent: &Path) -> io::Result<Self> {
        fs::create_dir_all(scratch_parent)?;
        let cache_directory = create_cache_directory(scratch_parent)?;
        match Self::build_in(input, &cache_directory) {
            Ok(source) => Ok(source),
            Err(error) => {
                let _ = fs::remove_dir_all(&cache_directory);
                Err(error)
            }
        }
    }

    fn build_in(input: &Path, cache_directory: &Path) -> io::Result<Self> {
        let mut reader = BufReader::with_capacity(1024 * 1024, File::open(input)?);

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

        skip_serialized_u64_vector(&mut reader)?;
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

        let alphabet_offset = gbwt_header.payload().offset;
        let first_node = alphabet_offset
            .checked_add(1)
            .ok_or_else(|| invalid_data("GBWT first-node overflow"))?;
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
            stats: DiskSourceStats {
                cache_directory: cache_directory.to_path_buf(),
                record_count: usize_to_u64(record_count)?,
                record_bytes,
                sequence_count: usize_to_u64(sequence_count)?,
                sequence_bytes,
                cache_bytes: directory_bytes(cache_directory)?,
                memory_cache_limit_bytes: usize_to_u64(4 * READ_CACHE_BYTES_PER_FILE)?,
            },
            _cache_guard: CacheDirectoryGuard {
                path: cache_directory.to_path_buf(),
            },
        };
        source.reference_seeds = source.build_reference_seeds(&metadata, &gbwt_tags)?;
        Ok(source)
    }

    #[must_use]
    pub fn stats(&self) -> &DiskSourceStats {
        &self.stats
    }

    fn build_reference_seeds(
        &self,
        metadata: &Metadata,
        gbwt_tags: &Tags,
    ) -> io::Result<Vec<SourceReferenceSeed>> {
        let endmarker_bytes = self
            .record_by_id(0)?
            .ok_or_else(|| invalid_data("GBWT endmarker record is empty"))?;
        let endmarker = Record::new(0, &endmarker_bytes)
            .ok_or_else(|| invalid_data("GBWT endmarker record is invalid"))?
            .decompress();
        let mut reference_names = BTreeSet::new();
        if let Some(names) = gbwt_tags.get(REFERENCE_SAMPLES_KEY) {
            reference_names.extend(names.split(' ').map(str::to_owned));
        }
        reference_names.insert(GENERIC_SAMPLE.to_owned());
        let mut seeds = Vec::new();
        for (path_id, path) in metadata.path_iter().enumerate() {
            let sample = metadata.sample_name(path.sample());
            if !reference_names.contains(&sample) {
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
}

impl PangenomeSource for DiskGbzSource {
    fn access_mode(&self) -> &'static str {
        "disk-backed-gbz"
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
}

struct CacheDirectoryGuard {
    path: PathBuf,
}

impl Drop for CacheDirectoryGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
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
    let mut writer = BufWriter::with_capacity(
        1024 * 1024,
        OpenOptions::new().create_new(true).write(true).open(path)?,
    );
    for (_, offset) in index.one_iter() {
        writer.write_all(&usize_to_u64(offset)?.to_le_bytes())?;
    }
    writer.write_all(&usize_to_u64(end)?.to_le_bytes())?;
    writer.flush()?;
    writer.get_ref().sync_all()
}

fn stream_compressed_vector(
    reader: &mut BufReader<File>,
    output: &Path,
    expected_bytes: u64,
) -> io::Result<u64> {
    let compressed_bytes = usize::load(reader)?;
    let limited = reader.by_ref().take(usize_to_u64(compressed_bytes)?);
    let mut decoder = zstd::stream::read::Decoder::new(limited)?;
    let mut writer = BufWriter::with_capacity(
        1024 * 1024,
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(output)?,
    );
    let limit = expected_bytes
        .checked_add(1)
        .ok_or_else(|| invalid_data("decompressed section length overflow"))?;
    let decoded_bytes = io::copy(&mut decoder.by_ref().take(limit), &mut writer)?;
    drop(decoder.finish());
    writer.flush()?;
    writer.get_ref().sync_all()?;
    if decoded_bytes != expected_bytes {
        return Err(invalid_data(format!(
            "decompressed section length {decoded_bytes} differs from declared {expected_bytes}"
        )));
    }
    skip_padding(reader, compressed_bytes)?;
    Ok(decoded_bytes)
}

fn stream_raw_byte_vector(
    reader: &mut BufReader<File>,
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
    let mut writer = BufWriter::with_capacity(
        1024 * 1024,
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(output)?,
    );
    let copied = io::copy(&mut reader.by_ref().take(bytes_u64), &mut writer)?;
    if copied != bytes_u64 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "raw source section is truncated",
        ));
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    skip_padding(reader, bytes)?;
    Ok(copied)
}

fn stream_packed_byte_vector(
    reader: &mut BufReader<File>,
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
    let mut writer = BufWriter::with_capacity(
        1024 * 1024,
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(output)?,
    );
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
    writer.flush()?;
    writer.get_ref().sync_all()?;
    usize_to_u64(len)
}

fn skip_serialized_u64_vector(reader: &mut BufReader<File>) -> io::Result<()> {
    let elements = usize::load(reader)?;
    let bytes = elements
        .checked_mul(std::mem::size_of::<u64>())
        .ok_or_else(|| invalid_data("serialized u64 vector length overflow"))?;
    reader.seek(SeekFrom::Current(i64::try_from(bytes).map_err(|_| {
        invalid_data("serialized vector is too large to seek")
    })?))?;
    Ok(())
}

fn skip_padding(reader: &mut BufReader<File>, bytes: usize) -> io::Result<()> {
    let padding = (8 - (bytes % 8)) % 8;
    if padding > 0 {
        let mut buffer = [0_u8; 8];
        reader.read_exact(&mut buffer[..padding])?;
    }
    Ok(())
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
}

impl BlockCache {
    fn open(path: &Path) -> io::Result<Self> {
        let file_bytes = fs::metadata(path)?.len();
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

fn usize_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| invalid_data("value does not fit in u64"))
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::{
        ArchiveBuildOptions, ChunkCodec, DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES,
        DEFAULT_MIN_WINDOW_SIZE, FixedArchiveConfig, build_fixed_archive_from_source_with_options,
    };
    use crate::source::{LoadedGbzSource, SourcePathIndex};
    use gbz::GBZ;

    #[test]
    fn disk_source_matches_loaded_records_sequences_and_references() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/micb-kir3dl1.gbz");
        let scratch = std::env::temp_dir();
        let disk = DiskGbzSource::build(&input, &scratch).unwrap();
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
    fn disk_source_reads_current_zstd_gbz_sections() {
        let legacy = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/micb-kir3dl1.gbz");
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
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/micb-kir3dl1.gbz");
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
