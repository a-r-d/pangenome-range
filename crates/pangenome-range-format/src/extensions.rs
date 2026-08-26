use crate::binary::{
    BinaryReader, count_bounded_by_bytes, invalid_data, put_u32, put_u64, usize_to_u32,
    usize_to_u64,
};
use crate::{ChunkCodec, decompress};
use std::collections::BTreeSet;
use std::io;

pub const EXTENSION_MAGIC: &[u8; 8] = b"PNGEXT01";
pub const EXTENSION_DIRECTORY_VERSION: u32 = 1;
pub const EXTENSION_DIRECTORY_HEADER_BYTES: usize = 32;
pub const EXTENSION_ENTRY_BYTES: usize = 64;
pub const MAX_EXTENSION_DIRECTORY_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionEntry {
    pub type_id: [u8; 16],
    pub required: bool,
    pub codec: ChunkCodec,
    pub offset: u64,
    pub encoded_len: u64,
    pub decoded_len: u64,
    pub integrity: [u8; 16],
}

/// Encodes a sorted v1 extension directory.
///
/// # Errors
///
/// Returns an error for invalid, unordered, duplicate, empty, oversized, or
/// non-representable extension metadata.
pub fn encode_extension_directory(entries: &[ExtensionEntry]) -> io::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(
        EXTENSION_DIRECTORY_HEADER_BYTES
            .checked_add(
                entries
                    .len()
                    .checked_mul(EXTENSION_ENTRY_BYTES)
                    .ok_or_else(|| invalid_data("extension directory size overflow"))?,
            )
            .ok_or_else(|| invalid_data("extension directory size overflow"))?,
    );
    output.extend_from_slice(EXTENSION_MAGIC);
    put_u32(&mut output, EXTENSION_DIRECTORY_VERSION);
    put_u32(&mut output, usize_to_u32(EXTENSION_ENTRY_BYTES)?);
    put_u64(&mut output, usize_to_u64(entries.len())?);
    output.extend_from_slice(&[0_u8; 8]);
    let mut previous = None;
    for entry in entries {
        if entry.type_id == [0; 16]
            || previous.is_some_and(|type_id| entry.type_id <= type_id)
            || entry.encoded_len == 0
            || entry.decoded_len == 0
        {
            return Err(invalid_data("invalid or unordered extension entry"));
        }
        previous = Some(entry.type_id);
        output.extend_from_slice(&entry.type_id);
        put_u32(&mut output, u32::from(entry.required));
        output.push(entry.codec.code());
        output.extend_from_slice(&[0_u8; 3]);
        put_u64(&mut output, entry.offset);
        put_u64(&mut output, entry.encoded_len);
        put_u64(&mut output, entry.decoded_len);
        output.extend_from_slice(&entry.integrity);
    }
    if usize_to_u64(output.len())? > MAX_EXTENSION_DIRECTORY_BYTES {
        return Err(invalid_data("extension directory exceeds its size limit"));
    }
    Ok(output)
}

/// Decodes and validates a complete extension directory.
///
/// # Errors
///
/// Returns an error for malformed headers/entries, invalid ranges, ordering or
/// size violations, unknown codecs, or trailing bytes.
pub fn decode_extension_directory(
    bytes: &[u8],
    data_offset: u64,
    object_len: u64,
) -> io::Result<Vec<ExtensionEntry>> {
    if usize_to_u64(bytes.len())? > MAX_EXTENSION_DIRECTORY_BYTES {
        return Err(invalid_data("extension directory exceeds its size limit"));
    }
    let mut reader = BinaryReader::new(bytes);
    if reader.take(8)? != EXTENSION_MAGIC
        || reader.u32()? != EXTENSION_DIRECTORY_VERSION
        || reader.u32()? as usize != EXTENSION_ENTRY_BYTES
    {
        return Err(invalid_data("invalid extension directory header"));
    }
    let count = count_bounded_by_bytes(
        reader.u64()?,
        reader.remaining(),
        EXTENSION_ENTRY_BYTES,
        "extension entries",
    )?;
    if reader.take(8)? != [0_u8; 8] {
        return Err(invalid_data(
            "extension directory reserved bytes are nonzero",
        ));
    }
    let mut entries = Vec::with_capacity(count);
    let mut types = BTreeSet::new();
    let mut previous = None;
    for _ in 0..count {
        let type_id: [u8; 16] = reader
            .take(16)?
            .try_into()
            .map_err(|_| invalid_data("invalid extension type identifier length"))?;
        let flags = reader.u32()?;
        let codec = ChunkCodec::from_code(reader.u8()?)?;
        if reader.take(3)? != [0_u8; 3] || flags & !1 != 0 {
            return Err(invalid_data("invalid extension flags or reserved bytes"));
        }
        let offset = reader.u64()?;
        let encoded_len = reader.u64()?;
        let decoded_len = reader.u64()?;
        let integrity = reader
            .take(16)?
            .try_into()
            .map_err(|_| invalid_data("invalid extension integrity length"))?;
        let end = offset
            .checked_add(encoded_len)
            .ok_or_else(|| invalid_data("extension payload range overflow"))?;
        if type_id == [0; 16]
            || previous.is_some_and(|previous| type_id <= previous)
            || !types.insert(type_id)
            || encoded_len == 0
            || decoded_len == 0
            || offset < data_offset
            || end > object_len
        {
            return Err(invalid_data("invalid extension entry"));
        }
        previous = Some(type_id);
        entries.push(ExtensionEntry {
            type_id,
            required: flags & 1 != 0,
            codec,
            offset,
            encoded_len,
            decoded_len,
            integrity,
        });
    }
    reader.finish()?;
    Ok(entries)
}

/// Verifies and decodes one extension payload.
///
/// # Errors
///
/// Returns an error for an encoded-length or BLAKE3 mismatch, invalid codec
/// framing, decompression failure, or decoded-length mismatch.
pub fn validate_extension_payload(entry: &ExtensionEntry, encoded: &[u8]) -> io::Result<Vec<u8>> {
    if usize_to_u64(encoded.len())? != entry.encoded_len {
        return Err(invalid_data("extension encoded length mismatch"));
    }
    let digest = blake3::hash(encoded);
    if digest.as_bytes()[..16] != entry.integrity {
        return Err(invalid_data("extension integrity mismatch"));
    }
    decompress(entry.codec, encoded, entry.decoded_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compress;

    #[test]
    fn directory_and_payload_integrity_round_trip() {
        let raw = b"archive provenance";
        let encoded = compress(ChunkCodec::Zstd3, raw).unwrap();
        let digest = blake3::hash(&encoded);
        let entry = ExtensionEntry {
            type_id: *b"provenance-v1---",
            required: false,
            codec: ChunkCodec::Zstd3,
            offset: 10_000,
            encoded_len: encoded.len() as u64,
            decoded_len: raw.len() as u64,
            integrity: digest.as_bytes()[..16].try_into().unwrap(),
        };
        let directory = encode_extension_directory(std::slice::from_ref(&entry)).unwrap();
        assert_eq!(
            decode_extension_directory(&directory, 9_000, 20_000).unwrap(),
            vec![entry.clone()]
        );
        assert_eq!(validate_extension_payload(&entry, &encoded).unwrap(), raw);
        let mut corrupt = encoded;
        corrupt[0] ^= 1;
        assert!(validate_extension_payload(&entry, &corrupt).is_err());
    }
}
