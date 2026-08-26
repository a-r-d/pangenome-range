use crate::binary::{invalid_data, u64_to_usize, usize_to_u64};
use crate::{
    ArchiveEntry, DIRECTORY_PAGE_BYTES, FileRangeSource, MAX_DECODED_OCCURRENCES_PER_TILE,
    RangeSource, RecordRegionalPayload, bootstrap, decode_directory_page, decompress,
    directory_page_offset, validate_extension_payload,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValidationMode {
    #[default]
    Standard,
    Full,
}

#[derive(Clone, Copy, Debug)]
pub struct ValidationOptions {
    pub mode: ValidationMode,
    pub workers: usize,
    pub max_queued_bytes: u64,
    pub progress_interval_ms: u64,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            mode: ValidationMode::Standard,
            workers: 1,
            max_queued_bytes: 512 * 1024 * 1024,
            progress_interval_ms: 5_000,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ArchiveValidationProgress {
    pub phase: &'static str,
    pub sequence: u64,
    pub percent_complete: f64,
    pub directory_pages_validated: u64,
    pub directory_pages_total: u64,
    pub directory_entries_validated: u64,
    pub directory_entries_total: u64,
    pub physical_payloads_validated: u64,
    pub compressed_payload_bytes_validated: u64,
    pub uncompressed_payload_bytes_validated: u64,
    pub entries_per_second: f64,
    pub estimated_seconds_remaining: Option<f64>,
    pub elapsed_seconds: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArchiveValidationSummary {
    pub schema_version: u32,
    pub archive_version: u32,
    pub archive_path: PathBuf,
    pub archive_bytes: u64,
    pub reference_manifests: u64,
    pub directory_pages: u64,
    pub directory_entries: u64,
    pub physical_payloads: u64,
    pub compressed_payload_bytes: u64,
    pub uncompressed_payload_bytes: u64,
    pub validation_mode: ValidationMode,
    pub requested_workers: u64,
    pub effective_workers: u64,
    pub max_queued_bytes: u64,
    pub peak_estimated_worker_bytes: u64,
    pub extension_entries: u64,
    pub extension_encoded_bytes: u64,
    pub extension_decoded_bytes: u64,
    pub directory_validation_wall_ms: f64,
    pub payload_validation_wall_ms: f64,
    pub payload_read_worker_ms: f64,
    pub integrity_worker_ms: f64,
    pub decompression_worker_ms: f64,
    pub regional_decode_worker_ms: f64,
    pub reconstruction_worker_ms: f64,
    pub validation_wall_ms: f64,
}

#[derive(Default)]
struct PayloadWorkerMeasurement {
    compressed_bytes: u64,
    uncompressed_bytes: u64,
    read_ms: f64,
    integrity_ms: f64,
    decompression_ms: f64,
    decode_ms: f64,
    reconstruction_ms: f64,
}

#[derive(Default)]
struct ProgressState {
    sequence: u64,
    last_emit_ms: f64,
    last_directory_pages: u64,
    last_directory_entries: u64,
    last_physical_payloads: u64,
}

/// Runs the default one-worker standard validation gate.
///
/// # Errors
///
/// Returns an error for archive I/O, corruption, unsafe resource requirements,
/// or any failed payload validation.
pub fn validate_archive(path: &Path) -> io::Result<ArchiveValidationSummary> {
    validate_archive_with_options(path, ValidationOptions::default(), |_| {})
}

#[allow(clippy::too_many_lines)]
/// Runs standard validation while emitting bounded progress updates.
///
/// # Errors
///
/// Returns an error for archive I/O, corruption, unsafe resource requirements,
/// progress accounting failure, or any failed payload validation.
pub fn validate_archive_with_progress(
    path: &Path,
    progress_interval_ms: u64,
    emit: impl FnMut(&ArchiveValidationProgress),
) -> io::Result<ArchiveValidationSummary> {
    validate_archive_with_options(
        path,
        ValidationOptions {
            progress_interval_ms,
            ..ValidationOptions::default()
        },
        emit,
    )
}

#[allow(clippy::too_many_lines)]
/// Runs the selected validation mode with bounded parallel workers.
///
/// # Errors
///
/// Returns an error for invalid options, archive I/O/corruption, range overlap,
/// memory-budget violations, worker failure, or any failed integrity, decode,
/// or reconstruction gate.
pub fn validate_archive_with_options(
    path: &Path,
    options: ValidationOptions,
    mut emit: impl FnMut(&ArchiveValidationProgress),
) -> io::Result<ArchiveValidationSummary> {
    if options.workers == 0 || options.max_queued_bytes == 0 {
        return Err(invalid_data(
            "validation workers and max queued bytes must be nonzero",
        ));
    }
    let started = Instant::now();
    let source = Arc::new(FileRangeSource::open(path)?);
    let bootstrap = bootstrap(source.as_ref())?;
    let header = bootstrap.header;
    let source_len = source.len()?;
    let mut entry_count = 0_u64;
    let mut physical_payloads = BTreeMap::<(u64, u64), ArchiveEntry>::new();
    let mut compressed_payload_bytes = 0_u64;
    let mut uncompressed_payload_bytes = 0_u64;
    let mut directory_pages = 0_u64;
    let directory_pages_total =
        bootstrap
            .root
            .manifests
            .iter()
            .try_fold(0_u64, |total, manifest| {
                total
                    .checked_add(manifest.page_count)
                    .ok_or_else(|| invalid_data("validation directory page count overflow"))
            })?;
    let mut progress_state = ProgressState::default();
    let directory_started = Instant::now();
    for manifest in &bootstrap.root.manifests {
        for bucket_index in 0..manifest.page_count {
            let page_offset = directory_page_offset(manifest, bucket_index)?;
            let page = source.read_range(page_offset, DIRECTORY_PAGE_BYTES)?;
            let entries = decode_directory_page(&page, manifest, bucket_index)?;
            directory_pages = directory_pages
                .checked_add(1)
                .ok_or_else(|| invalid_data("validated directory page count overflow"))?;
            for entry in entries {
                let payload_end = entry
                    .offset
                    .checked_add(entry.compressed_len)
                    .ok_or_else(|| invalid_data("payload range overflow during validation"))?;
                if entry.offset < header.data_offset || payload_end > source_len {
                    return Err(invalid_data("payload range is outside the archive"));
                }
                if register_physical_payload(&mut physical_payloads, &entry)? {
                    compressed_payload_bytes = compressed_payload_bytes
                        .checked_add(entry.compressed_len)
                        .ok_or_else(|| invalid_data("validated compressed byte count overflow"))?;
                    uncompressed_payload_bytes = uncompressed_payload_bytes
                        .checked_add(entry.uncompressed_len)
                        .ok_or_else(|| {
                            invalid_data("validated uncompressed byte count overflow")
                        })?;
                }
                entry_count = entry_count
                    .checked_add(1)
                    .ok_or_else(|| invalid_data("validated directory entry count overflow"))?;
            }
        }
    }
    if entry_count != header.entry_count {
        return Err(invalid_data(
            "validated directory count differs from archive header",
        ));
    }
    let directory_validation_wall_ms = directory_started.elapsed().as_secs_f64() * 1_000.0;
    let entries = physical_payloads.into_values().collect::<Vec<_>>();
    validate_physical_payload_ranges(&entries)?;

    let mut extension_encoded_bytes = 0_u64;
    let mut extension_decoded_bytes = 0_u64;
    for extension in &bootstrap.extensions {
        let encoded = source.read_range(extension.offset, u64_to_usize(extension.encoded_len)?)?;
        let decoded = validate_extension_payload(extension, &encoded)?;
        extension_encoded_bytes = extension_encoded_bytes
            .checked_add(extension.encoded_len)
            .ok_or_else(|| invalid_data("extension encoded byte count overflow"))?;
        extension_decoded_bytes = extension_decoded_bytes
            .checked_add(usize_to_u64(decoded.len())?)
            .ok_or_else(|| invalid_data("extension decoded byte count overflow"))?;
    }

    let max_job_bytes = entries.iter().try_fold(0_u64, |maximum, entry| {
        estimated_worker_bytes(entry, options.mode).map(|bytes| maximum.max(bytes))
    })?;
    if max_job_bytes > options.max_queued_bytes {
        return Err(invalid_data(format!(
            "one validation job requires an estimated {max_job_bytes} bytes, above max queued bytes {}",
            options.max_queued_bytes
        )));
    }
    let memory_workers = options
        .max_queued_bytes
        .checked_div(max_job_bytes)
        .map_or(options.workers, |workers| {
            usize::try_from(workers).unwrap_or(usize::MAX).max(1)
        });
    let physical_payload_count = usize_to_u64(entries.len())?;
    let effective_workers = options
        .workers
        .min(memory_workers)
        .min(entries.len().max(1));
    let peak_estimated_worker_bytes = max_job_bytes
        .checked_mul(usize_to_u64(effective_workers)?)
        .ok_or_else(|| invalid_data("validation worker memory estimate overflow"))?;
    let payload_started = Instant::now();
    let entries = Arc::new(entries);
    let measurements = validate_payloads_parallel(
        &source,
        &entries,
        options.mode,
        effective_workers,
        |validated, measurement| {
            maybe_emit_progress(
                options.progress_interval_ms,
                false,
                started,
                &mut progress_state,
                directory_pages,
                directory_pages_total,
                entry_count,
                header.entry_count,
                validated,
                physical_payload_count,
                compressed_payload_bytes,
                uncompressed_payload_bytes,
                &mut emit,
            )?;
            let _ = measurement;
            Ok(())
        },
    )?;
    let payload_validation_wall_ms = payload_started.elapsed().as_secs_f64() * 1_000.0;
    let aggregate =
        measurements
            .into_iter()
            .fold(PayloadWorkerMeasurement::default(), |mut total, current| {
                total.compressed_bytes += current.compressed_bytes;
                total.uncompressed_bytes += current.uncompressed_bytes;
                total.read_ms += current.read_ms;
                total.integrity_ms += current.integrity_ms;
                total.decompression_ms += current.decompression_ms;
                total.decode_ms += current.decode_ms;
                total.reconstruction_ms += current.reconstruction_ms;
                total
            });
    if aggregate.compressed_bytes != compressed_payload_bytes
        || aggregate.uncompressed_bytes != uncompressed_payload_bytes
    {
        return Err(invalid_data(
            "worker byte totals differ from directory totals",
        ));
    }
    maybe_emit_progress(
        options.progress_interval_ms,
        true,
        started,
        &mut progress_state,
        directory_pages,
        directory_pages_total,
        entry_count,
        header.entry_count,
        physical_payload_count,
        physical_payload_count,
        compressed_payload_bytes,
        uncompressed_payload_bytes,
        &mut emit,
    )?;
    Ok(ArchiveValidationSummary {
        schema_version: 2,
        archive_version: header.version,
        archive_path: path.to_path_buf(),
        archive_bytes: source_len,
        reference_manifests: usize_to_u64(bootstrap.root.manifests.len())?,
        directory_pages,
        directory_entries: entry_count,
        physical_payloads: physical_payload_count,
        compressed_payload_bytes,
        uncompressed_payload_bytes,
        validation_mode: options.mode,
        requested_workers: usize_to_u64(options.workers)?,
        effective_workers: usize_to_u64(effective_workers)?,
        max_queued_bytes: options.max_queued_bytes,
        peak_estimated_worker_bytes,
        extension_entries: usize_to_u64(bootstrap.extensions.len())?,
        extension_encoded_bytes,
        extension_decoded_bytes,
        directory_validation_wall_ms,
        payload_validation_wall_ms,
        payload_read_worker_ms: aggregate.read_ms,
        integrity_worker_ms: aggregate.integrity_ms,
        decompression_worker_ms: aggregate.decompression_ms,
        regional_decode_worker_ms: aggregate.decode_ms,
        reconstruction_worker_ms: aggregate.reconstruction_ms,
        validation_wall_ms: started.elapsed().as_secs_f64() * 1_000.0,
    })
}

/// Registers one physical payload and returns `true` only for its first
/// directory reference. Exact duplicate references are therefore validated
/// once, while conflicting metadata for one byte range fails closed.
fn register_physical_payload(
    payloads: &mut BTreeMap<(u64, u64), ArchiveEntry>,
    entry: &ArchiveEntry,
) -> io::Result<bool> {
    if let Some(existing) = payloads.insert((entry.offset, entry.compressed_len), entry.clone()) {
        if existing != *entry {
            return Err(invalid_data(
                "duplicate physical payload has conflicting directory metadata",
            ));
        }
        Ok(false)
    } else {
        Ok(true)
    }
}

fn validate_physical_payload_ranges(entries: &[ArchiveEntry]) -> io::Result<()> {
    for pair in entries.windows(2) {
        let previous_end = pair[0]
            .offset
            .checked_add(pair[0].compressed_len)
            .ok_or_else(|| invalid_data("payload range overflow during overlap check"))?;
        if pair[1].offset < previous_end {
            return Err(invalid_data("physical payload ranges partially overlap"));
        }
    }
    Ok(())
}

fn estimated_worker_bytes(entry: &ArchiveEntry, mode: ValidationMode) -> io::Result<u64> {
    let base = entry
        .uncompressed_len
        .checked_mul(2)
        .and_then(|decoded| decoded.checked_add(entry.compressed_len))
        .ok_or_else(|| invalid_data("validation worker memory estimate overflow"))?;
    if mode == ValidationMode::Standard {
        return Ok(base);
    }
    base.checked_add(
        MAX_DECODED_OCCURRENCES_PER_TILE
            .checked_mul(17)
            .ok_or_else(|| invalid_data("full validation memory estimate overflow"))?,
    )
    .ok_or_else(|| invalid_data("full validation memory estimate overflow"))
}

fn validate_payloads_parallel(
    source: &Arc<FileRangeSource>,
    entries: &Arc<Vec<ArchiveEntry>>,
    mode: ValidationMode,
    workers: usize,
    mut completed: impl FnMut(u64, &PayloadWorkerMeasurement) -> io::Result<()>,
) -> io::Result<Vec<PayloadWorkerMeasurement>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let next = Arc::new(AtomicUsize::new(0));
    let stopped = Arc::new(AtomicBool::new(false));
    let (sender, receiver) =
        mpsc::sync_channel::<io::Result<PayloadWorkerMeasurement>>(workers * 2);
    thread::scope(|scope| -> io::Result<Vec<PayloadWorkerMeasurement>> {
        for _ in 0..workers {
            let source = Arc::clone(source);
            let entries = Arc::clone(entries);
            let next = Arc::clone(&next);
            let stopped = Arc::clone(&stopped);
            let sender = sender.clone();
            scope.spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let index = next.fetch_add(1, Ordering::AcqRel);
                    let Some(entry) = entries.get(index) else {
                        break;
                    };
                    let result = validate_one_payload(&source, entry, mode);
                    let failed = result.is_err();
                    if sender.send(result).is_err() {
                        break;
                    }
                    if failed {
                        stopped.store(true, Ordering::Release);
                        break;
                    }
                }
            });
        }
        drop(sender);
        let mut measurements = Vec::with_capacity(entries.len());
        while measurements.len() < entries.len() {
            let measurement = receiver
                .recv()
                .map_err(|_| invalid_data("validation workers stopped before completion"))??;
            measurements.push(measurement);
            completed(
                usize_to_u64(measurements.len())?,
                measurements.last().unwrap(),
            )?;
        }
        Ok(measurements)
    })
}

fn validate_one_payload(
    source: &FileRangeSource,
    entry: &ArchiveEntry,
    mode: ValidationMode,
) -> io::Result<PayloadWorkerMeasurement> {
    let read_started = Instant::now();
    let encoded = source.read_range(entry.offset, u64_to_usize(entry.compressed_len)?)?;
    let read_ms = read_started.elapsed().as_secs_f64() * 1_000.0;
    let integrity_started = Instant::now();
    if blake3::hash(&encoded).as_bytes()[..16] != entry.integrity {
        return Err(invalid_data("validated payload integrity mismatch"));
    }
    let integrity_ms = integrity_started.elapsed().as_secs_f64() * 1_000.0;
    let decompression_started = Instant::now();
    let raw = decompress(entry.codec, &encoded, entry.uncompressed_len)?;
    let decompression_ms = decompression_started.elapsed().as_secs_f64() * 1_000.0;
    let decode_started = Instant::now();
    let payload = RecordRegionalPayload::decode(&raw)?;
    if payload.core_start != entry.start || payload.core_end != entry.end {
        return Err(invalid_data(
            "validated payload provenance differs from its directory entry",
        ));
    }
    let decode_ms = decode_started.elapsed().as_secs_f64() * 1_000.0;
    let reconstruction_started = Instant::now();
    if mode == ValidationMode::Full {
        payload.reconstruct_traversals()?;
    }
    let reconstruction_ms = reconstruction_started.elapsed().as_secs_f64() * 1_000.0;
    Ok(PayloadWorkerMeasurement {
        compressed_bytes: entry.compressed_len,
        uncompressed_bytes: entry.uncompressed_len,
        read_ms,
        integrity_ms,
        decompression_ms,
        decode_ms,
        reconstruction_ms,
    })
}

#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
fn maybe_emit_progress(
    interval_ms: u64,
    force: bool,
    started: Instant,
    state: &mut ProgressState,
    directory_pages_validated: u64,
    directory_pages_total: u64,
    directory_entries_validated: u64,
    directory_entries_total: u64,
    physical_payloads_validated: u64,
    physical_payloads_total: u64,
    compressed_payload_bytes_validated: u64,
    uncompressed_payload_bytes_validated: u64,
    emit: &mut impl FnMut(&ArchiveValidationProgress),
) -> io::Result<()> {
    let elapsed_seconds = started.elapsed().as_secs_f64();
    let elapsed_ms = elapsed_seconds * 1_000.0;
    if force
        && state.sequence > 0
        && state.last_directory_pages == directory_pages_validated
        && state.last_directory_entries == directory_entries_validated
        && state.last_physical_payloads == physical_payloads_validated
    {
        return Ok(());
    }
    if !force && state.sequence == 0 && interval_ms > 0 && elapsed_ms < interval_ms as f64 {
        return Ok(());
    }
    if !force && state.sequence > 0 && elapsed_ms - state.last_emit_ms < interval_ms as f64 {
        return Ok(());
    }
    state.sequence = state
        .sequence
        .checked_add(1)
        .ok_or_else(|| invalid_data("validation progress sequence overflow"))?;
    state.last_emit_ms = elapsed_ms;
    state.last_directory_pages = directory_pages_validated;
    state.last_directory_entries = directory_entries_validated;
    state.last_physical_payloads = physical_payloads_validated;
    let entries_per_second = if elapsed_seconds > 0.0 {
        physical_payloads_validated as f64 / elapsed_seconds
    } else {
        0.0
    };
    let estimated_seconds_remaining = (entries_per_second > 0.0).then(|| {
        physical_payloads_total.saturating_sub(physical_payloads_validated) as f64
            / entries_per_second
    });
    let percent_complete = if physical_payloads_total == 0 {
        100.0
    } else {
        physical_payloads_validated as f64 / physical_payloads_total as f64 * 100.0
    };
    emit(&ArchiveValidationProgress {
        phase: "archive_validation_progress",
        sequence: state.sequence,
        percent_complete,
        directory_pages_validated,
        directory_pages_total,
        directory_entries_validated,
        directory_entries_total,
        physical_payloads_validated,
        compressed_payload_bytes_validated,
        uncompressed_payload_bytes_validated,
        entries_per_second,
        estimated_seconds_remaining,
        elapsed_seconds,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_the_shared_archive_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-data/conformance/format-v1.pngr");
        let summary = validate_archive(&path).unwrap();
        assert_eq!(summary.archive_version, 1);
        assert_eq!(summary.directory_pages, 1);
        assert_eq!(summary.directory_entries, 1);
        assert_eq!(summary.physical_payloads, 1);
    }

    #[test]
    fn standard_validation_is_equivalent_across_worker_counts() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-data/conformance/micb-kir3dl1-reader-v1.pngr");
        let baseline = validate_archive_with_options(
            &path,
            ValidationOptions {
                workers: 1,
                ..ValidationOptions::default()
            },
            |_| {},
        )
        .unwrap();
        assert!(baseline.physical_payloads > 1);
        for workers in [2, 4, 8] {
            let current = validate_archive_with_options(
                &path,
                ValidationOptions {
                    workers,
                    ..ValidationOptions::default()
                },
                |_| {},
            )
            .unwrap();
            assert_eq!(current.directory_entries, baseline.directory_entries);
            assert_eq!(current.physical_payloads, baseline.physical_payloads);
            assert_eq!(
                current.compressed_payload_bytes,
                baseline.compressed_payload_bytes
            );
            assert_eq!(
                current.uncompressed_payload_bytes,
                baseline.uncompressed_payload_bytes
            );
        }
    }

    #[test]
    fn exact_duplicate_physical_payload_is_registered_once() {
        let entry = ArchiveEntry {
            start: 10,
            end: 20,
            offset: 4096,
            compressed_len: 100,
            uncompressed_len: 200,
            integrity: [7; 16],
            codec: crate::ChunkCodec::Zstd3,
        };
        let mut payloads = BTreeMap::new();
        assert!(register_physical_payload(&mut payloads, &entry).unwrap());
        assert!(!register_physical_payload(&mut payloads, &entry).unwrap());
        assert_eq!(payloads.len(), 1);

        let mut conflicting = entry.clone();
        conflicting.integrity = [8; 16];
        assert!(register_physical_payload(&mut payloads, &conflicting).is_err());
    }

    #[test]
    fn partial_physical_payload_overlap_fails_closed() {
        let first = ArchiveEntry {
            start: 10,
            end: 20,
            offset: 4096,
            compressed_len: 100,
            uncompressed_len: 200,
            integrity: [7; 16],
            codec: crate::ChunkCodec::Zstd3,
        };
        let mut second = first.clone();
        second.start = 20;
        second.end = 30;
        second.offset = 4195;
        assert!(validate_physical_payload_ranges(&[first, second]).is_err());
    }

    #[test]
    fn corrupted_payload_fails_before_success() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test-data/conformance/format-v1.pngr");
        let mut bytes = std::fs::read(fixture).unwrap();
        let last = bytes.last_mut().unwrap();
        *last ^= 0xff;
        let path = std::env::temp_dir().join(format!(
            "pangenome-range-corrupt-validation-{}",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        assert!(validate_archive(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn header_constant_remains_consumed() {
        assert_eq!(crate::HEADER_LEN, 64);
    }
}
