use crate::binary::{invalid_data, u64_to_usize, usize_to_u64};
use crate::{
    DIRECTORY_PAGE_BYTES, FileRangeSource, RangeSource, bootstrap, decode_directory_page,
    directory_page_offset,
};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityPlacementEstimate {
    pub id: &'static str,
    pub digest_bits: u32,
    pub placement: &'static str,
    pub directory_entry_bytes: Option<u64>,
    pub directory_entries_per_page: Option<u64>,
    pub dense_bucket_pages_over_capacity: u64,
    pub modeled_archive_growth_bytes: u64,
    pub modeled_index_growth_bytes: u64,
    pub extra_index_reads_per_query: &'static str,
    pub detects_before_decompression: bool,
    pub limitation: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityEvaluation {
    pub schema_version: u32,
    pub archive_path: PathBuf,
    pub archive_bytes: u64,
    pub directory_pages: u64,
    pub directory_entries: u64,
    pub physical_payloads: u64,
    pub compressed_payload_bytes: u64,
    pub maximum_entries_in_one_page: u64,
    pub directory_scan_wall_ms: f64,
    pub payload_read_wall_ms: f64,
    pub blake3_wall_ms: f64,
    pub blake3_mebibytes_per_second: f64,
    pub placements: Vec<IntegrityPlacementEstimate>,
}

/// Scans an archive once and models candidate integrity placements.
///
/// # Errors
///
/// Returns an error for archive I/O/corruption, invalid directory or payload
/// ranges, unsafe integer conversion, or counter overflow.
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub fn evaluate_integrity_options(path: &Path) -> io::Result<IntegrityEvaluation> {
    let source = FileRangeSource::open(path)?;
    let archive_bytes = source.len()?;
    let directory_started = Instant::now();
    let metadata = bootstrap(&source)?;
    let mut pages = 0_u64;
    let mut entries = 0_u64;
    let mut maximum_entries = 0_u64;
    let mut page_occupancies = Vec::new();
    let mut physical = BTreeSet::new();
    for manifest in &metadata.root.manifests {
        for bucket in 0..manifest.page_count {
            let page = source.read_range(
                directory_page_offset(manifest, bucket)?,
                DIRECTORY_PAGE_BYTES,
            )?;
            let decoded = decode_directory_page(&page, manifest, bucket)?;
            let count = usize_to_u64(decoded.len())?;
            pages = pages
                .checked_add(1)
                .ok_or_else(|| invalid_data("integrity page count overflow"))?;
            entries = entries
                .checked_add(count)
                .ok_or_else(|| invalid_data("integrity entry count overflow"))?;
            maximum_entries = maximum_entries.max(count);
            page_occupancies.push(count);
            for entry in decoded {
                physical.insert((
                    entry.offset,
                    entry.compressed_len,
                    entry.uncompressed_len,
                    entry.codec.code(),
                ));
            }
        }
    }
    let directory_scan_wall_ms = directory_started.elapsed().as_secs_f64() * 1_000.0;
    let mut payload_read_wall_ms = 0.0;
    let mut blake3_wall_ms = 0.0;
    let mut compressed_payload_bytes = 0_u64;
    for &(offset, encoded_len, _, _) in &physical {
        let read_started = Instant::now();
        let encoded = source.read_range(offset, u64_to_usize(encoded_len)?)?;
        payload_read_wall_ms += read_started.elapsed().as_secs_f64() * 1_000.0;
        compressed_payload_bytes = compressed_payload_bytes
            .checked_add(encoded_len)
            .ok_or_else(|| invalid_data("integrity payload byte count overflow"))?;
        let hash_started = Instant::now();
        std::hint::black_box(blake3::hash(&encoded));
        blake3_wall_ms += hash_started.elapsed().as_secs_f64() * 1_000.0;
    }
    let physical_payloads = usize_to_u64(physical.len())?;
    let mib = compressed_payload_bytes as f64 / (1024.0 * 1024.0);
    let blake3_mebibytes_per_second = if blake3_wall_ms == 0.0 {
        0.0
    } else {
        mib / (blake3_wall_ms / 1_000.0)
    };
    let dense = |capacity: u64| {
        usize_to_u64(
            page_occupancies
                .iter()
                .filter(|&&count| count > capacity)
                .count(),
        )
        .unwrap_or(u64::MAX)
    };
    let extension_growth = |digest_bytes: u64| {
        96_u64.saturating_add(physical_payloads.saturating_mul(8 + digest_bytes))
    };
    let placements = vec![
        IntegrityPlacementEstimate {
            id: "none",
            digest_bits: 0,
            placement: "none",
            directory_entry_bytes: Some(40),
            directory_entries_per_page: Some(102),
            dense_bucket_pages_over_capacity: 0,
            modeled_archive_growth_bytes: 0,
            modeled_index_growth_bytes: 0,
            extra_index_reads_per_query: "0",
            detects_before_decompression: false,
            limitation: "relies on exact lengths, zstd validation, structural decode, object identity, and external whole-object SHA-256",
        },
        directory_estimate("blake3-64-directory", 64, 48, 85, &dense),
        directory_estimate("blake3-128-directory", 128, 56, 72, &dense),
        IntegrityPlacementEstimate {
            id: "blake3-64-regional-header",
            digest_bits: 64,
            placement: "compressed regional header",
            directory_entry_bytes: Some(40),
            directory_entries_per_page: Some(102),
            dense_bucket_pages_over_capacity: 0,
            modeled_archive_growth_bytes: physical_payloads.saturating_mul(8),
            modeled_index_growth_bytes: 0,
            extra_index_reads_per_query: "0",
            detects_before_decompression: false,
            limitation: "digest is unavailable until the containing payload has already decompressed",
        },
        IntegrityPlacementEstimate {
            id: "blake3-128-regional-header",
            digest_bits: 128,
            placement: "compressed regional header",
            directory_entry_bytes: Some(40),
            directory_entries_per_page: Some(102),
            dense_bucket_pages_over_capacity: 0,
            modeled_archive_growth_bytes: physical_payloads.saturating_mul(16),
            modeled_index_growth_bytes: 0,
            extra_index_reads_per_query: "0",
            detects_before_decompression: false,
            limitation: "digest is unavailable until the containing payload has already decompressed",
        },
        IntegrityPlacementEstimate {
            id: "blake3-64-extension-table",
            digest_bits: 64,
            placement: "offset-keyed extension table",
            directory_entry_bytes: Some(40),
            directory_entries_per_page: Some(102),
            dense_bucket_pages_over_capacity: 0,
            modeled_archive_growth_bytes: extension_growth(8),
            modeled_index_growth_bytes: extension_growth(8),
            extra_index_reads_per_query: "at least 1 when the relevant table range is not cached",
            detects_before_decompression: true,
            limitation: "adds another lookup object/range and identity mapping for every physical payload",
        },
        IntegrityPlacementEstimate {
            id: "blake3-128-extension-table",
            digest_bits: 128,
            placement: "offset-keyed extension table",
            directory_entry_bytes: Some(40),
            directory_entries_per_page: Some(102),
            dense_bucket_pages_over_capacity: 0,
            modeled_archive_growth_bytes: extension_growth(16),
            modeled_index_growth_bytes: extension_growth(16),
            extra_index_reads_per_query: "at least 1 when the relevant table range is not cached",
            detects_before_decompression: true,
            limitation: "adds another lookup object/range and identity mapping for every physical payload",
        },
    ];
    Ok(IntegrityEvaluation {
        schema_version: 1,
        archive_path: path.to_path_buf(),
        archive_bytes,
        directory_pages: pages,
        directory_entries: entries,
        physical_payloads,
        compressed_payload_bytes,
        maximum_entries_in_one_page: maximum_entries,
        directory_scan_wall_ms,
        payload_read_wall_ms,
        blake3_wall_ms,
        blake3_mebibytes_per_second,
        placements,
    })
}

fn directory_estimate(
    id: &'static str,
    digest_bits: u32,
    entry_bytes: u64,
    capacity: u64,
    dense: &impl Fn(u64) -> u64,
) -> IntegrityPlacementEstimate {
    IntegrityPlacementEstimate {
        id,
        digest_bits,
        placement: "fixed directory entry",
        directory_entry_bytes: Some(entry_bytes),
        directory_entries_per_page: Some(capacity),
        dense_bucket_pages_over_capacity: dense(capacity),
        modeled_archive_growth_bytes: 0,
        modeled_index_growth_bytes: 0,
        extra_index_reads_per_query: "0",
        detects_before_decompression: true,
        limitation: "reduces the hard number of adaptive entries that fit in one 4 KiB arithmetic page",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_the_shared_fixture_without_mutating_it() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-data/conformance/format-v1.pngr");
        let report = evaluate_integrity_options(&path).unwrap();
        assert_eq!(report.directory_pages, 1);
        assert_eq!(report.directory_entries, 1);
        assert_eq!(report.physical_payloads, 1);
        assert_eq!(report.maximum_entries_in_one_page, 1);
        assert_eq!(report.placements[2].modeled_archive_growth_bytes, 0);
    }
}
