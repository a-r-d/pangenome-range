use crate::binary::{
    BinaryReader, count_bounded_by_bytes, invalid_data, put_u32, put_u64, u32_to_usize,
    u64_to_usize, usize_to_u32, usize_to_u64,
};
use crate::regional::MAX_DECODED_OCCURRENCES_PER_TILE;
use crate::{ExtensionPage, MAX_FEATURE_DESCRIPTOR_BYTES, MAX_FEATURE_PAGE_BYTES};
use std::io;

/// Optional production extension containing a paged path catalog and tile-local
/// multiplicity-bearing named memberships.
pub const PATH_MEMBERSHIP_TYPE_ID: [u8; 16] = *b"path-members-v1-";
pub const PATH_MEMBERSHIP_DESCRIPTOR_MAGIC: &[u8; 8] = b"PNGPMD01";
pub const PATH_CATALOG_PAGE_MAGIC: &[u8; 8] = b"PNGPCP01";
pub const PATH_MEMBERSHIP_DIRECTORY_MAGIC: &[u8; 8] = b"PNGPMI01";
pub const TILE_MEMBERSHIP_PAGE_MAGIC: &[u8; 8] = b"PNGPMT01";
pub const PATH_MEMBERSHIP_VERSION: u32 = 1;
pub const PATH_CATALOG_RECORDS_PER_PAGE: u32 = 1_024;
pub const PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES: usize = 4 * 1024;
pub const PATH_MEMBERSHIP_DIRECTORY_HEADER_BYTES: usize = 32;
pub const PATH_MEMBERSHIP_DIRECTORY_ENTRY_BYTES: usize = 56;
pub const PATH_MEMBERSHIP_DIRECTORY_ENTRIES_PER_PAGE: usize = (PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES
    - PATH_MEMBERSHIP_DIRECTORY_HEADER_BYTES)
    / PATH_MEMBERSHIP_DIRECTORY_ENTRY_BYTES;
pub const MAX_PATH_CATALOG_PAGES: usize = 1_000_000;
pub const MAX_PATH_MEMBERSHIP_MANIFESTS: usize = 1_000_000;
pub const MAX_PATH_MEMBERSHIP_GROUPS_PER_TILE: usize = 65_536;
/// Maximum number of materialized `(path_id, orientation, multiplicity)` records
/// in one group or one tile. This is intentionally separate from occurrence
/// weight because multiplicity does not require one decoded record per occurrence.
pub const MAX_PATH_MEMBERSHIPS_PER_GROUP: usize = 250_000;
pub const MAX_PATH_MEMBERSHIPS_PER_TILE: usize = 250_000;

const PATH_MEMBERSHIP_DESCRIPTOR_HEADER_BYTES: usize = 112;
const PATH_CATALOG_DESCRIPTOR_BYTES: usize = 64;
const PATH_MEMBERSHIP_MANIFEST_BYTES: usize = 32;

const DELTA_CODEC: u8 = 0;
const RUN_CODEC: u8 = 1;

pub const PATH_MEMBERSHIP_DELTA_CODEC: u8 = DELTA_CODEC;
pub const PATH_MEMBERSHIP_RUN_CODEC: u8 = RUN_CODEC;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathCatalogRecord {
    pub path_id: u64,
    /// Deterministic textual rendering reconstructed from GBWT metadata.
    pub canonical_name: String,
    pub sample: String,
    pub contig: String,
    pub haplotype: u64,
    pub fragment: u64,
    pub sense: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathCatalogPageDescriptor {
    pub first_path_id: u64,
    pub record_count: u64,
    pub storage: ExtensionPage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathMembershipDirectoryEntry {
    pub group_count: u64,
    pub storage: ExtensionPage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathMembershipManifest {
    pub manifest_index: u32,
    pub first_page_offset: u64,
    pub page_count: u64,
    pub entry_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathMembershipDescriptor {
    pub path_count: u64,
    pub records_per_catalog_page: u32,
    pub identity_source: PathIdentitySource,
    pub identity_source_sha256: [u8; 32],
    pub group_count: u64,
    pub occurrence_total: u64,
    pub group_unique_path_count_sum: u64,
    pub delta_group_count: u64,
    pub run_group_count: u64,
    pub catalog_pages: Vec<PathCatalogPageDescriptor>,
    pub manifests: Vec<PathMembershipManifest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathIdentitySource {
    EmbeddedGbwtDaBoundedLfV1,
    PreparedAuthenticatedOracleV1,
}

impl PathIdentitySource {
    fn code(self) -> u8 {
        match self {
            Self::EmbeddedGbwtDaBoundedLfV1 => 1,
            Self::PreparedAuthenticatedOracleV1 => 2,
        }
    }

    fn from_code(code: u8) -> io::Result<Self> {
        match code {
            1 => Ok(Self::EmbeddedGbwtDaBoundedLfV1),
            2 => Ok(Self::PreparedAuthenticatedOracleV1),
            _ => Err(invalid_data("unknown path identity source")),
        }
    }

    #[must_use]
    pub const fn implementation(self) -> &'static str {
        match self {
            Self::EmbeddedGbwtDaBoundedLfV1 => "embedded-gbwt-da-bounded-lf-v1",
            Self::PreparedAuthenticatedOracleV1 => "prepared-authenticated-oracle-v1",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathMembership {
    pub path_id: u64,
    pub multiplicity: u64,
    pub reversed_relative_to_group: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraversalMembershipGroup {
    pub traversal_digest: [u8; 16],
    pub occurrence_weight: u64,
    pub unique_path_count: u64,
    pub memberships: Vec<PathMembership>,
}

/// Returns the domain-separated BLAKE3-128 identity of one oriented-handle traversal.
///
/// # Panics
///
/// The fixed 16-byte conversion cannot fail because BLAKE3 always returns 32 bytes.
#[must_use]
pub fn traversal_membership_digest(
    sample: &str,
    contig: &str,
    core_start: u64,
    core_end: u64,
    regional_payload_integrity: &[u8; 16],
    handles: &[u64],
) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"pangenome-range/path-membership/traversal/v1\0");
    hasher.update(&(sample.len() as u64).to_le_bytes());
    hasher.update(sample.as_bytes());
    hasher.update(&(contig.len() as u64).to_le_bytes());
    hasher.update(contig.as_bytes());
    hasher.update(&core_start.to_le_bytes());
    hasher.update(&core_end.to_le_bytes());
    hasher.update(regional_payload_integrity);
    hasher.update(&(handles.len() as u64).to_le_bytes());
    for handle in handles {
        hasher.update(&handle.to_le_bytes());
    }
    hasher.finalize().as_bytes()[..16]
        .try_into()
        .expect("fixed BLAKE3 digest")
}

fn put_extension_page(output: &mut Vec<u8>, page: &ExtensionPage) {
    put_u64(output, page.offset);
    put_u64(output, page.encoded_len);
    put_u64(output, page.decoded_len);
    output.push(page.codec.code());
    output.extend_from_slice(&[0_u8; 7]);
    output.extend_from_slice(&page.integrity);
}

fn read_extension_page(reader: &mut BinaryReader<'_>) -> io::Result<ExtensionPage> {
    let offset = reader.u64()?;
    let encoded_len = reader.u64()?;
    let decoded_len = reader.u64()?;
    let codec = crate::ChunkCodec::from_code(reader.u8()?)?;
    if reader.take(7)? != [0_u8; 7] {
        return Err(invalid_data(
            "path-membership page reserved bytes are nonzero",
        ));
    }
    let integrity = reader
        .take(16)?
        .try_into()
        .map_err(|_| invalid_data("invalid path-membership page digest"))?;
    Ok(ExtensionPage {
        offset,
        encoded_len,
        decoded_len,
        codec,
        integrity,
    })
}

fn descriptor_bytes(catalog_count: usize, manifest_count: usize) -> io::Result<usize> {
    let bytes = PATH_MEMBERSHIP_DESCRIPTOR_HEADER_BYTES
        .checked_add(
            catalog_count
                .checked_mul(PATH_CATALOG_DESCRIPTOR_BYTES)
                .ok_or_else(|| invalid_data("path-membership descriptor size overflow"))?,
        )
        .and_then(|value| {
            manifest_count
                .checked_mul(PATH_MEMBERSHIP_MANIFEST_BYTES)
                .and_then(|bytes| value.checked_add(bytes))
        })
        .ok_or_else(|| invalid_data("path-membership descriptor size overflow"))?;
    if usize_to_u64(bytes)? > MAX_FEATURE_DESCRIPTOR_BYTES {
        return Err(invalid_data("path-membership descriptor is too large"));
    }
    Ok(bytes)
}

/// Encodes the small root descriptor for the path-membership extension.
///
/// # Errors
///
/// Returns an error for invalid dimensions, ordering, ranges, or integer overflow.
pub fn encode_path_membership_descriptor(
    descriptor: &PathMembershipDescriptor,
) -> io::Result<Vec<u8>> {
    if descriptor.path_count == 0
        || descriptor.records_per_catalog_page == 0
        || descriptor.records_per_catalog_page > 65_536
        || descriptor.catalog_pages.is_empty()
        || descriptor.catalog_pages.len() > MAX_PATH_CATALOG_PAGES
        || descriptor.manifests.is_empty()
        || descriptor.manifests.len() > MAX_PATH_MEMBERSHIP_MANIFESTS
    {
        return Err(invalid_data(
            "invalid path-membership descriptor dimensions",
        ));
    }
    if descriptor.identity_source_sha256 == [0; 32]
        || descriptor
            .delta_group_count
            .checked_add(descriptor.run_group_count)
            != Some(descriptor.group_count)
        || descriptor.group_unique_path_count_sum > descriptor.occurrence_total
    {
        return Err(invalid_data("invalid path-membership provenance totals"));
    }
    let encoded_bytes =
        descriptor_bytes(descriptor.catalog_pages.len(), descriptor.manifests.len())?;
    let mut output = Vec::with_capacity(encoded_bytes);
    output.extend_from_slice(PATH_MEMBERSHIP_DESCRIPTOR_MAGIC);
    put_u32(&mut output, PATH_MEMBERSHIP_VERSION);
    put_u32(&mut output, descriptor.records_per_catalog_page);
    put_u64(&mut output, descriptor.path_count);
    put_u32(&mut output, usize_to_u32(descriptor.catalog_pages.len())?);
    put_u32(&mut output, usize_to_u32(descriptor.manifests.len())?);
    output.push(descriptor.identity_source.code());
    output.extend_from_slice(&[0_u8; 7]);
    output.extend_from_slice(&descriptor.identity_source_sha256);
    put_u64(&mut output, descriptor.group_count);
    put_u64(&mut output, descriptor.occurrence_total);
    put_u64(&mut output, descriptor.group_unique_path_count_sum);
    put_u64(&mut output, descriptor.delta_group_count);
    put_u64(&mut output, descriptor.run_group_count);
    let mut expected_first = 0_u64;
    let mut catalog_records = 0_u64;
    for page in &descriptor.catalog_pages {
        if page.first_path_id != expected_first || page.record_count == 0 {
            return Err(invalid_data("path catalog page IDs are not contiguous"));
        }
        put_u64(&mut output, page.first_path_id);
        put_u64(&mut output, page.record_count);
        put_extension_page(&mut output, &page.storage);
        expected_first = expected_first
            .checked_add(page.record_count)
            .ok_or_else(|| invalid_data("path catalog record count overflow"))?;
        catalog_records = expected_first;
    }
    if catalog_records != descriptor.path_count {
        return Err(invalid_data(
            "path catalog page count differs from descriptor",
        ));
    }
    let mut previous_page_end = None;
    let mut total_entries = 0_u64;
    for (index, manifest) in descriptor.manifests.iter().enumerate() {
        let expected_index = usize_to_u32(index)?;
        let page_bytes = manifest
            .page_count
            .checked_mul(usize_to_u64(PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES)?)
            .ok_or_else(|| invalid_data("path-membership directory size overflow"))?;
        let page_end = manifest
            .first_page_offset
            .checked_add(page_bytes)
            .ok_or_else(|| invalid_data("path-membership directory range overflow"))?;
        if manifest.manifest_index != expected_index
            || manifest.page_count == 0
            || manifest.entry_count == 0
            || previous_page_end.is_some_and(|end| manifest.first_page_offset != end)
        {
            return Err(invalid_data("invalid path-membership manifest"));
        }
        put_u32(&mut output, manifest.manifest_index);
        put_u32(&mut output, 0);
        put_u64(&mut output, manifest.first_page_offset);
        put_u64(&mut output, manifest.page_count);
        put_u64(&mut output, manifest.entry_count);
        previous_page_end = Some(page_end);
        total_entries = total_entries
            .checked_add(manifest.entry_count)
            .ok_or_else(|| invalid_data("path-membership entry count overflow"))?;
    }
    if total_entries == 0 {
        return Err(invalid_data("path-membership descriptor has no entries"));
    }
    debug_assert_eq!(output.len(), encoded_bytes);
    Ok(output)
}

/// Decodes and bounds-checks the path-membership root descriptor.
///
/// # Errors
///
/// Returns an error for truncation, trailing bytes, invalid dimensions, ordering,
/// codecs, or object ranges.
#[allow(clippy::too_many_lines)]
pub fn decode_path_membership_descriptor(
    bytes: &[u8],
    data_offset: u64,
    object_len: u64,
) -> io::Result<PathMembershipDescriptor> {
    if usize_to_u64(bytes.len())? > MAX_FEATURE_DESCRIPTOR_BYTES {
        return Err(invalid_data("path-membership descriptor is too large"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != PATH_MEMBERSHIP_DESCRIPTOR_MAGIC
        || reader.u32()? != PATH_MEMBERSHIP_VERSION
    {
        return Err(invalid_data("invalid path-membership descriptor header"));
    }
    let records_per_catalog_page = reader.u32()?;
    let path_count = reader.u64()?;
    let catalog_count = u32_to_usize(reader.u32()?)?;
    let manifest_count = u32_to_usize(reader.u32()?)?;
    let identity_source = PathIdentitySource::from_code(reader.u8()?)?;
    if reader.take(7)? != [0_u8; 7] {
        return Err(invalid_data(
            "path-membership provenance reserved bytes are nonzero",
        ));
    }
    let identity_source_sha256 = reader
        .take(32)?
        .try_into()
        .map_err(|_| invalid_data("invalid path identity source checksum"))?;
    let group_count = reader.u64()?;
    let occurrence_total = reader.u64()?;
    let group_unique_path_count_sum = reader.u64()?;
    let delta_group_count = reader.u64()?;
    let run_group_count = reader.u64()?;
    if path_count == 0
        || records_per_catalog_page == 0
        || records_per_catalog_page > 65_536
        || catalog_count == 0
        || catalog_count > MAX_PATH_CATALOG_PAGES
        || manifest_count == 0
        || manifest_count > MAX_PATH_MEMBERSHIP_MANIFESTS
    {
        return Err(invalid_data(
            "invalid path-membership descriptor dimensions",
        ));
    }
    if identity_source_sha256 == [0; 32]
        || delta_group_count.checked_add(run_group_count) != Some(group_count)
        || group_unique_path_count_sum > occurrence_total
    {
        return Err(invalid_data("invalid path-membership provenance totals"));
    }
    let expected_bytes = descriptor_bytes(catalog_count, manifest_count)?;
    if bytes.len() != expected_bytes {
        return Err(invalid_data("invalid path-membership descriptor length"));
    }
    let mut catalog_pages = Vec::with_capacity(catalog_count);
    let mut expected_first = 0_u64;
    for _ in 0..catalog_count {
        let first_path_id = reader.u64()?;
        let record_count = reader.u64()?;
        let storage = read_extension_page(&mut reader)?;
        storage.validate(data_offset, object_len)?;
        if first_path_id != expected_first
            || record_count == 0
            || record_count > u64::from(records_per_catalog_page)
        {
            return Err(invalid_data("invalid path catalog page dimensions"));
        }
        expected_first = expected_first
            .checked_add(record_count)
            .ok_or_else(|| invalid_data("path catalog record count overflow"))?;
        catalog_pages.push(PathCatalogPageDescriptor {
            first_path_id,
            record_count,
            storage,
        });
    }
    if expected_first != path_count {
        return Err(invalid_data(
            "path catalog pages do not cover the descriptor",
        ));
    }
    let mut manifests = Vec::with_capacity(manifest_count);
    let mut previous_page_end = None;
    let mut total_entries = 0_u64;
    for index in 0..manifest_count {
        let manifest_index = reader.u32()?;
        if reader.u32()? != 0 {
            return Err(invalid_data(
                "path-membership manifest reserved bytes are nonzero",
            ));
        }
        let first_page_offset = reader.u64()?;
        let page_count = reader.u64()?;
        let entry_count = reader.u64()?;
        let page_bytes = page_count
            .checked_mul(usize_to_u64(PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES)?)
            .ok_or_else(|| invalid_data("path-membership directory size overflow"))?;
        let page_end = first_page_offset
            .checked_add(page_bytes)
            .ok_or_else(|| invalid_data("path-membership directory range overflow"))?;
        if manifest_index != usize_to_u32(index)?
            || page_count == 0
            || entry_count == 0
            || first_page_offset < data_offset
            || page_end > object_len
            || previous_page_end.is_some_and(|end| first_page_offset != end)
        {
            return Err(invalid_data("invalid path-membership manifest"));
        }
        manifests.push(PathMembershipManifest {
            manifest_index,
            first_page_offset,
            page_count,
            entry_count,
        });
        previous_page_end = Some(page_end);
        total_entries = total_entries
            .checked_add(entry_count)
            .ok_or_else(|| invalid_data("path-membership entry count overflow"))?;
    }
    if total_entries == 0 {
        return Err(invalid_data("path-membership descriptor has no entries"));
    }
    reader.finish()?;
    Ok(PathMembershipDescriptor {
        path_count,
        records_per_catalog_page,
        identity_source,
        identity_source_sha256,
        group_count,
        occurrence_total,
        group_unique_path_count_sum,
        delta_group_count,
        run_group_count,
        catalog_pages,
        manifests,
    })
}

/// Encodes one fixed 4 KiB membership-directory page aligned with a graph directory page.
///
/// # Errors
///
/// Returns an error for an oversized page, invalid group count, page range, or
/// integer overflow. Empty pages are valid when the aligned graph bucket is empty.
pub fn encode_path_membership_directory_page(
    entries: &[PathMembershipDirectoryEntry],
) -> io::Result<[u8; PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES]> {
    if entries.len() > PATH_MEMBERSHIP_DIRECTORY_ENTRIES_PER_PAGE {
        return Err(invalid_data(
            "invalid path-membership directory entry count",
        ));
    }
    let mut output = Vec::with_capacity(PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES);
    output.extend_from_slice(PATH_MEMBERSHIP_DIRECTORY_MAGIC);
    put_u32(&mut output, PATH_MEMBERSHIP_VERSION);
    put_u32(&mut output, usize_to_u32(entries.len())?);
    output.extend_from_slice(&[0_u8; 16]);
    for entry in entries {
        if entry.group_count > MAX_PATH_MEMBERSHIP_GROUPS_PER_TILE as u64 {
            return Err(invalid_data("invalid path-membership directory entry"));
        }
        put_u64(&mut output, entry.group_count);
        put_extension_page(&mut output, &entry.storage);
    }
    output.resize(PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES, 0);
    let digest = blake3::hash(&output[PATH_MEMBERSHIP_DIRECTORY_HEADER_BYTES..]);
    output[16..32].copy_from_slice(&digest.as_bytes()[..16]);
    output
        .try_into()
        .map_err(|_| invalid_data("path-membership directory page size mismatch"))
}

/// Decodes one fixed membership-directory page and validates every child range.
///
/// # Errors
///
/// Returns an error for corruption, invalid counts, child storage, nonzero padding,
/// or object ranges.
pub fn decode_path_membership_directory_page(
    bytes: &[u8],
    data_offset: u64,
    object_len: u64,
) -> io::Result<Vec<PathMembershipDirectoryEntry>> {
    if bytes.len() != PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES {
        return Err(invalid_data("invalid path-membership directory page size"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != PATH_MEMBERSHIP_DIRECTORY_MAGIC
        || reader.u32()? != PATH_MEMBERSHIP_VERSION
    {
        return Err(invalid_data(
            "invalid path-membership directory page header",
        ));
    }
    let count = u32_to_usize(reader.u32()?)?;
    let expected_digest = reader.take(16)?;
    let actual_digest = blake3::hash(&bytes[PATH_MEMBERSHIP_DIRECTORY_HEADER_BYTES..]);
    if count > PATH_MEMBERSHIP_DIRECTORY_ENTRIES_PER_PAGE
        || expected_digest != &actual_digest.as_bytes()[..16]
    {
        return Err(invalid_data("invalid path-membership directory page"));
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let group_count = reader.u64()?;
        let storage = read_extension_page(&mut reader)?;
        storage.validate(data_offset, object_len)?;
        if group_count > MAX_PATH_MEMBERSHIP_GROUPS_PER_TILE as u64 {
            return Err(invalid_data("invalid path-membership directory entry"));
        }
        entries.push(PathMembershipDirectoryEntry {
            group_count,
            storage,
        });
    }
    if reader
        .take(reader.remaining())?
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(invalid_data("path-membership directory padding is nonzero"));
    }
    Ok(entries)
}

fn common_prefix(left: &[u8], right: &[u8]) -> usize {
    left.iter().zip(right).take_while(|(a, b)| a == b).count()
}

fn put_varint(output: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn read_varint(reader: &mut BinaryReader<'_>) -> io::Result<u64> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    loop {
        let byte = reader.u8()?;
        let payload = u64::from(byte & 0x7f);
        if shift >= 64 || payload > (u64::MAX >> shift) {
            return Err(invalid_data("path-membership varint overflow"));
        }
        value |= payload << shift;
        if byte & 0x80 == 0 {
            if shift != 0 && payload == 0 {
                return Err(invalid_data("non-minimal path-membership varint"));
            }
            return Ok(value);
        }
        shift = shift
            .checked_add(7)
            .ok_or_else(|| invalid_data("path-membership varint shift overflow"))?;
    }
}

fn put_front_coded(output: &mut Vec<u8>, previous: &str, value: &str) {
    let mut prefix = common_prefix(previous.as_bytes(), value.as_bytes());
    while prefix > 0 && !previous.is_char_boundary(prefix) {
        prefix -= 1;
    }
    put_varint(output, prefix as u64);
    put_varint(output, (value.len() - prefix) as u64);
    output.extend_from_slice(&value.as_bytes()[prefix..]);
}

fn read_front_coded(reader: &mut BinaryReader<'_>, previous: &str) -> io::Result<String> {
    let prefix = u64_to_usize(read_varint(reader)?)?;
    let suffix_len = u64_to_usize(read_varint(reader)?)?;
    if prefix > previous.len() || !previous.is_char_boundary(prefix) {
        return Err(invalid_data("invalid front-coded path string prefix"));
    }
    let suffix = std::str::from_utf8(reader.take(suffix_len)?)
        .map_err(|_| invalid_data("invalid UTF-8 path string"))?;
    let mut value = previous[..prefix].to_owned();
    value.push_str(suffix);
    Ok(value)
}

/// Encodes one independently compressed path-catalog page.
///
/// # Errors
///
/// Returns an error for an empty or oversized page, noncontiguous IDs, invalid path
/// sense, string length, or integer overflow.
pub fn encode_path_catalog_page(records: &[PathCatalogRecord]) -> io::Result<Vec<u8>> {
    if records.is_empty() || records.len() > 65_536 {
        return Err(invalid_data("invalid path catalog page record count"));
    }
    let first = records[0].path_id;
    let mut output = Vec::new();
    output.extend_from_slice(PATH_CATALOG_PAGE_MAGIC);
    put_u32(&mut output, PATH_MEMBERSHIP_VERSION);
    put_u32(&mut output, usize_to_u32(records.len())?);
    put_u64(&mut output, first);
    let mut canonical_name = String::new();
    let mut sample = String::new();
    let mut contig = String::new();
    for (index, record) in records.iter().enumerate() {
        let expected = first
            .checked_add(usize_to_u64(index)?)
            .ok_or_else(|| invalid_data("path catalog ID overflow"))?;
        if record.path_id != expected || record.sense > 3 {
            return Err(invalid_data("invalid path catalog record order or sense"));
        }
        put_front_coded(&mut output, &canonical_name, &record.canonical_name);
        put_front_coded(&mut output, &sample, &record.sample);
        put_front_coded(&mut output, &contig, &record.contig);
        put_varint(&mut output, record.haplotype);
        put_varint(&mut output, record.fragment);
        output.push(record.sense);
        canonical_name.clone_from(&record.canonical_name);
        sample.clone_from(&record.sample);
        contig.clone_from(&record.contig);
    }
    if usize_to_u64(output.len())? > MAX_FEATURE_PAGE_BYTES {
        return Err(invalid_data("path catalog page is too large"));
    }
    Ok(output)
}

/// Decodes one complete path-catalog page.
///
/// # Errors
///
/// Returns an error for an invalid header, count, string, sense, integer overflow,
/// truncation, or trailing bytes.
pub fn decode_path_catalog_page(bytes: &[u8]) -> io::Result<Vec<PathCatalogRecord>> {
    if usize_to_u64(bytes.len())? > MAX_FEATURE_PAGE_BYTES {
        return Err(invalid_data("path catalog page is too large"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != PATH_CATALOG_PAGE_MAGIC || reader.u32()? != PATH_MEMBERSHIP_VERSION {
        return Err(invalid_data("invalid path catalog page header"));
    }
    let count =
        count_bounded_by_bytes(reader.u32()?.into(), reader.remaining(), 6, "path catalog")?;
    if count == 0 {
        return Err(invalid_data("empty path catalog page"));
    }
    let first = reader.u64()?;
    let mut records = Vec::with_capacity(count);
    let mut canonical_name = String::new();
    let mut sample = String::new();
    let mut contig = String::new();
    let mut reconstructed_bytes = 0_u64;
    for index in 0..count {
        let next_canonical_name = read_front_coded(&mut reader, &canonical_name)?;
        let next_sample = read_front_coded(&mut reader, &sample)?;
        let next_contig = read_front_coded(&mut reader, &contig)?;
        reconstructed_bytes = reconstructed_bytes
            .checked_add(usize_to_u64(next_canonical_name.len())?)
            .and_then(|value| {
                usize_to_u64(next_sample.len())
                    .ok()
                    .and_then(|bytes| value.checked_add(bytes))
            })
            .and_then(|value| {
                usize_to_u64(next_contig.len())
                    .ok()
                    .and_then(|bytes| value.checked_add(bytes))
            })
            .ok_or_else(|| invalid_data("path catalog reconstructed string size overflow"))?;
        if reconstructed_bytes > MAX_FEATURE_PAGE_BYTES {
            return Err(invalid_data(
                "path catalog reconstructed strings exceed the page bound",
            ));
        }
        let haplotype = read_varint(&mut reader)?;
        let fragment = read_varint(&mut reader)?;
        let sense = reader.u8()?;
        if sense > 3 {
            return Err(invalid_data("invalid path sense"));
        }
        records.push(PathCatalogRecord {
            path_id: first
                .checked_add(usize_to_u64(index)?)
                .ok_or_else(|| invalid_data("path catalog ID overflow"))?,
            canonical_name: next_canonical_name.clone(),
            sample: next_sample.clone(),
            contig: next_contig.clone(),
            haplotype,
            fragment,
            sense,
        });
        canonical_name = next_canonical_name;
        sample = next_sample;
        contig = next_contig;
    }
    reader.finish()?;
    Ok(records)
}

fn encode_delta(memberships: &[PathMembership]) -> io::Result<Vec<u8>> {
    let mut values = memberships.to_vec();
    values.sort_by_key(|item| (item.path_id, item.reversed_relative_to_group));
    validate_membership_id_order(&values)?;
    let mut output = vec![DELTA_CODEC];
    put_varint(&mut output, usize_to_u64(values.len())?);
    let mut previous = 0_u64;
    for item in values {
        if item.multiplicity == 0 {
            return Err(invalid_data("zero path-membership multiplicity"));
        }
        put_varint(&mut output, item.path_id - previous);
        put_varint(&mut output, item.multiplicity);
        output.push(u8::from(item.reversed_relative_to_group));
        previous = item.path_id;
    }
    Ok(output)
}

fn encode_runs(memberships: &[PathMembership]) -> io::Result<Vec<u8>> {
    let mut values = memberships.to_vec();
    values.sort_by_key(|item| (item.path_id, item.reversed_relative_to_group));
    validate_membership_id_order(&values)?;
    let mut records = Vec::<(u8, u64, u64, bool)>::new();
    let mut index = 0;
    while index < values.len() {
        if values[index].multiplicity == 0 {
            return Err(invalid_data("zero path-membership multiplicity"));
        }
        let mut end = index + 1;
        if values[index].multiplicity == 1 {
            while end < values.len()
                && values[end].multiplicity == 1
                && values[end].reversed_relative_to_group
                    == values[index].reversed_relative_to_group
                && values[end].path_id == values[end - 1].path_id + 1
            {
                end += 1;
            }
        }
        if end - index >= 2 {
            records.push((
                1,
                values[index].path_id,
                (end - index) as u64,
                values[index].reversed_relative_to_group,
            ));
        } else {
            records.push((
                0,
                values[index].path_id,
                values[index].multiplicity,
                values[index].reversed_relative_to_group,
            ));
        }
        index = end;
    }
    let mut output = vec![RUN_CODEC];
    put_varint(&mut output, usize_to_u64(values.len())?);
    put_varint(&mut output, usize_to_u64(records.len())?);
    let mut previous_end = 0_u64;
    for (kind, path_id, value, reverse) in records {
        output.push(kind);
        put_varint(&mut output, path_id - previous_end);
        put_varint(&mut output, value);
        output.push(u8::from(reverse));
        previous_end = if kind == 1 {
            path_id
                .checked_add(value - 1)
                .ok_or_else(|| invalid_data("path-membership run overflow"))?
        } else {
            path_id
        };
    }
    Ok(output)
}

fn decode_memberships(bytes: &[u8], path_count: u64) -> io::Result<Vec<PathMembership>> {
    let mut reader = BinaryReader::new(bytes);
    let codec = reader.u8()?;
    let expected_u64 = read_varint(&mut reader)?;
    if expected_u64 == 0 || expected_u64 > MAX_PATH_MEMBERSHIPS_PER_GROUP as u64 {
        return Err(invalid_data(
            "path-membership entry count exceeds its safety bound",
        ));
    }
    let expected = u64_to_usize(expected_u64)?;
    let mut result = Vec::with_capacity(expected.min(4_096));
    let mut previous_end = 0_u64;
    match codec {
        DELTA_CODEC => {
            for _ in 0..expected {
                let path_id = previous_end
                    .checked_add(read_varint(&mut reader)?)
                    .ok_or_else(|| invalid_data("path-membership delta overflow"))?;
                let multiplicity = read_varint(&mut reader)?;
                let reverse = reader.u8()?;
                if path_id >= path_count || multiplicity == 0 || reverse > 1 {
                    return Err(invalid_data("invalid path-membership entry"));
                }
                result.push(PathMembership {
                    path_id,
                    multiplicity,
                    reversed_relative_to_group: reverse != 0,
                });
                previous_end = path_id;
            }
        }
        RUN_CODEC => {
            let record_count = count_bounded_by_bytes(
                read_varint(&mut reader)?,
                reader.remaining(),
                4,
                "path-membership runs",
            )?;
            for _ in 0..record_count {
                let kind = reader.u8()?;
                let path_id = previous_end
                    .checked_add(read_varint(&mut reader)?)
                    .ok_or_else(|| invalid_data("path-membership run delta overflow"))?;
                let value = read_varint(&mut reader)?;
                let reverse = reader.u8()?;
                if value == 0 || reverse > 1 || kind > 1 {
                    return Err(invalid_data("invalid path-membership run"));
                }
                if kind == 0 {
                    if path_id >= path_count || result.len() >= expected {
                        return Err(invalid_data("invalid path-membership run bounds"));
                    }
                    result.push(PathMembership {
                        path_id,
                        multiplicity: value,
                        reversed_relative_to_group: reverse != 0,
                    });
                    previous_end = path_id;
                } else {
                    let end = path_id
                        .checked_add(value)
                        .ok_or_else(|| invalid_data("path-membership run overflow"))?;
                    let expanded = usize_to_u64(result.len())?
                        .checked_add(value)
                        .ok_or_else(|| invalid_data("path-membership run count overflow"))?;
                    if end > path_count || expanded > expected_u64 {
                        return Err(invalid_data("invalid path-membership run bounds"));
                    }
                    for id in path_id..end {
                        result.push(PathMembership {
                            path_id: id,
                            multiplicity: 1,
                            reversed_relative_to_group: reverse != 0,
                        });
                    }
                    previous_end = end - 1;
                }
            }
            if result.len() != expected {
                return Err(invalid_data("path-membership run count mismatch"));
            }
        }
        _ => return Err(invalid_data("unknown path-membership codec")),
    }
    reader.finish()?;
    if result.iter().any(|item| item.path_id >= path_count) {
        return Err(invalid_data("path membership refers outside the catalog"));
    }
    validate_membership_id_order(&result)?;
    Ok(result)
}

fn validate_membership_id_order(memberships: &[PathMembership]) -> io::Result<()> {
    if memberships.windows(2).any(|pair| {
        (pair[0].path_id, pair[0].reversed_relative_to_group)
            >= (pair[1].path_id, pair[1].reversed_relative_to_group)
    }) {
        return Err(invalid_data(
            "path-membership path/orientation pairs are not unique and ordered",
        ));
    }
    Ok(())
}

/// Returns the deterministic codec selected for one membership group.
///
/// # Errors
///
/// Returns an error when the memberships cannot be encoded canonically.
pub fn selected_path_membership_codec(memberships: &[PathMembership]) -> io::Result<u8> {
    let delta = encode_delta(memberships)?;
    let runs = encode_runs(memberships)?;
    Ok(if (runs.len(), RUN_CODEC) < (delta.len(), DELTA_CODEC) {
        RUN_CODEC
    } else {
        DELTA_CODEC
    })
}

/// Encodes one tile-local membership page, choosing delta or interval runs per group.
///
/// # Errors
///
/// Returns an error for invalid dimensions, duplicate path IDs, inconsistent totals,
/// excessive occurrence expansion, or integer overflow.
pub fn encode_tile_membership_page(
    core_start: u64,
    core_end: u64,
    regional_payload_integrity: &[u8; 16],
    groups: &[TraversalMembershipGroup],
) -> io::Result<Vec<u8>> {
    if core_start >= core_end
        || groups.len() > MAX_PATH_MEMBERSHIP_GROUPS_PER_TILE
        || groups
            .windows(2)
            .any(|pair| pair[0].traversal_digest >= pair[1].traversal_digest)
    {
        return Err(invalid_data("invalid tile-membership page dimensions"));
    }
    let mut output = Vec::new();
    output.extend_from_slice(TILE_MEMBERSHIP_PAGE_MAGIC);
    put_u32(&mut output, PATH_MEMBERSHIP_VERSION);
    put_u32(&mut output, usize_to_u32(groups.len())?);
    put_u64(&mut output, core_start);
    put_u64(&mut output, core_end);
    output.extend_from_slice(regional_payload_integrity);
    let mut total_occurrence_weight = 0_u64;
    let mut total_memberships = 0_usize;
    for group in groups {
        if group.occurrence_weight == 0
            || group.occurrence_weight > MAX_DECODED_OCCURRENCES_PER_TILE
            || group.memberships.is_empty()
        {
            return Err(invalid_data("empty tile-membership group"));
        }
        if group.memberships.len() > MAX_PATH_MEMBERSHIPS_PER_GROUP {
            return Err(invalid_data(
                "path-membership entry count exceeds its safety bound",
            ));
        }
        total_memberships = total_memberships
            .checked_add(group.memberships.len())
            .ok_or_else(|| invalid_data("tile-membership record count overflow"))?;
        if total_memberships > MAX_PATH_MEMBERSHIPS_PER_TILE {
            return Err(invalid_data(
                "tile-membership record count exceeds its safety bound",
            ));
        }
        total_occurrence_weight = total_occurrence_weight
            .checked_add(group.occurrence_weight)
            .ok_or_else(|| invalid_data("tile-membership occurrence weight overflow"))?;
        if total_occurrence_weight > MAX_DECODED_OCCURRENCES_PER_TILE {
            return Err(invalid_data(
                "tile-membership occurrence total exceeds its safety bound",
            ));
        }
        let delta = encode_delta(&group.memberships)?;
        let runs = encode_runs(&group.memberships)?;
        let encoded = if (runs.len(), RUN_CODEC) < (delta.len(), DELTA_CODEC) {
            runs
        } else {
            delta
        };
        let sum = group.memberships.iter().try_fold(0_u64, |total, item| {
            total
                .checked_add(item.multiplicity)
                .ok_or_else(|| invalid_data("path-membership weight overflow"))
        })?;
        let unique = group
            .memberships
            .iter()
            .map(|item| item.path_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if sum != group.occurrence_weight || usize_to_u64(unique)? != group.unique_path_count {
            return Err(invalid_data(
                "tile-membership group totals are inconsistent",
            ));
        }
        output.extend_from_slice(&group.traversal_digest);
        put_u64(&mut output, group.occurrence_weight);
        put_u64(&mut output, group.unique_path_count);
        put_u64(&mut output, usize_to_u64(encoded.len())?);
        output.extend_from_slice(&encoded);
    }
    if usize_to_u64(output.len())? > MAX_FEATURE_PAGE_BYTES {
        return Err(invalid_data("tile-membership page is too large"));
    }
    Ok(output)
}

/// Decodes one complete tile-local membership page.
///
/// # Errors
///
/// Returns an error for an invalid header, dimensions, ordering, codec, multiplicity,
/// safety bound, truncation, or trailing bytes.
pub fn decode_tile_membership_page(
    bytes: &[u8],
    path_count: u64,
) -> io::Result<(u64, u64, [u8; 16], Vec<TraversalMembershipGroup>)> {
    if usize_to_u64(bytes.len())? > MAX_FEATURE_PAGE_BYTES {
        return Err(invalid_data("tile-membership page is too large"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != TILE_MEMBERSHIP_PAGE_MAGIC || reader.u32()? != PATH_MEMBERSHIP_VERSION {
        return Err(invalid_data("invalid tile-membership page header"));
    }
    let count = count_bounded_by_bytes(
        reader.u32()?.into(),
        reader.remaining(),
        41,
        "tile membership",
    )?;
    if count > MAX_PATH_MEMBERSHIP_GROUPS_PER_TILE {
        return Err(invalid_data("invalid tile-membership group count"));
    }
    let core_start = reader.u64()?;
    let core_end = reader.u64()?;
    let regional_payload_integrity = reader
        .take(16)?
        .try_into()
        .map_err(|_| invalid_data("invalid regional payload integrity"))?;
    if core_start >= core_end {
        return Err(invalid_data("invalid tile-membership core interval"));
    }
    let mut groups = Vec::with_capacity(count);
    let mut total_occurrence_weight = 0_u64;
    let mut total_memberships = 0_usize;
    let mut previous_digest = None;
    for _ in 0..count {
        let traversal_digest = reader
            .take(16)?
            .try_into()
            .map_err(|_| invalid_data("invalid traversal digest"))?;
        let occurrence_weight = reader.u64()?;
        let unique_path_count = reader.u64()?;
        let encoded_len = u64_to_usize(reader.u64()?)?;
        let encoded = reader.take(encoded_len)?;
        let codec = encoded
            .first()
            .copied()
            .ok_or_else(|| invalid_data("empty membership codec payload"))?;
        let memberships = decode_memberships(encoded, path_count)?;
        total_memberships = total_memberships
            .checked_add(memberships.len())
            .ok_or_else(|| invalid_data("tile-membership record count overflow"))?;
        if total_memberships > MAX_PATH_MEMBERSHIPS_PER_TILE {
            return Err(invalid_data(
                "tile-membership record count exceeds its safety bound",
            ));
        }
        if selected_path_membership_codec(&memberships)? != codec {
            return Err(invalid_data(
                "membership codec differs from deterministic selection",
            ));
        }
        let sum = memberships.iter().try_fold(0_u64, |total, item| {
            total
                .checked_add(item.multiplicity)
                .ok_or_else(|| invalid_data("path-membership weight overflow"))
        })?;
        let unique = memberships
            .iter()
            .map(|item| item.path_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if occurrence_weight == 0
            || sum != occurrence_weight
            || usize_to_u64(unique)? != unique_path_count
            || previous_digest.is_some_and(|previous| previous >= traversal_digest)
        {
            return Err(invalid_data(
                "tile-membership group totals are inconsistent",
            ));
        }
        total_occurrence_weight = total_occurrence_weight
            .checked_add(occurrence_weight)
            .ok_or_else(|| invalid_data("tile-membership occurrence weight overflow"))?;
        if total_occurrence_weight > MAX_DECODED_OCCURRENCES_PER_TILE {
            return Err(invalid_data(
                "tile-membership occurrence total exceeds its safety bound",
            ));
        }
        groups.push(TraversalMembershipGroup {
            traversal_digest,
            occurrence_weight,
            unique_path_count,
            memberships,
        });
        previous_digest = Some(traversal_digest);
    }
    reader.finish()?;
    Ok((core_start, core_end, regional_payload_integrity, groups))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChunkCodec;

    fn storage(offset: u64) -> ExtensionPage {
        ExtensionPage {
            offset,
            encoded_len: 10,
            decoded_len: 20,
            codec: ChunkCodec::Zstd3,
            integrity: [7; 16],
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn descriptor_and_pages_round_trip() {
        let records = vec![
            PathCatalogRecord {
                path_id: 0,
                canonical_name: "sample#0#chr1".into(),
                sample: "sample".into(),
                contig: "chr1".into(),
                haplotype: 0,
                fragment: 0,
                sense: 3,
            },
            PathCatalogRecord {
                path_id: 1,
                canonical_name: "sample#1#chr1".into(),
                sample: "sample".into(),
                contig: "chr1".into(),
                haplotype: 1,
                fragment: 0,
                sense: 3,
            },
        ];
        let catalog = encode_path_catalog_page(&records).unwrap();
        assert_eq!(decode_path_catalog_page(&catalog).unwrap(), records);

        let regional_integrity = [9; 16];
        assert_eq!(
            traversal_membership_digest(
                "sample",
                "chr1",
                100,
                200,
                &regional_integrity,
                &[2, 4, 6],
            ),
            [
                0x67, 0x0d, 0x47, 0xd5, 0x46, 0xab, 0xb2, 0x1b, 0xce, 0x59, 0x5e, 0xe9, 0x81, 0x3e,
                0xb7, 0xa0,
            ]
        );
        let groups = vec![TraversalMembershipGroup {
            traversal_digest: traversal_membership_digest(
                "sample",
                "chr1",
                100,
                200,
                &regional_integrity,
                &[2, 4, 6],
            ),
            occurrence_weight: 2,
            unique_path_count: 2,
            memberships: vec![
                PathMembership {
                    path_id: 0,
                    multiplicity: 1,
                    reversed_relative_to_group: false,
                },
                PathMembership {
                    path_id: 1,
                    multiplicity: 1,
                    reversed_relative_to_group: false,
                },
            ],
        }];
        let tile = encode_tile_membership_page(100, 200, &regional_integrity, &groups).unwrap();
        assert_eq!(
            decode_tile_membership_page(&tile, 2).unwrap(),
            (100, 200, regional_integrity, groups)
        );
        assert!(decode_tile_membership_page(&tile[..tile.len() - 1], 2).is_err());
        assert!(decode_tile_membership_page(&tile, 1).is_err());
        let empty_tile = encode_tile_membership_page(200, 300, &regional_integrity, &[]).unwrap();
        assert_eq!(
            decode_tile_membership_page(&empty_tile, 2).unwrap(),
            (200, 300, regional_integrity, Vec::new())
        );

        let descriptor = PathMembershipDescriptor {
            path_count: 2,
            records_per_catalog_page: 1_024,
            identity_source: PathIdentitySource::EmbeddedGbwtDaBoundedLfV1,
            identity_source_sha256: [7; 32],
            group_count: 1,
            occurrence_total: 2,
            group_unique_path_count_sum: 2,
            delta_group_count: 1,
            run_group_count: 0,
            catalog_pages: vec![PathCatalogPageDescriptor {
                first_path_id: 0,
                record_count: 2,
                storage: storage(1_000),
            }],
            manifests: vec![PathMembershipManifest {
                manifest_index: 0,
                first_page_offset: 2_000,
                page_count: 1,
                entry_count: 1,
            }],
        };
        let encoded = encode_path_membership_descriptor(&descriptor).unwrap();
        assert_eq!(
            decode_path_membership_descriptor(&encoded, 500, 7_000).unwrap(),
            descriptor
        );
        assert!(
            decode_path_membership_descriptor(&encoded[..encoded.len() - 1], 500, 7_000).is_err()
        );

        let directory_entries = vec![PathMembershipDirectoryEntry {
            group_count: 1,
            storage: storage(1_500),
        }];
        let directory = encode_path_membership_directory_page(&directory_entries).unwrap();
        assert_eq!(
            decode_path_membership_directory_page(&directory, 500, 7_000).unwrap(),
            directory_entries
        );
        let mut corrupt_directory = directory;
        corrupt_directory[PATH_MEMBERSHIP_DIRECTORY_HEADER_BYTES] ^= 1;
        assert!(decode_path_membership_directory_page(&corrupt_directory, 500, 7_000).is_err());
    }

    #[test]
    fn membership_decode_preserves_dual_orientations_and_rejects_exact_duplicates() {
        let dual_orientation = vec![
            PathMembership {
                path_id: 1,
                multiplicity: 3,
                reversed_relative_to_group: false,
            },
            PathMembership {
                path_id: 1,
                multiplicity: 1,
                reversed_relative_to_group: true,
            },
        ];
        let encoded = encode_delta(&dual_orientation).unwrap();
        assert_eq!(decode_memberships(&encoded, 2).unwrap(), dual_orientation);
        let encoded = encode_runs(&dual_orientation).unwrap();
        assert_eq!(decode_memberships(&encoded, 2).unwrap(), dual_orientation);
        let exact_duplicate = vec![
            PathMembership {
                path_id: 1,
                multiplicity: 1,
                reversed_relative_to_group: false,
            },
            PathMembership {
                path_id: 1,
                multiplicity: 1,
                reversed_relative_to_group: false,
            },
        ];
        assert!(encode_delta(&exact_duplicate).is_err());

        let group = TraversalMembershipGroup {
            traversal_digest: [1; 16],
            occurrence_weight: 4,
            unique_path_count: 1,
            memberships: dual_orientation,
        };
        let page =
            encode_tile_membership_page(100, 200, &[9; 16], std::slice::from_ref(&group)).unwrap();
        assert_eq!(
            decode_tile_membership_page(&page, 2).unwrap().3,
            vec![group]
        );
    }

    #[test]
    fn membership_decode_rejects_non_boundary_prefixes_and_run_expansion_bombs() {
        let mut catalog = Vec::new();
        catalog.extend_from_slice(PATH_CATALOG_PAGE_MAGIC);
        put_u32(&mut catalog, PATH_MEMBERSHIP_VERSION);
        put_u32(&mut catalog, 2);
        put_u64(&mut catalog, 0);
        catalog.extend_from_slice(&[0, 2, 0xc3, 0xa9, 0, 0, 0, 0, 0, 0, 0]);
        catalog.extend_from_slice(&[1, 1, 0xa9, 0, 0, 0, 0, 0, 0, 0]);
        assert!(decode_path_catalog_page(&catalog).is_err());

        let mut outside_catalog = vec![RUN_CODEC];
        put_varint(&mut outside_catalog, 2);
        put_varint(&mut outside_catalog, 1);
        outside_catalog.push(1);
        put_varint(&mut outside_catalog, 0);
        put_varint(&mut outside_catalog, 2);
        outside_catalog.push(0);
        assert!(decode_memberships(&outside_catalog, 1).is_err());

        let mut excessive = vec![RUN_CODEC];
        put_varint(&mut excessive, MAX_PATH_MEMBERSHIPS_PER_GROUP as u64 + 1);
        assert!(decode_memberships(&excessive, u64::MAX).is_err());
    }

    #[test]
    fn membership_codec_rejects_adversarial_varints_and_records() {
        let zero_id = vec![PathMembership {
            path_id: 0,
            multiplicity: 1,
            reversed_relative_to_group: false,
        }];
        let encoded = encode_delta(&zero_id).unwrap();
        assert_eq!(decode_memberships(&encoded, 1).unwrap(), zero_id);

        assert!(decode_memberships(&[DELTA_CODEC, 0x81, 0x00], 2).is_err());
        assert!(decode_memberships(&[DELTA_CODEC, 1, 0, 0, 0], 2).is_err());
        assert!(decode_memberships(&[DELTA_CODEC, 1, 0, 1, 2], 2).is_err());
        assert!(decode_memberships(&[DELTA_CODEC, 1, 0, 1], 2).is_err());
        assert!(decode_memberships(&[DELTA_CODEC, 1, 0, 1, 0, 0], 2).is_err());
        assert!(decode_memberships(&[2, 1, 0, 1, 0], 2).is_err());

        let overflowing = TraversalMembershipGroup {
            traversal_digest: [3; 16],
            occurrence_weight: u64::MAX,
            unique_path_count: 2,
            memberships: vec![
                PathMembership {
                    path_id: 0,
                    multiplicity: u64::MAX,
                    reversed_relative_to_group: false,
                },
                PathMembership {
                    path_id: 1,
                    multiplicity: 1,
                    reversed_relative_to_group: false,
                },
            ],
        };
        assert!(encode_tile_membership_page(0, 1, &[1; 16], &[overflowing]).is_err());

        let descending = vec![
            PathMembership {
                path_id: 1,
                multiplicity: 1,
                reversed_relative_to_group: false,
            },
            PathMembership {
                path_id: 0,
                multiplicity: 1,
                reversed_relative_to_group: false,
            },
        ];
        assert_eq!(
            decode_memberships(&encode_delta(&descending).unwrap(), 2).unwrap(),
            [descending[1].clone(), descending[0].clone()]
        );
    }
}
