use crate::binary::{BinaryReader, invalid_data, put_string, put_u32, put_u64, usize_to_u64};
use crate::{CONSTRUCTION_CONTEXT, ChunkCodec};
use std::io;

pub const ARCHIVE_METADATA_TYPE_ID: [u8; 16] = *b"archive-meta-v1-";
pub const ARCHIVE_METADATA_MAGIC: &[u8; 8] = b"PNGMET01";
pub const ARCHIVE_METADATA_VERSION: u32 = 1;
pub const ARCHIVE_METADATA_HEADER_BYTES: u32 = 112;
pub const MAX_ARCHIVE_METADATA_BYTES: u64 = 1024 * 1024;
const HAPLOTYPE_SEMANTICS: u8 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveMetadata {
    pub source_gbz_bytes: u64,
    pub source_gbz_sha256: [u8; 32],
    pub encoder_package_version: String,
    pub format_implementation: String,
    pub regional_window_size: u64,
    pub construction_context: u64,
    pub payload_codec: ChunkCodec,
    pub haplotype_semantics: String,
    pub reference_sample: Option<String>,
    pub reference_assembly: Option<String>,
    pub dataset_title: Option<String>,
    pub dataset_description: Option<String>,
    pub source_uri: Option<String>,
    pub annotation_filename: Option<String>,
    pub annotation_sha256: Option<[u8; 32]>,
    pub annotation_release: Option<String>,
    pub annotation_assembly: Option<String>,
}

/// Encodes deterministic archive provenance using the registered v1 schema.
///
/// # Errors
///
/// Returns an error when required fields are invalid, strings cannot be
/// length-encoded, or the encoded metadata exceeds the v1 size limit.
pub fn encode_archive_metadata(value: &ArchiveMetadata) -> io::Result<Vec<u8>> {
    validate(value)?;
    let mut output = Vec::new();
    output.extend_from_slice(ARCHIVE_METADATA_MAGIC);
    put_u32(&mut output, ARCHIVE_METADATA_VERSION);
    put_u32(&mut output, ARCHIVE_METADATA_HEADER_BYTES);
    put_u64(&mut output, value.source_gbz_bytes);
    output.extend_from_slice(&value.source_gbz_sha256);
    put_u64(&mut output, value.regional_window_size);
    put_u64(&mut output, value.construction_context);
    output.push(value.payload_codec.code());
    output.push(HAPLOTYPE_SEMANTICS);
    output.push(u8::from(value.annotation_sha256.is_some()));
    output.extend_from_slice(&[0_u8; 5]);
    output.extend_from_slice(&value.annotation_sha256.unwrap_or([0; 32]));
    for field in [
        Some(value.encoder_package_version.as_str()),
        Some(value.format_implementation.as_str()),
        value.reference_sample.as_deref(),
        value.reference_assembly.as_deref(),
        value.dataset_title.as_deref(),
        value.dataset_description.as_deref(),
        value.source_uri.as_deref(),
        value.annotation_filename.as_deref(),
        value.annotation_release.as_deref(),
        value.annotation_assembly.as_deref(),
    ] {
        put_string(&mut output, field.unwrap_or_default())?;
    }
    if usize_to_u64(output.len())? > MAX_ARCHIVE_METADATA_BYTES {
        return Err(invalid_data("archive metadata exceeds its size limit"));
    }
    Ok(output)
}

/// Decodes and validates deterministic archive provenance.
///
/// # Errors
///
/// Returns an error for malformed, unsupported, truncated, trailing, or
/// resource-limit-violating metadata.
pub fn decode_archive_metadata(bytes: &[u8]) -> io::Result<ArchiveMetadata> {
    if usize_to_u64(bytes.len())? > MAX_ARCHIVE_METADATA_BYTES {
        return Err(invalid_data("archive metadata exceeds its size limit"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != ARCHIVE_METADATA_MAGIC
        || reader.u32()? != ARCHIVE_METADATA_VERSION
        || reader.u32()? != ARCHIVE_METADATA_HEADER_BYTES
    {
        return Err(invalid_data("invalid archive metadata header"));
    }
    let source_gbz_bytes = reader.u64()?;
    let source_gbz_sha256 = reader
        .take(32)?
        .try_into()
        .map_err(|_| invalid_data("invalid source GBZ checksum"))?;
    let regional_window_size = reader.u64()?;
    let construction_context = reader.u64()?;
    let payload_codec = ChunkCodec::from_code(reader.u8()?)?;
    let semantics = reader.u8()?;
    let annotation_present = reader.u8()?;
    if reader.take(5)? != [0_u8; 5] || annotation_present > 1 {
        return Err(invalid_data(
            "invalid archive metadata flags or reserved bytes",
        ));
    }
    let annotation_bytes: [u8; 32] = reader
        .take(32)?
        .try_into()
        .map_err(|_| invalid_data("invalid annotation checksum"))?;
    let encoder_package_version = reader.string()?;
    let format_implementation = reader.string()?;
    let reference_sample = optional_string(reader.string()?);
    let reference_assembly = optional_string(reader.string()?);
    let dataset_title = optional_string(reader.string()?);
    let dataset_description = optional_string(reader.string()?);
    let source_uri = optional_string(reader.string()?);
    let annotation_filename = optional_string(reader.string()?);
    let annotation_release = optional_string(reader.string()?);
    let annotation_assembly = optional_string(reader.string()?);
    reader.finish()?;
    if semantics != HAPLOTYPE_SEMANTICS {
        return Err(invalid_data("invalid archive metadata haplotype semantics"));
    }
    let annotation_sha256 = match annotation_present {
        0 if annotation_bytes == [0; 32] => None,
        1 if annotation_bytes != [0; 32] => Some(annotation_bytes),
        _ => return Err(invalid_data("invalid archive annotation checksum presence")),
    };
    let value = ArchiveMetadata {
        source_gbz_bytes,
        source_gbz_sha256,
        encoder_package_version,
        format_implementation,
        regional_window_size,
        construction_context,
        payload_codec,
        haplotype_semantics: "anonymous-distinct-weighted-tile-paths".into(),
        reference_sample,
        reference_assembly,
        dataset_title,
        dataset_description,
        source_uri,
        annotation_filename,
        annotation_sha256,
        annotation_release,
        annotation_assembly,
    };
    validate(&value)?;
    Ok(value)
}

fn optional_string(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn validate(value: &ArchiveMetadata) -> io::Result<()> {
    if value.source_gbz_bytes == 0
        || value.source_gbz_sha256 == [0; 32]
        || value.encoder_package_version.is_empty()
        || value.format_implementation.is_empty()
        || value.regional_window_size == 0
        || value.construction_context != CONSTRUCTION_CONTEXT
        || value.haplotype_semantics != "anonymous-distinct-weighted-tile-paths"
        || value.annotation_sha256.is_some() != value.annotation_filename.is_some()
        || value.annotation_sha256 == Some([0; 32])
        || (value.annotation_filename.is_none()
            && (value.annotation_release.is_some() || value.annotation_assembly.is_some()))
        || [
            value.reference_sample.as_deref(),
            value.reference_assembly.as_deref(),
            value.dataset_title.as_deref(),
            value.dataset_description.as_deref(),
            value.source_uri.as_deref(),
            value.annotation_filename.as_deref(),
            value.annotation_release.as_deref(),
            value.annotation_assembly.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(str::is_empty)
    {
        return Err(invalid_data("invalid archive metadata"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_metadata_round_trips_and_preserves_absence() {
        let value = ArchiveMetadata {
            source_gbz_bytes: 73_920,
            source_gbz_sha256: [7; 32],
            encoder_package_version: "0.1.0".into(),
            format_implementation: "pangenome-range-rust-v1".into(),
            regional_window_size: 16_384,
            construction_context: CONSTRUCTION_CONTEXT,
            payload_codec: ChunkCodec::Zstd3,
            haplotype_semantics: "anonymous-distinct-weighted-tile-paths".into(),
            reference_sample: Some("GRCh38".into()),
            reference_assembly: Some("GRCh38.p14".into()),
            dataset_title: Some("Fixture".into()),
            dataset_description: None,
            source_uri: Some("https://example.test/source.gbz".into()),
            annotation_filename: Some("genes.gff3".into()),
            annotation_sha256: Some([9; 32]),
            annotation_release: Some("v50".into()),
            annotation_assembly: Some("GRCh38.p14".into()),
        };
        let bytes = encode_archive_metadata(&value).unwrap();
        assert_eq!(decode_archive_metadata(&bytes).unwrap(), value);
        assert!(decode_archive_metadata(&bytes[..bytes.len() - 1]).is_err());
    }
}
