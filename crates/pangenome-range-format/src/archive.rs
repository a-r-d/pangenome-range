use crate::binary::{
    BinaryReader, count_bounded_by_bytes, invalid_data, put_string, put_u32, put_u64, u32_to_usize,
    u64_to_usize, usize_to_u32, usize_to_u64,
};
use crate::{ExtensionEntry, RangeSource, decode_extension_directory};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io;

pub const ARCHIVE_MAGIC: &[u8; 8] = b"PNGRNG01";
pub const ARCHIVE_VERSION: u32 = 1;
pub const HEADER_LEN: usize = 64;
pub const DIRECTORY_PAGE_BYTES: usize = 4 * 1024;
pub const DIRECTORY_PAGE_HEADER_BYTES: usize = 16;
pub const DIRECTORY_ENTRY_BYTES: usize = 7 * std::mem::size_of::<u64>();
pub const DIRECTORY_ENTRIES_PER_PAGE: usize =
    (DIRECTORY_PAGE_BYTES - DIRECTORY_PAGE_HEADER_BYTES) / DIRECTORY_ENTRY_BYTES;
pub const DIRECTORY_BUCKET_WINDOWS: u64 = 32;
pub const BOOTSTRAP_LEN: usize = 16 * 1024;
pub const MAX_ROOT_BYTES: u64 = 16 * 1024 * 1024;

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
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Zstd1 => "zstd-1",
            Self::Zstd3 => "zstd-3",
            Self::Zstd6 => "zstd-6",
        }
    }

    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Zstd1 => 1,
            Self::Zstd3 => 3,
            Self::Zstd6 => 6,
        }
    }

    /// Decodes the normative one-byte codec identifier.
    ///
    /// # Errors
    ///
    /// Returns an error for any codec value not assigned by format v1.
    pub fn from_code(code: u8) -> io::Result<Self> {
        match code {
            0 => Ok(Self::None),
            1 => Ok(Self::Zstd1),
            3 => Ok(Self::Zstd3),
            6 => Ok(Self::Zstd6),
            _ => Err(invalid_data(format!("unknown chunk codec {code}"))),
        }
    }

    const fn level(self) -> Option<i32> {
        match self {
            Self::None => None,
            Self::Zstd1 => Some(1),
            Self::Zstd3 => Some(3),
            Self::Zstd6 => Some(6),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveHeader {
    pub version: u32,
    pub root_len: u64,
    pub entry_count: u64,
    pub data_offset: u64,
    pub extension_directory_offset: u64,
    pub extension_directory_len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub start: u64,
    pub end: u64,
    pub offset: u64,
    pub compressed_len: u64,
    pub uncompressed_len: u64,
    pub integrity: [u8; 16],
    pub codec: ChunkCodec,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceManifest {
    pub sample: String,
    pub contig: String,
    pub start: u64,
    pub end: u64,
    pub grid_start: u64,
    pub window_size: u64,
    pub bucket_span: u64,
    pub first_page_offset: u64,
    pub page_count: u64,
    pub entry_count: u64,
    pub codec: ChunkCodec,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootIndex {
    pub logical_bytes: u64,
    pub manifests: Vec<ReferenceManifest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bootstrap {
    pub bytes: Vec<u8>,
    pub header: ArchiveHeader,
    pub root: RootIndex,
    pub extensions: Vec<ExtensionEntry>,
    pub dependency_rounds: u64,
}

#[must_use]
pub fn encode_header(root_len: u64, entry_count: u64, data_offset: u64) -> [u8; HEADER_LEN] {
    encode_header_with_extensions(root_len, entry_count, data_offset, 0, 0)
}

#[must_use]
pub fn encode_header_with_extensions(
    root_len: u64,
    entry_count: u64,
    data_offset: u64,
    extension_directory_offset: u64,
    extension_directory_len: u64,
) -> [u8; HEADER_LEN] {
    let mut output = [0_u8; HEADER_LEN];
    output[..8].copy_from_slice(ARCHIVE_MAGIC);
    output[8..12].copy_from_slice(&ARCHIVE_VERSION.to_le_bytes());
    output[12..16].copy_from_slice(&64_u32.to_le_bytes());
    output[16..24].copy_from_slice(&64_u64.to_le_bytes());
    output[24..32].copy_from_slice(&root_len.to_le_bytes());
    output[32..40].copy_from_slice(&entry_count.to_le_bytes());
    output[40..48].copy_from_slice(&data_offset.to_le_bytes());
    output[48..56].copy_from_slice(&extension_directory_offset.to_le_bytes());
    output[56..64].copy_from_slice(&extension_directory_len.to_le_bytes());
    output
}

/// Decodes and validates the exact 64-byte archive header.
///
/// # Errors
///
/// Returns an error for a wrong length, unsupported identity/version, invalid
/// extension pointer, or overflowing root range.
pub fn decode_header(bytes: &[u8]) -> io::Result<ArchiveHeader> {
    if bytes.len() != HEADER_LEN {
        return Err(invalid_data("invalid archive header"));
    }
    let mut reader = BinaryReader::new(bytes);
    let magic = reader.take(8)?;
    let version = reader.u32()?;
    let header_len = reader.u32()?;
    let root_offset = reader.u64()?;
    if magic != ARCHIVE_MAGIC
        || version != ARCHIVE_VERSION
        || usize::try_from(header_len).ok() != Some(HEADER_LEN)
        || root_offset != 64
    {
        return Err(invalid_data(format!(
            "unsupported archive version {version}, header length {header_len}, or root offset {root_offset}"
        )));
    }
    let root_len = reader.u64()?;
    let entry_count = reader.u64()?;
    let data_offset = reader.u64()?;
    let extension_directory_offset = reader.u64()?;
    let extension_directory_len = reader.u64()?;
    reader.finish()?;
    if (extension_directory_offset == 0) != (extension_directory_len == 0)
        || extension_directory_len > crate::MAX_EXTENSION_DIRECTORY_BYTES
    {
        return Err(invalid_data("invalid extension directory pointer"));
    }
    if extension_directory_len != 0
        && extension_directory_offset
            != 64_u64
                .checked_add(root_len)
                .ok_or_else(|| invalid_data("root index end overflow"))?
    {
        return Err(invalid_data("invalid extension directory pointer"));
    }
    Ok(ArchiveHeader {
        version,
        root_len,
        entry_count,
        data_offset,
        extension_directory_offset,
        extension_directory_len,
    })
}

impl ArchiveHeader {
    /// Returns the first directory-page offset implied by this header.
    ///
    /// # Errors
    ///
    /// Returns an error if the root or extension directory end overflows.
    pub fn directory_start(&self) -> io::Result<u64> {
        if self.extension_directory_len == 0 {
            usize_to_u64(HEADER_LEN)?
                .checked_add(self.root_len)
                .ok_or_else(|| invalid_data("root index end overflow"))
        } else {
            self.extension_directory_offset
                .checked_add(self.extension_directory_len)
                .ok_or_else(|| invalid_data("extension directory end overflow"))
        }
    }
}

/// Encodes the exact v1 root manifest sequence.
///
/// # Errors
///
/// Returns an error when a manifest count/string length cannot fit the v1
/// unsigned length fields.
pub fn encode_root_index(manifests: &[ReferenceManifest]) -> io::Result<Vec<u8>> {
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

/// Decodes and validates a complete v1 root manifest sequence.
///
/// # Errors
///
/// Returns an error for malformed, truncated, unordered, duplicate,
/// overflowing, or header-inconsistent root data.
pub fn decode_root_index(bytes: &[u8], header: ArchiveHeader) -> io::Result<RootIndex> {
    let mut reader = BinaryReader::new(bytes);
    let count =
        count_bounded_by_bytes(reader.u64()?, reader.remaining(), 88, "reference manifests")?;
    let mut manifests = Vec::with_capacity(count);
    let root_end = usize_to_u64(HEADER_LEN)?
        .checked_add(header.root_len)
        .ok_or_else(|| invalid_data("root index end overflow"))?;
    let mut previous_page_end = header.directory_start()?;
    let mut total_entries = 0_u64;
    let mut identities = BTreeSet::new();
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
            return Err(invalid_data(
                "reference manifest reserved bytes are nonzero",
            ));
        }
        if manifest.sample.is_empty() || manifest.contig.is_empty() {
            return Err(invalid_data("reference manifest identity is empty"));
        }
        if !identities.insert((
            manifest.sample.clone(),
            manifest.contig.clone(),
            manifest.start,
            manifest.end,
        )) {
            return Err(invalid_data("duplicate reference manifest interval"));
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
            return Err(invalid_data("invalid arithmetic reference manifest"));
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
        )));
    }
    Ok(RootIndex {
        logical_bytes: root_end,
        manifests,
    })
}

/// Encodes one fixed arithmetic directory page.
///
/// # Errors
///
/// Returns an error when the page exceeds capacity or an entry has an invalid
/// core interval or integer representation.
pub fn encode_directory_page(
    entries: &[ArchiveEntry],
    bucket_start: u64,
) -> io::Result<[u8; DIRECTORY_PAGE_BYTES]> {
    if entries.len() > DIRECTORY_ENTRIES_PER_PAGE {
        return Err(invalid_data(format!(
            "directory bucket contains {} adaptive chunks; fixed page capacity is {DIRECTORY_ENTRIES_PER_PAGE}",
            entries.len()
        )));
    }
    let mut encoded = Vec::with_capacity(DIRECTORY_PAGE_BYTES);
    put_u32(&mut encoded, usize_to_u32(entries.len())?);
    put_u32(&mut encoded, usize_to_u32(DIRECTORY_ENTRY_BYTES)?);
    put_u64(&mut encoded, bucket_start);
    for entry in entries {
        if entry.start >= entry.end || entry.start < bucket_start {
            return Err(invalid_data("directory entry is outside its bucket"));
        }
        put_u64(&mut encoded, entry.start);
        put_u64(&mut encoded, entry.end);
        put_u64(&mut encoded, entry.offset);
        put_u64(&mut encoded, entry.compressed_len);
        put_u64(&mut encoded, entry.uncompressed_len);
        encoded.extend_from_slice(&entry.integrity);
    }
    let mut output = [0_u8; DIRECTORY_PAGE_BYTES];
    output[..encoded.len()].copy_from_slice(&encoded);
    Ok(output)
}

/// Decodes and validates one fixed arithmetic directory page.
///
/// # Errors
///
/// Returns an error for a wrong page size, invalid bucket arithmetic,
/// malformed entries, bad ordering, or nonzero padding.
pub fn decode_directory_page(
    bytes: &[u8],
    manifest: &ReferenceManifest,
    bucket_index: u64,
) -> io::Result<Vec<ArchiveEntry>> {
    if bytes.len() != DIRECTORY_PAGE_BYTES {
        return Err(invalid_data("directory page has the wrong fixed size"));
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
        return Err(invalid_data("invalid fixed directory page header"));
    }
    let bucket_end = bucket_start
        .checked_add(manifest.bucket_span)
        .ok_or_else(|| invalid_data("directory bucket end overflow"))?;
    let mut entries = Vec::with_capacity(count);
    let mut previous_key = None;
    for _ in 0..count {
        let start = reader.u64()?;
        let end = reader.u64()?;
        let offset = reader.u64()?;
        let compressed_len = reader.u64()?;
        let uncompressed_len = reader.u64()?;
        let integrity: [u8; 16] = reader
            .take(16)?
            .try_into()
            .map_err(|_| invalid_data("invalid directory integrity length"))?;
        if start >= end
            || start < bucket_start
            || end > bucket_end.min(manifest.end)
            || compressed_len == 0
            || uncompressed_len == 0
            || previous_key.is_some_and(|previous| {
                (
                    start,
                    end,
                    offset,
                    compressed_len,
                    uncompressed_len,
                    integrity,
                ) < previous
            })
        {
            return Err(invalid_data("invalid fixed directory entry"));
        }
        previous_key = Some((
            start,
            end,
            offset,
            compressed_len,
            uncompressed_len,
            integrity,
        ));
        entries.push(ArchiveEntry {
            start,
            end,
            offset,
            compressed_len,
            uncompressed_len,
            integrity,
            codec: manifest.codec,
        });
    }
    let padding_start = DIRECTORY_PAGE_HEADER_BYTES
        .checked_add(
            count
                .checked_mul(DIRECTORY_ENTRY_BYTES)
                .ok_or_else(|| invalid_data("directory page size overflow"))?,
        )
        .ok_or_else(|| invalid_data("directory page size overflow"))?;
    if bytes[padding_start..].iter().any(|&value| value != 0) {
        return Err(invalid_data("directory page padding is nonzero"));
    }
    Ok(entries)
}

/// Computes the absolute byte offset of one arithmetic directory page.
///
/// # Errors
///
/// Returns an error when the bucket is outside the manifest or offset
/// arithmetic overflows.
pub fn directory_page_offset(manifest: &ReferenceManifest, bucket_index: u64) -> io::Result<u64> {
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

/// Reads and validates the bounded archive bootstrap metadata.
///
/// # Errors
///
/// Returns an error for source I/O failure, invalid header/root/extensions, or
/// archive ranges that violate configured limits.
pub fn bootstrap(source: &impl RangeSource) -> io::Result<Bootstrap> {
    let source_len = source.len()?;
    let first_len = source_len.min(usize_to_u64(BOOTSTRAP_LEN)?);
    let mut bytes = source.read_range(0, u64_to_usize(first_len)?)?;
    if bytes.len() < HEADER_LEN {
        return Err(invalid_data("archive is shorter than its header"));
    }
    let header = decode_header(&bytes[..HEADER_LEN])?;
    let root_end = usize_to_u64(HEADER_LEN)?
        .checked_add(header.root_len)
        .ok_or_else(|| invalid_data("root index end overflow"))?;
    let metadata_end = header.directory_start()?;
    if header.root_len > MAX_ROOT_BYTES
        || root_end > metadata_end
        || metadata_end > header.data_offset
        || header.data_offset > source_len
    {
        return Err(invalid_data("archive directory offsets are inconsistent"));
    }
    let mut dependency_rounds = 1;
    if metadata_end > first_len {
        let remainder = source.read_range(first_len, u64_to_usize(metadata_end - first_len)?)?;
        bytes.extend_from_slice(&remainder);
        dependency_rounds += 1;
    }
    let root = decode_root_index(&bytes[HEADER_LEN..u64_to_usize(root_end)?], header)?;
    let extensions = if header.extension_directory_len == 0 {
        Vec::new()
    } else {
        decode_extension_directory(
            &bytes[u64_to_usize(header.extension_directory_offset)?..u64_to_usize(metadata_end)?],
            header.data_offset,
            source_len,
        )?
    };
    if extensions.iter().any(|entry| entry.required) {
        return Err(invalid_data(
            "archive contains an unknown required extension",
        ));
    }
    Ok(Bootstrap {
        bytes,
        header,
        root,
        extensions,
        dependency_rounds,
    })
}

/// Encodes one stored payload with the selected deterministic codec.
///
/// # Errors
///
/// Returns an error when zstd compression fails.
pub fn compress(codec: ChunkCodec, bytes: &[u8]) -> io::Result<Vec<u8>> {
    if let Some(level) = codec.level() {
        zstd::bulk::compress(bytes, level)
    } else {
        Ok(bytes.to_vec())
    }
}

/// Decodes exactly one stored payload to its declared length.
///
/// # Errors
///
/// Returns an error for invalid framing, dictionaries, concatenated/trailing
/// bytes, unsupported sizes, decompressor failure, or length mismatch.
pub fn decompress(codec: ChunkCodec, bytes: &[u8], expected_len: u64) -> io::Result<Vec<u8>> {
    let result = if codec.level().is_some() {
        let declared_len = zstd::zstd_safe::get_frame_content_size(bytes)
            .map_err(|error| invalid_data(format!("invalid zstd frame header: {error:?}")))?
            .ok_or_else(|| invalid_data("zstd frame omits its content size"))?;
        if declared_len != expected_len {
            return Err(invalid_data(format!(
                "zstd frame declares {declared_len} bytes, expected {expected_len}"
            )));
        }
        if zstd::zstd_safe::get_dict_id_from_frame(bytes).is_some() {
            return Err(invalid_data("zstd dictionaries are not supported"));
        }
        let frame_len = zstd::zstd_safe::find_frame_compressed_size(bytes)
            .map_err(|error| invalid_data(format!("invalid zstd frame: {error}")))?;
        if frame_len != bytes.len() {
            return Err(invalid_data("zstd payload must contain exactly one frame"));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> ReferenceManifest {
        ReferenceManifest {
            sample: "GRCh38".into(),
            contig: "chr1".into(),
            start: 100,
            end: 200,
            grid_start: 0,
            window_size: 10,
            bucket_span: 320,
            first_page_offset: 170,
            page_count: 1,
            entry_count: 1,
            codec: ChunkCodec::Zstd3,
        }
    }

    #[test]
    fn archive_metadata_round_trip_is_byte_stable() {
        let mut manifest = manifest();
        let root = encode_root_index(&[manifest.clone()]).unwrap();
        manifest.first_page_offset = u64::try_from(HEADER_LEN + root.len()).unwrap();
        let root = encode_root_index(&[manifest.clone()]).unwrap();
        let data_offset = manifest.first_page_offset + u64::try_from(DIRECTORY_PAGE_BYTES).unwrap();
        let header = encode_header(u64::try_from(root.len()).unwrap(), 1, data_offset);
        let decoded_header = decode_header(&header).unwrap();
        assert_eq!(decoded_header.root_len, u64::try_from(root.len()).unwrap());
        assert_eq!(
            decode_root_index(&root, decoded_header).unwrap().manifests,
            vec![manifest]
        );
    }

    #[test]
    fn directory_capacity_is_exactly_72_entries_after_integrity_digest() {
        assert_eq!(DIRECTORY_ENTRIES_PER_PAGE, 72);
        assert_eq!(
            (DIRECTORY_PAGE_BYTES - DIRECTORY_PAGE_HEADER_BYTES) / 40,
            102
        );
    }

    #[test]
    fn a_73rd_directory_entry_fails_closed() {
        let entries = (0_u64..=72)
            .map(|start| ArchiveEntry {
                start,
                end: start + 1,
                offset: 10_000 + start,
                compressed_len: 1,
                uncompressed_len: 1,
                integrity: [0; 16],
                codec: ChunkCodec::None,
            })
            .collect::<Vec<_>>();
        assert!(encode_directory_page(&entries, 0).is_err());
    }

    #[test]
    fn obsolete_102_and_103_entry_boundaries_fail_after_format_change() {
        for count in [102_u64, 103] {
            let entries = (0..count)
                .map(|start| ArchiveEntry {
                    start,
                    end: start + 1,
                    offset: 10_000 + start,
                    compressed_len: 1,
                    uncompressed_len: 1,
                    integrity: [0; 16],
                    codec: ChunkCodec::None,
                })
                .collect::<Vec<_>>();
            assert!(encode_directory_page(&entries, 0).is_err());
        }
    }

    #[test]
    fn directory_entries_must_be_lexicographically_ordered() {
        let manifest = manifest();
        let entries = [
            ArchiveEntry {
                start: 2,
                end: 4,
                offset: 10_001,
                compressed_len: 1,
                uncompressed_len: 1,
                integrity: [0; 16],
                codec: ChunkCodec::None,
            },
            ArchiveEntry {
                start: 1,
                end: 3,
                offset: 10_000,
                compressed_len: 1,
                uncompressed_len: 1,
                integrity: [0; 16],
                codec: ChunkCodec::None,
            },
        ];
        let page = encode_directory_page(&entries, 0).unwrap();
        assert!(decode_directory_page(&page, &manifest, 0).is_err());
    }

    #[test]
    fn zstd_requires_one_sized_frame_with_no_trailing_bytes() {
        let raw = b"one exact frame";
        let compressed = compress(ChunkCodec::Zstd3, raw).unwrap();
        assert_eq!(
            decompress(ChunkCodec::Zstd3, &compressed, raw.len() as u64).unwrap(),
            raw
        );
        let mut trailing = compressed.clone();
        trailing.push(0);
        assert!(decompress(ChunkCodec::Zstd3, &trailing, raw.len() as u64).is_err());
        assert!(decompress(ChunkCodec::Zstd3, &compressed, raw.len() as u64 + 1).is_err());

        let mut dictionary = compressed.clone();
        dictionary[4] |= 0x01;
        assert!(decompress(ChunkCodec::Zstd3, &dictionary, raw.len() as u64).is_err());

        let mut reserved = compressed.clone();
        reserved[4] |= 0x08;
        assert!(decompress(ChunkCodec::Zstd3, &reserved, raw.len() as u64).is_err());

        let mut skippable = compressed.clone();
        skippable[..4].copy_from_slice(&[0x50, 0x2a, 0x4d, 0x18]);
        assert!(decompress(ChunkCodec::Zstd3, &skippable, raw.len() as u64).is_err());

        let mut truncated_checksum = compressed.clone();
        truncated_checksum[4] |= 0x04;
        truncated_checksum.truncate(truncated_checksum.len() - 3);
        assert!(decompress(ChunkCodec::Zstd3, &truncated_checksum, raw.len() as u64).is_err());
    }
}
