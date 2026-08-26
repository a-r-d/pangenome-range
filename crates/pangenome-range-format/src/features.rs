use crate::binary::{
    BinaryReader, count_bounded_by_bytes, invalid_data, put_string, put_u32, put_u64, usize_to_u32,
    usize_to_u64,
};
use crate::{ChunkCodec, decompress};
use std::io;

pub const NAMED_LOCI_TYPE_ID: [u8; 16] = *b"named-loci-v1---";
pub const SUMMARY_PYRAMID_TYPE_ID: [u8; 16] = *b"summary-pyr-v1--";
pub const NAMED_LOCI_MAGIC: &[u8; 8] = b"PNGLOC01";
pub const NAMED_LOCI_PAGE_MAGIC: &[u8; 8] = b"PNGLPG01";
pub const SUMMARY_PYRAMID_MAGIC: &[u8; 8] = b"PNGSUM01";
pub const SUMMARY_PAGE_MAGIC: &[u8; 8] = b"PNGSMP01";
pub const FEATURE_EXTENSION_VERSION: u32 = 1;
pub const SUMMARY_BIN_BYTES: usize = 8 * 8;
pub const MAX_FEATURE_DESCRIPTOR_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_FEATURE_PAGE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_LOCUS_PAGES: usize = 65_536;
pub const MAX_LOCUS_RECORDS_PER_PAGE: usize = 65_536;
pub const MAX_SUMMARY_SERIES: usize = 65_536;
pub const MAX_SUMMARY_BINS_PER_PAGE: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionPage {
    pub offset: u64,
    pub encoded_len: u64,
    pub decoded_len: u64,
    pub codec: ChunkCodec,
    pub integrity: [u8; 16],
}

impl ExtensionPage {
    /// Validates the stored child range owned by a known extension.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, oversized, overflowing, or out-of-object ranges.
    pub fn validate(&self, data_offset: u64, object_len: u64) -> io::Result<()> {
        let end = self
            .offset
            .checked_add(self.encoded_len)
            .ok_or_else(|| invalid_data("extension page range overflow"))?;
        if self.encoded_len == 0
            || self.decoded_len == 0
            || self.decoded_len > MAX_FEATURE_PAGE_BYTES
            || self.offset < data_offset
            || end > object_len
        {
            return Err(invalid_data("invalid extension page range"));
        }
        Ok(())
    }
}

/// Verifies and decodes one independently addressed known-extension page.
///
/// # Errors
///
/// Returns an error for a length or integrity mismatch, invalid compression,
/// or a decoded-length mismatch.
pub fn validate_extension_page(page: &ExtensionPage, encoded: &[u8]) -> io::Result<Vec<u8>> {
    if usize_to_u64(encoded.len())? != page.encoded_len {
        return Err(invalid_data("extension page encoded length mismatch"));
    }
    let digest = blake3::hash(encoded);
    if digest.as_bytes()[..16] != page.integrity {
        return Err(invalid_data("extension page integrity mismatch"));
    }
    decompress(page.codec, encoded, page.decoded_len)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocusRecord {
    pub normalized_key: String,
    pub matched_name: String,
    pub display_name: String,
    pub stable_id: String,
    pub feature_type: String,
    pub sample: String,
    pub contig: String,
    pub start: u64,
    pub end: u64,
    pub strand: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocusPageDescriptor {
    pub first_key: String,
    pub last_key: String,
    pub record_count: u64,
    pub storage: ExtensionPage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedLociDescriptor {
    pub annotation_sha256: [u8; 32],
    pub annotation_name: String,
    pub record_count: u64,
    pub pages: Vec<LocusPageDescriptor>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SummaryBin {
    pub covered_bases: u64,
    pub tile_count: u64,
    pub encoded_bytes: u64,
    pub decoded_bytes: u64,
    pub node_records: u64,
    pub edge_records: u64,
    pub gbwt_records: u64,
    pub occurrences: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummarySeriesDescriptor {
    pub manifest_index: u32,
    pub level: u32,
    pub bin_span: u64,
    pub first_bin_start: u64,
    pub bin_count: u64,
    pub storage: ExtensionPage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryPyramidDescriptor {
    pub base_bin_span: u64,
    pub series: Vec<SummarySeriesDescriptor>,
}

/// Applies the format's locale-independent locus-key normalization.
#[must_use]
pub fn normalize_locus_key(value: &str) -> String {
    value
        .trim_matches(|character: char| character.is_ascii_whitespace())
        .chars()
        .map(|character| {
            if character.is_ascii_uppercase() {
                character.to_ascii_lowercase()
            } else {
                character
            }
        })
        .collect()
}

/// Encodes the small named-locus descriptor addressed by the extension directory.
///
/// # Errors
///
/// Returns an error for invalid counts, key ordering, totals, or size limits.
pub fn encode_named_loci_descriptor(value: &NamedLociDescriptor) -> io::Result<Vec<u8>> {
    if (value.record_count == 0) != value.pages.is_empty() || value.pages.len() > MAX_LOCUS_PAGES {
        return Err(invalid_data("invalid named-locus descriptor counts"));
    }
    let mut output = Vec::new();
    output.extend_from_slice(NAMED_LOCI_MAGIC);
    put_u32(&mut output, FEATURE_EXTENSION_VERSION);
    put_u32(&mut output, usize_to_u32(value.pages.len())?);
    put_u64(&mut output, value.record_count);
    output.extend_from_slice(&value.annotation_sha256);
    put_string(&mut output, &value.annotation_name)?;
    let mut previous_last: Option<&str> = None;
    let mut total = 0_u64;
    for page in &value.pages {
        if page.first_key.is_empty()
            || page.last_key < page.first_key
            || previous_last.is_some_and(|previous| page.first_key.as_str() <= previous)
            || page.record_count == 0
        {
            return Err(invalid_data("invalid named-locus page ordering"));
        }
        previous_last = Some(&page.last_key);
        total = total
            .checked_add(page.record_count)
            .ok_or_else(|| invalid_data("named-locus record count overflow"))?;
        put_string(&mut output, &page.first_key)?;
        put_string(&mut output, &page.last_key)?;
        put_u64(&mut output, page.record_count);
        put_extension_page(&mut output, &page.storage);
    }
    if total != value.record_count || usize_to_u64(output.len())? > MAX_FEATURE_DESCRIPTOR_BYTES {
        return Err(invalid_data("invalid named-locus descriptor size or total"));
    }
    Ok(output)
}

/// Decodes and validates a complete named-locus descriptor.
///
/// # Errors
///
/// Returns an error for malformed bytes, invalid counts/order/ranges, unsafe
/// sizes, or trailing data.
pub fn decode_named_loci_descriptor(
    bytes: &[u8],
    data_offset: u64,
    object_len: u64,
) -> io::Result<NamedLociDescriptor> {
    if usize_to_u64(bytes.len())? > MAX_FEATURE_DESCRIPTOR_BYTES {
        return Err(invalid_data(
            "named-locus descriptor exceeds its size limit",
        ));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != NAMED_LOCI_MAGIC || reader.u32()? != FEATURE_EXTENSION_VERSION {
        return Err(invalid_data("invalid named-locus descriptor header"));
    }
    let page_count = usize::try_from(reader.u32()?)
        .map_err(|_| invalid_data("named-locus page count does not fit usize"))?;
    if page_count > MAX_LOCUS_PAGES {
        return Err(invalid_data("named-locus page count exceeds its limit"));
    }
    let record_count = reader.u64()?;
    let annotation_sha256 = reader
        .take(32)?
        .try_into()
        .map_err(|_| invalid_data("invalid annotation checksum"))?;
    let annotation_name = reader.string()?;
    let mut pages = Vec::with_capacity(page_count);
    let mut previous_last: Option<String> = None;
    let mut total = 0_u64;
    for _ in 0..page_count {
        let first_key = reader.string()?;
        let last_key = reader.string()?;
        let page_records = reader.u64()?;
        let storage = read_extension_page(&mut reader)?;
        storage.validate(data_offset, object_len)?;
        if first_key.is_empty()
            || last_key < first_key
            || previous_last
                .as_ref()
                .is_some_and(|previous| first_key <= *previous)
            || page_records == 0
        {
            return Err(invalid_data("invalid named-locus page ordering"));
        }
        total = total
            .checked_add(page_records)
            .ok_or_else(|| invalid_data("named-locus record count overflow"))?;
        previous_last = Some(last_key.clone());
        pages.push(LocusPageDescriptor {
            first_key,
            last_key,
            record_count: page_records,
            storage,
        });
    }
    reader.finish()?;
    if (record_count == 0) != pages.is_empty() || total != record_count {
        return Err(invalid_data("named-locus descriptor count mismatch"));
    }
    Ok(NamedLociDescriptor {
        annotation_sha256,
        annotation_name,
        record_count,
        pages,
    })
}

/// Encodes one independently stored, sorted named-locus leaf page.
///
/// # Errors
///
/// Returns an error for an empty or oversized page, invalid records, ordering,
/// or non-representable lengths.
pub fn encode_locus_page(records: &[LocusRecord]) -> io::Result<Vec<u8>> {
    if records.is_empty() || records.len() > MAX_LOCUS_RECORDS_PER_PAGE {
        return Err(invalid_data("invalid named-locus page record count"));
    }
    let mut output = Vec::new();
    output.extend_from_slice(NAMED_LOCI_PAGE_MAGIC);
    put_u32(&mut output, FEATURE_EXTENSION_VERSION);
    put_u32(&mut output, usize_to_u32(records.len())?);
    let mut previous: Option<(&str, &str, &str, u64, u64, &str)> = None;
    for record in records {
        validate_locus_record(record)?;
        let key = (
            record.normalized_key.as_str(),
            record.sample.as_str(),
            record.contig.as_str(),
            record.start,
            record.end,
            record.stable_id.as_str(),
        );
        if previous.is_some_and(|previous| key < previous) {
            return Err(invalid_data("named-locus records are not sorted"));
        }
        previous = Some(key);
        put_string(&mut output, &record.normalized_key)?;
        put_string(&mut output, &record.matched_name)?;
        put_string(&mut output, &record.display_name)?;
        put_string(&mut output, &record.stable_id)?;
        put_string(&mut output, &record.feature_type)?;
        put_string(&mut output, &record.sample)?;
        put_string(&mut output, &record.contig)?;
        put_u64(&mut output, record.start);
        put_u64(&mut output, record.end);
        output.push(record.strand);
        output.extend_from_slice(&[0_u8; 7]);
    }
    if usize_to_u64(output.len())? > MAX_FEATURE_PAGE_BYTES {
        return Err(invalid_data("named-locus page exceeds its size limit"));
    }
    Ok(output)
}

/// Decodes one complete named-locus leaf page.
///
/// # Errors
///
/// Returns an error for malformed bytes, unsafe counts, invalid records or
/// ordering, or trailing data.
pub fn decode_locus_page(bytes: &[u8]) -> io::Result<Vec<LocusRecord>> {
    if usize_to_u64(bytes.len())? > MAX_FEATURE_PAGE_BYTES {
        return Err(invalid_data("named-locus page exceeds its size limit"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != NAMED_LOCI_PAGE_MAGIC || reader.u32()? != FEATURE_EXTENSION_VERSION {
        return Err(invalid_data("invalid named-locus page header"));
    }
    let count = count_bounded_by_bytes(
        u64::from(reader.u32()?),
        reader.remaining(),
        72,
        "named-locus records",
    )?;
    if count == 0 || count > MAX_LOCUS_RECORDS_PER_PAGE {
        return Err(invalid_data("invalid named-locus page record count"));
    }
    let mut records = Vec::with_capacity(count);
    let mut previous: Option<(String, String, String, u64, u64, String)> = None;
    for _ in 0..count {
        let record = LocusRecord {
            normalized_key: reader.string()?,
            matched_name: reader.string()?,
            display_name: reader.string()?,
            stable_id: reader.string()?,
            feature_type: reader.string()?,
            sample: reader.string()?,
            contig: reader.string()?,
            start: reader.u64()?,
            end: reader.u64()?,
            strand: reader.u8()?,
        };
        if reader.take(7)? != [0_u8; 7] {
            return Err(invalid_data(
                "named-locus record reserved bytes are nonzero",
            ));
        }
        validate_locus_record(&record)?;
        let key = (
            record.normalized_key.clone(),
            record.sample.clone(),
            record.contig.clone(),
            record.start,
            record.end,
            record.stable_id.clone(),
        );
        if previous.as_ref().is_some_and(|previous| key < *previous) {
            return Err(invalid_data("named-locus records are not sorted"));
        }
        previous = Some(key);
        records.push(record);
    }
    reader.finish()?;
    Ok(records)
}

/// Encodes the summary-pyramid descriptor addressed by the extension directory.
///
/// # Errors
///
/// Returns an error for invalid dimensions, ordering, size limits, or
/// non-representable counts.
pub fn encode_summary_descriptor(value: &SummaryPyramidDescriptor) -> io::Result<Vec<u8>> {
    if value.base_bin_span == 0
        || value.series.is_empty()
        || value.series.len() > MAX_SUMMARY_SERIES
    {
        return Err(invalid_data("invalid summary descriptor counts"));
    }
    let mut output = Vec::new();
    output.extend_from_slice(SUMMARY_PYRAMID_MAGIC);
    put_u32(&mut output, FEATURE_EXTENSION_VERSION);
    put_u32(&mut output, usize_to_u32(value.series.len())?);
    put_u64(&mut output, value.base_bin_span);
    output.extend_from_slice(&[0_u8; 8]);
    let mut previous = None;
    for series in &value.series {
        let key = (series.manifest_index, series.level);
        if previous.is_some_and(|previous| key <= previous)
            || series.bin_span == 0
            || series.bin_count == 0
            || series.first_bin_start % series.bin_span != 0
        {
            return Err(invalid_data("invalid summary series descriptor"));
        }
        previous = Some(key);
        put_u32(&mut output, series.manifest_index);
        put_u32(&mut output, series.level);
        put_u64(&mut output, series.bin_span);
        put_u64(&mut output, series.first_bin_start);
        put_u64(&mut output, series.bin_count);
        put_extension_page(&mut output, &series.storage);
    }
    if usize_to_u64(output.len())? > MAX_FEATURE_DESCRIPTOR_BYTES {
        return Err(invalid_data("summary descriptor exceeds its size limit"));
    }
    Ok(output)
}

/// Decodes and validates a complete summary-pyramid descriptor.
///
/// # Errors
///
/// Returns an error for malformed bytes, invalid dimensions/order/ranges,
/// unsafe sizes, or trailing data.
pub fn decode_summary_descriptor(
    bytes: &[u8],
    data_offset: u64,
    object_len: u64,
) -> io::Result<SummaryPyramidDescriptor> {
    if usize_to_u64(bytes.len())? > MAX_FEATURE_DESCRIPTOR_BYTES {
        return Err(invalid_data("summary descriptor exceeds its size limit"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != SUMMARY_PYRAMID_MAGIC || reader.u32()? != FEATURE_EXTENSION_VERSION {
        return Err(invalid_data("invalid summary descriptor header"));
    }
    let count = usize::try_from(reader.u32()?)
        .map_err(|_| invalid_data("summary series count does not fit usize"))?;
    let base_bin_span = reader.u64()?;
    if reader.take(8)? != [0_u8; 8]
        || count == 0
        || count > MAX_SUMMARY_SERIES
        || base_bin_span == 0
    {
        return Err(invalid_data("invalid summary descriptor counts"));
    }
    let mut series = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let manifest_index = reader.u32()?;
        let level = reader.u32()?;
        let bin_span = reader.u64()?;
        let first_bin_start = reader.u64()?;
        let bin_count = reader.u64()?;
        let storage = read_extension_page(&mut reader)?;
        storage.validate(data_offset, object_len)?;
        let key = (manifest_index, level);
        if previous.is_some_and(|previous| key <= previous)
            || bin_span == 0
            || bin_count == 0
            || first_bin_start % bin_span != 0
        {
            return Err(invalid_data("invalid summary series descriptor"));
        }
        previous = Some(key);
        series.push(SummarySeriesDescriptor {
            manifest_index,
            level,
            bin_span,
            first_bin_start,
            bin_count,
            storage,
        });
    }
    reader.finish()?;
    Ok(SummaryPyramidDescriptor {
        base_bin_span,
        series,
    })
}

/// Encodes one fixed-width summary series page.
///
/// # Errors
///
/// Returns an error for invalid dimensions, excessive size, or a
/// non-representable bin count.
pub fn encode_summary_page(
    manifest_index: u32,
    level: u32,
    bin_span: u64,
    first_bin_start: u64,
    bins: &[SummaryBin],
) -> io::Result<Vec<u8>> {
    if bin_span == 0
        || first_bin_start % bin_span != 0
        || bins.is_empty()
        || bins.len() > MAX_SUMMARY_BINS_PER_PAGE
    {
        return Err(invalid_data("invalid summary page dimensions"));
    }
    let mut output = Vec::with_capacity(48 + bins.len() * SUMMARY_BIN_BYTES);
    output.extend_from_slice(SUMMARY_PAGE_MAGIC);
    put_u32(&mut output, FEATURE_EXTENSION_VERSION);
    put_u32(&mut output, usize_to_u32(SUMMARY_BIN_BYTES)?);
    put_u32(&mut output, manifest_index);
    put_u32(&mut output, level);
    put_u64(&mut output, bin_span);
    put_u64(&mut output, first_bin_start);
    put_u64(&mut output, usize_to_u64(bins.len())?);
    for bin in bins {
        put_summary_bin(&mut output, bin);
    }
    if usize_to_u64(output.len())? > MAX_FEATURE_PAGE_BYTES {
        return Err(invalid_data("summary page exceeds its size limit"));
    }
    Ok(output)
}

/// Decodes one fixed-width summary series page.
///
/// # Errors
///
/// Returns an error for malformed bytes, descriptor disagreement, unsafe bin
/// counts, excessive size, or trailing data.
pub fn decode_summary_page(
    bytes: &[u8],
    expected: &SummarySeriesDescriptor,
) -> io::Result<Vec<SummaryBin>> {
    if usize_to_u64(bytes.len())? > MAX_FEATURE_PAGE_BYTES {
        return Err(invalid_data("summary page exceeds its size limit"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != SUMMARY_PAGE_MAGIC
        || reader.u32()? != FEATURE_EXTENSION_VERSION
        || reader.u32()? as usize != SUMMARY_BIN_BYTES
        || reader.u32()? != expected.manifest_index
        || reader.u32()? != expected.level
        || reader.u64()? != expected.bin_span
        || reader.u64()? != expected.first_bin_start
    {
        return Err(invalid_data("summary page differs from its descriptor"));
    }
    let count = count_bounded_by_bytes(
        reader.u64()?,
        reader.remaining(),
        SUMMARY_BIN_BYTES,
        "summary bins",
    )?;
    if count == 0 || count > MAX_SUMMARY_BINS_PER_PAGE || count as u64 != expected.bin_count {
        return Err(invalid_data("summary page bin count mismatch"));
    }
    let mut bins = Vec::with_capacity(count);
    for _ in 0..count {
        bins.push(SummaryBin {
            covered_bases: reader.u64()?,
            tile_count: reader.u64()?,
            encoded_bytes: reader.u64()?,
            decoded_bytes: reader.u64()?,
            node_records: reader.u64()?,
            edge_records: reader.u64()?,
            gbwt_records: reader.u64()?,
            occurrences: reader.u64()?,
        });
    }
    reader.finish()?;
    Ok(bins)
}

fn validate_locus_record(record: &LocusRecord) -> io::Result<()> {
    if record.normalized_key.is_empty()
        || record.normalized_key != normalize_locus_key(&record.matched_name)
        || record.display_name.is_empty()
        || record.stable_id.is_empty()
        || record.feature_type.is_empty()
        || record.sample.is_empty()
        || record.contig.is_empty()
        || record.start >= record.end
        || !matches!(record.strand, 0..=2)
    {
        return Err(invalid_data("invalid named-locus record"));
    }
    Ok(())
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
    let codec = ChunkCodec::from_code(reader.u8()?)?;
    if reader.take(7)? != [0_u8; 7] {
        return Err(invalid_data("extension page reserved bytes are nonzero"));
    }
    let integrity = reader
        .take(16)?
        .try_into()
        .map_err(|_| invalid_data("invalid extension page integrity length"))?;
    Ok(ExtensionPage {
        offset,
        encoded_len,
        decoded_len,
        codec,
        integrity,
    })
}

fn put_summary_bin(output: &mut Vec<u8>, bin: &SummaryBin) {
    put_u64(output, bin.covered_bases);
    put_u64(output, bin.tile_count);
    put_u64(output, bin.encoded_bytes);
    put_u64(output, bin.decoded_bytes);
    put_u64(output, bin.node_records);
    put_u64(output, bin.edge_records);
    put_u64(output, bin.gbwt_records);
    put_u64(output, bin.occurrences);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(offset: u64) -> ExtensionPage {
        ExtensionPage {
            offset,
            encoded_len: 100,
            decoded_len: 200,
            codec: ChunkCodec::Zstd3,
            integrity: [7; 16],
        }
    }

    #[test]
    fn named_loci_round_trip() {
        let records = vec![LocusRecord {
            normalized_key: "brca1".into(),
            matched_name: "BRCA1".into(),
            display_name: "BRCA1".into(),
            stable_id: "gene:1".into(),
            feature_type: "gene".into(),
            sample: "GRCh38".into(),
            contig: "chr17".into(),
            start: 100,
            end: 200,
            strand: 2,
        }];
        let encoded = encode_locus_page(&records).unwrap();
        assert_eq!(decode_locus_page(&encoded).unwrap(), records);
        let descriptor = NamedLociDescriptor {
            annotation_sha256: [3; 32],
            annotation_name: "genes.gff3".into(),
            record_count: 1,
            pages: vec![LocusPageDescriptor {
                first_key: "brca1".into(),
                last_key: "brca1".into(),
                record_count: 1,
                storage: page(10_000),
            }],
        };
        let encoded = encode_named_loci_descriptor(&descriptor).unwrap();
        assert_eq!(
            decode_named_loci_descriptor(&encoded, 9_000, 20_000).unwrap(),
            descriptor
        );
    }

    #[test]
    fn summary_round_trip() {
        let bins = vec![SummaryBin {
            covered_bases: 100,
            tile_count: 1,
            encoded_bytes: 12,
            decoded_bytes: 20,
            node_records: 4,
            edge_records: 3,
            gbwt_records: 4,
            occurrences: 9,
        }];
        let series = SummarySeriesDescriptor {
            manifest_index: 0,
            level: 0,
            bin_span: 1_048_576,
            first_bin_start: 0,
            bin_count: 1,
            storage: page(10_000),
        };
        let encoded = encode_summary_page(0, 0, 1_048_576, 0, &bins).unwrap();
        assert_eq!(decode_summary_page(&encoded, &series).unwrap(), bins);
        let descriptor = SummaryPyramidDescriptor {
            base_bin_span: 1_048_576,
            series: vec![series],
        };
        let encoded = encode_summary_descriptor(&descriptor).unwrap();
        assert_eq!(
            decode_summary_descriptor(&encoded, 9_000, 20_000).unwrap(),
            descriptor
        );
    }
}
