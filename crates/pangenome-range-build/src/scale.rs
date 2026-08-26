use super::fixed::{
    ArchiveBuildMetrics, ArchiveBuildOptions, BuildProgressMode, ChunkCodec,
    DEFAULT_MAX_QUEUED_BYTES, DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES, DEFAULT_MIN_WINDOW_SIZE,
    DEFAULT_PROGRESS_INTERVAL_MS, ExperimentResult, FixedArchiveConfig, QueryMeasurement,
    QuerySpec, build_fixed_archive, build_fixed_archive_with_options, query_fixed_archive,
    reference_paths, source_oracle,
};
use super::source::{
    LoadedGbzSource, PangenomeSource, SourceMemoryPreflight, source_memory_preflight,
};
use gbz::GBZ;
use gbz_base::PathIndex;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use simple_sds::serialize;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const PATH_INDEX_INTERVAL: usize = 1_000;
const WINDOW_SIZE: u64 = 16_384;
const QUERY_CONTEXT: u64 = 100;
const QUERY_SIZES: [u64; 4] = [1_000, 10_000, 100_000, 1_000_000];
const QUERY_COALESCING_GAP: u64 = 65_536;

#[derive(Clone, Debug)]
pub struct EncodeOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    pub report: Option<PathBuf>,
    pub sample: Option<String>,
    pub contig: Option<String>,
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub window_size: u64,
    pub codec: ChunkCodec,
    pub max_uncompressed_chunk_bytes: u64,
    pub min_window_size: u64,
    pub threads: usize,
    pub max_queued_bytes: u64,
    pub scratch_dir: Option<PathBuf>,
    pub keep_partial: bool,
    pub progress: BuildProgressMode,
    pub progress_interval_ms: u64,
    pub max_chunks: Option<u64>,
}

impl EncodeOptions {
    #[must_use]
    pub fn new(input: PathBuf, output: PathBuf) -> Self {
        Self {
            input,
            output,
            report: None,
            sample: None,
            contig: None,
            start: None,
            end: None,
            window_size: WINDOW_SIZE,
            codec: ChunkCodec::Zstd3,
            max_uncompressed_chunk_bytes: DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES,
            min_window_size: DEFAULT_MIN_WINDOW_SIZE,
            threads: default_encode_threads(),
            max_queued_bytes: DEFAULT_MAX_QUEUED_BYTES,
            scratch_dir: None,
            keep_partial: false,
            progress: BuildProgressMode::Off,
            progress_interval_ms: DEFAULT_PROGRESS_INTERVAL_MS,
            max_chunks: None,
        }
    }
}

fn default_encode_threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(8)
}

#[derive(Debug, Serialize)]
pub struct EncodeSummary {
    pub schema_version: u32,
    pub archive_version: u32,
    pub regional_payload_version: u32,
    pub source_path: PathBuf,
    pub source_gbz_bytes: u64,
    pub source_access_mode: &'static str,
    pub source_access_is_bounded: bool,
    pub source_memory_preflight: SourceMemoryPreflight,
    pub source_sha256: String,
    pub source_checksum_wall_ms: f64,
    pub source_load_wall_ms: f64,
    pub rss_after_source_load_kib: Option<u64>,
    pub path_index_wall_ms: f64,
    pub rss_after_path_index_kib: Option<u64>,
    pub process_peak_rss_kib: Option<u64>,
    pub output_path: PathBuf,
    pub output_sha256: String,
    pub prebuild_wall_ms: f64,
    pub output_checksum_wall_ms: f64,
    pub encode_wall_ms_before_report: f64,
    pub accounted_critical_path_wall_ms: f64,
    pub unattributed_critical_path_wall_ms: f64,
    pub sample: Option<String>,
    pub contig: Option<String>,
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub max_chunks: Option<u64>,
    pub window_size: u64,
    pub codec: ChunkCodec,
    pub haplotype_semantics: &'static str,
    pub threads: usize,
    pub max_queued_bytes: u64,
    pub progress_interval_ms: u64,
    pub scratch_dir: Option<PathBuf>,
    pub chunks_per_second: f64,
    pub reference_bp_per_second: f64,
    pub time_to_first_payload_from_encode_start_ms: f64,
    pub build: ArchiveBuildMetrics,
}

fn encode_progress(mode: BuildProgressMode, phase: &str, message: &str) {
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!("[{phase}] {message}"),
        BuildProgressMode::Json => eprintln!(
            "{}",
            serde_json::json!({ "phase": phase, "message": message })
        ),
    }
}

/// Runs the production-shaped direct archive encoder and writes a JSON report.
///
/// # Errors
///
/// Returns an error if inputs/options are invalid, construction fails, or the
/// completed archive/report cannot be validated and persisted.
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub fn run_encode(options: &EncodeOptions) -> ExperimentResult<EncodeSummary> {
    let encode_started = Instant::now();
    if !options.input.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("input GBZ does not exist: {}", options.input.display()),
        )
        .into());
    }
    if options.output.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "refusing to overwrite archive: {}",
                options.output.display()
            ),
        )
        .into());
    }
    if let Some(parent) = options
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    if let Some(scratch_dir) = &options.scratch_dir {
        fs::create_dir_all(scratch_dir)?;
    }
    let source_gbz_bytes = fs::metadata(&options.input)?.len();
    let source_memory_preflight = source_memory_preflight(source_gbz_bytes)?;
    let checksum_started = Instant::now();
    let source_sha256 = file_sha256_with_progress(
        &options.input,
        options.progress,
        "source_checksum",
        "hashing GBZ source",
        options.progress_interval_ms,
    )?;
    let source_checksum_wall_ms = elapsed_ms(checksum_started);
    let load_started = Instant::now();
    let graph: GBZ = run_with_phase_heartbeat(
        options.progress,
        "source_load",
        "loading GBZ source into memory",
        options.progress_interval_ms,
        || serialize::load_from(&options.input),
    )?;
    let source_access_mode = LoadedGbzSource::new(&graph, None).access_mode();
    let source_load_wall_ms = elapsed_ms(load_started);
    let rss_after_source_load_kib = process_current_rss_kib();
    let index_started = Instant::now();
    let path_index = run_with_phase_heartbeat(
        options.progress,
        "path_index",
        "building compact reference path index",
        options.progress_interval_ms,
        || PathIndex::new(&graph, PATH_INDEX_INTERVAL, false),
    )?;
    let path_index_wall_ms = elapsed_ms(index_started);
    let rss_after_path_index_kib = process_current_rss_kib();
    let config = FixedArchiveConfig {
        experiment_id: "fixed-v1-record".into(),
        window_size: options.window_size,
        codec: options.codec,
        deduplicate_chunks: false,
        max_uncompressed_chunk_bytes: options.max_uncompressed_chunk_bytes,
        min_window_size: options.min_window_size,
    };
    let build_options = ArchiveBuildOptions {
        sample: options.sample.clone(),
        contig: options.contig.clone(),
        start: options.start,
        end: options.end,
        max_chunks: options.max_chunks,
        threads: options.threads,
        max_queued_bytes: options.max_queued_bytes,
        keep_partial: options.keep_partial,
        progress: options.progress,
        progress_interval_ms: options.progress_interval_ms,
    };
    encode_progress(
        options.progress,
        "archive_build",
        "starting direct archive writer",
    );
    let prebuild_wall_ms = elapsed_ms(encode_started);
    let build = build_fixed_archive_with_options(
        &graph,
        &path_index,
        source_gbz_bytes,
        &options.output,
        &config,
        &build_options,
    )?;
    let output_checksum_started = Instant::now();
    let output_sha256 = file_sha256_with_progress(
        &options.output,
        options.progress,
        "output_checksum",
        "hashing completed archive",
        options.progress_interval_ms,
    )?;
    let output_checksum_wall_ms = elapsed_ms(output_checksum_started);
    let encode_wall_ms_before_report = elapsed_ms(encode_started);
    let accounted_critical_path_wall_ms =
        prebuild_wall_ms + build.construction_wall_ms + output_checksum_wall_ms;
    let unattributed_critical_path_wall_ms =
        (encode_wall_ms_before_report - accounted_critical_path_wall_ms).max(0.0);
    let seconds = build.construction_wall_ms / 1_000.0;
    let chunks_per_second = if seconds > 0.0 {
        build.directory_entries as f64 / seconds
    } else {
        0.0
    };
    let reference_bp_per_second = if seconds > 0.0 {
        build.reference_bases_processed as f64 / seconds
    } else {
        0.0
    };
    let summary = EncodeSummary {
        schema_version: 3,
        archive_version: 1,
        regional_payload_version: 1,
        source_path: options.input.clone(),
        source_gbz_bytes,
        source_access_mode,
        source_access_is_bounded: false,
        source_memory_preflight,
        source_sha256,
        source_checksum_wall_ms,
        source_load_wall_ms,
        rss_after_source_load_kib,
        path_index_wall_ms,
        rss_after_path_index_kib,
        process_peak_rss_kib: process_peak_rss_kib(),
        output_path: options.output.clone(),
        output_sha256,
        prebuild_wall_ms,
        output_checksum_wall_ms,
        encode_wall_ms_before_report,
        accounted_critical_path_wall_ms,
        unattributed_critical_path_wall_ms,
        sample: options.sample.clone(),
        contig: options.contig.clone(),
        start: options.start,
        end: options.end,
        max_chunks: options.max_chunks,
        window_size: options.window_size,
        codec: options.codec,
        haplotype_semantics: "anonymous-distinct-weighted-tile-paths",
        threads: options.threads,
        max_queued_bytes: options.max_queued_bytes,
        progress_interval_ms: options.progress_interval_ms,
        scratch_dir: options.scratch_dir.clone(),
        chunks_per_second,
        reference_bp_per_second,
        time_to_first_payload_from_encode_start_ms: prebuild_wall_ms + build.first_payload_wall_ms,
        build,
    };
    let report = options
        .report
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("{}.report.json", options.output.display())));
    write_json(&report, &summary)?;
    Ok(summary)
}

#[derive(Clone, Debug, Serialize)]
pub struct EncoderScaleOptions {
    pub input: PathBuf,
    pub archive: PathBuf,
    pub results_dir: PathBuf,
    pub run_id: String,
}

/// Builds one production-shaped archive without constructing the GBZ-base
/// baseline, then checks one deterministic query at each supported scale.
///
/// # Errors
///
/// Returns an error if the input cannot be decoded, the archive cannot be
/// built, a correctness check fails, or an evidence path already exists.
pub fn run_encoder_scale_experiment(options: &EncoderScaleOptions) -> ExperimentResult<()> {
    validate_options(options)?;
    let source_bytes = fs::metadata(&options.input)?.len();
    fs::create_dir_all(&options.results_dir)?;
    if let Some(parent) = options.archive.parent() {
        fs::create_dir_all(parent)?;
    }

    let source_sha256 = file_sha256(&options.input)?;
    let config = FixedArchiveConfig {
        experiment_id: "fixed-v1-16k-zstd3".into(),
        window_size: WINDOW_SIZE,
        codec: ChunkCodec::Zstd3,
        deduplicate_chunks: false,
        max_uncompressed_chunk_bytes: DEFAULT_MAX_UNCOMPRESSED_CHUNK_BYTES,
        min_window_size: DEFAULT_MIN_WINDOW_SIZE,
    };
    write_json(
        &options.results_dir.join("config.json"),
        &json!({
            "schema_version": 1,
            "project": "pangenome-range",
            "run_id": options.run_id,
            "experiment_mode": "encoder-scale",
            "input": options.input,
            "source_gbz_bytes": source_bytes,
            "source_sha256": source_sha256,
            "archive": options.archive,
            "archive_config": config,
            "path_index_interval": PATH_INDEX_INTERVAL,
            "correctness_query_sizes": QUERY_SIZES,
            "correctness_query_coalescing_gap": QUERY_COALESCING_GAP,
            "note": "Encoder-only scale run: no GBZ-base database and no layout sweep are built.",
        }),
    )?;

    eprintln!("loading source GBZ {}", options.input.display());
    let load_started = Instant::now();
    let graph: GBZ = serialize::load_from(&options.input)?;
    let load_wall_ms = elapsed_ms(load_started);
    let rss_after_load_kib = process_current_rss_kib();

    eprintln!("building in-memory path index");
    let index_started = Instant::now();
    let path_index = PathIndex::new(&graph, PATH_INDEX_INTERVAL, false)?;
    let path_index_wall_ms = elapsed_ms(index_started);
    let rss_after_path_index_kib = process_current_rss_kib();

    eprintln!(
        "building encoder-scale archive {}",
        options.archive.display()
    );
    let build = build_fixed_archive(&graph, &path_index, source_bytes, &options.archive, &config)?;

    let references = reference_paths(&graph)?;
    let queries = scale_queries(&references)?;
    let mut measurements = Vec::with_capacity(queries.len());
    for query in &queries {
        eprintln!("checking {} bp query", query.length());
        let oracle = source_oracle(&graph, &path_index, query)?;
        let measurement = query_fixed_archive(
            &options.archive,
            &config,
            query,
            QUERY_COALESCING_GAP,
            &oracle,
            &graph,
            &path_index,
        )?;
        if !measurement.correctness {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("archive failed correctness query {}", query.id),
            )
            .into());
        }
        measurements.push(measurement);
    }

    let peak_rss_kib = process_peak_rss_kib();
    let summary = EncoderScaleSummary {
        source_gbz_bytes: source_bytes,
        source_sha256,
        archive_path: options.archive.clone(),
        gbz_load_wall_ms: load_wall_ms,
        rss_after_gbz_load_kib: rss_after_load_kib,
        path_index_wall_ms,
        rss_after_path_index_kib,
        process_peak_rss_kib: peak_rss_kib,
        build,
        correctness_queries: measurements,
    };
    write_json(&options.results_dir.join("summary.json"), &summary)?;
    write_report(&options.results_dir.join("REPORT.md"), options, &summary)?;
    eprintln!("retained results in {}", options.results_dir.display());
    Ok(())
}

#[derive(Debug, Serialize)]
struct EncoderScaleSummary {
    source_gbz_bytes: u64,
    source_sha256: String,
    archive_path: PathBuf,
    gbz_load_wall_ms: f64,
    rss_after_gbz_load_kib: Option<u64>,
    path_index_wall_ms: f64,
    rss_after_path_index_kib: Option<u64>,
    process_peak_rss_kib: Option<u64>,
    build: ArchiveBuildMetrics,
    correctness_queries: Vec<QueryMeasurement>,
}

fn validate_options(options: &EncoderScaleOptions) -> io::Result<()> {
    if !options.input.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("input GBZ does not exist: {}", options.input.display()),
        ));
    }
    if options.results_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "refusing to overwrite retained results: {}",
                options.results_dir.display()
            ),
        ));
    }
    if options.archive.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "refusing to overwrite scale archive: {}",
                options.archive.display()
            ),
        ));
    }
    Ok(())
}

fn scale_queries(
    references: &[super::fixed::ReferencePathSpec],
) -> ExperimentResult<Vec<QuerySpec>> {
    let reference = references
        .iter()
        .filter(|reference| reference.start == 0)
        .max_by_key(|reference| reference.end.saturating_sub(reference.start))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no reference path found"))?;
    let reference_length = reference.end.saturating_sub(reference.start);
    let mut queries = Vec::new();
    for size in QUERY_SIZES {
        if size > reference_length {
            continue;
        }
        let start = reference.start + (reference_length - size) / 2;
        queries.push(QuerySpec {
            id: format!("scale-{size}"),
            class: "encoder-scale-correctness".into(),
            sample: reference.name.sample.clone(),
            contig: reference.name.contig.clone(),
            start,
            end: start + size,
            context: QUERY_CONTEXT,
        });
    }
    if queries.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "reference paths are too short for scale correctness queries",
        )
        .into());
    }
    Ok(queries)
}

fn write_report(
    path: &Path,
    options: &EncoderScaleOptions,
    summary: &EncoderScaleSummary,
) -> ExperimentResult<()> {
    let mut output = BufWriter::new(File::create(path)?);
    let all_correct = summary
        .correctness_queries
        .iter()
        .all(|measurement| measurement.correctness);
    writeln!(output, "# Encoder scale run: {}", options.run_id)?;
    writeln!(output)?;
    writeln!(
        output,
        "This is an encoder-focused scale run. It builds one 16 KiB/zstd-3 archive and does not build GBZ-base or run the 20-layout sweep."
    )?;
    writeln!(output)?;
    writeln!(output, "## Result")?;
    writeln!(output)?;
    writeln!(output, "- Source GBZ: {} bytes", summary.source_gbz_bytes)?;
    writeln!(
        output,
        "- Archive: {} bytes ({:.3}x source)",
        summary.build.archive_bytes, summary.build.expansion_ratio
    )?;
    writeln!(
        output,
        "- Load / path-index / encode: {:.3} s / {:.3} s / {:.3} s",
        summary.gbz_load_wall_ms / 1_000.0,
        summary.path_index_wall_ms / 1_000.0,
        summary.build.construction_wall_ms / 1_000.0
    )?;
    writeln!(
        output,
        "- RSS after load / after path index / process peak: {} / {} / {} KiB",
        option_u64(summary.rss_after_gbz_load_kib),
        option_u64(summary.rss_after_path_index_kib),
        option_u64(summary.process_peak_rss_kib)
    )?;
    writeln!(
        output,
        "- Payload spool / occurrence index: {} / {} bytes",
        summary.build.payload_spool_bytes, summary.build.path_occurrence_index_bytes
    )?;
    writeln!(
        output,
        "- Peak raw / compressed chunk: {} / {} bytes",
        summary.build.peak_raw_chunk_bytes, summary.build.peak_compressed_chunk_bytes
    )?;
    writeln!(
        output,
        "- Correctness: {} ({}/{} queries)",
        all_correct,
        summary
            .correctness_queries
            .iter()
            .filter(|measurement| measurement.correctness)
            .count(),
        summary.correctness_queries.len()
    )?;
    writeln!(output)?;
    writeln!(output, "## Correctness queries")?;
    writeln!(output)?;
    writeln!(output, "| Query size | Reads | Bytes fetched | Correct |")?;
    writeln!(output, "|---:|---:|---:|:---:|")?;
    for measurement in &summary.correctness_queries {
        writeln!(
            output,
            "| {} | {} | {} | {} |",
            measurement.query_size,
            measurement.physical_reads,
            measurement.total_bytes_fetched,
            measurement.correctness
        )?;
    }
    writeln!(output)?;
    writeln!(
        output,
        "The source and archive live outside the repository at `{}` and `{}`. The bounded temporary payload spool was created beside the archive and removed after completion; no occurrence-index file was created.",
        options.input.display(),
        options.archive.display()
    )?;
    output.flush()?;
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> ExperimentResult<()> {
    let mut output = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut output, value)?;
    writeln!(output)?;
    output.flush()?;
    Ok(())
}

fn file_sha256(path: &Path) -> io::Result<String> {
    file_sha256_with_progress(
        path,
        BuildProgressMode::Off,
        "checksum",
        "hashing file",
        DEFAULT_PROGRESS_INTERVAL_MS,
    )
}

fn run_with_phase_heartbeat<T>(
    mode: BuildProgressMode,
    phase: &'static str,
    message: &'static str,
    interval_ms: u64,
    work: impl FnOnce() -> T,
) -> T {
    encode_progress(mode, phase, message);
    if mode == BuildProgressMode::Off {
        return work();
    }
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    let reporter_done = Arc::clone(&done);
    let reporter = std::thread::spawn(move || {
        let started = Instant::now();
        let interval = Duration::from_millis(interval_ms.max(250));
        let (lock, condition) = &*reporter_done;
        let Ok(mut finished) = lock.lock() else {
            return;
        };
        loop {
            let Ok((next_finished, timeout)) = condition.wait_timeout(finished, interval) else {
                return;
            };
            finished = next_finished;
            if *finished {
                return;
            }
            if timeout.timed_out() {
                let elapsed_seconds = started.elapsed().as_secs_f64();
                match mode {
                    BuildProgressMode::Off => {}
                    BuildProgressMode::Plain => eprintln!(
                        "[{phase}] still working | elapsed {}",
                        format_duration(elapsed_seconds)
                    ),
                    BuildProgressMode::Json => eprintln!(
                        "{}",
                        json!({
                            "phase": phase,
                            "event": "heartbeat",
                            "message": message,
                            "elapsed_seconds": elapsed_seconds,
                        })
                    ),
                }
            }
        }
    });
    let result = work();
    let (lock, condition) = &*done;
    if let Ok(mut finished) = lock.lock() {
        *finished = true;
        condition.notify_all();
    }
    let _ = reporter.join();
    result
}

#[allow(clippy::cast_precision_loss)]
fn file_sha256_with_progress(
    path: &Path,
    mode: BuildProgressMode,
    phase: &'static str,
    message: &'static str,
    interval_ms: u64,
) -> io::Result<String> {
    encode_progress(mode, phase, message);
    let total_bytes = fs::metadata(path)?.len();
    let mut input = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let started = Instant::now();
    let mut bytes_read = 0_u64;
    let mut last_emit_ms = 0.0;
    let mut last_reported_bytes = u64::MAX;
    let mut sequence = 0_u64;
    loop {
        let length = input.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hasher.update(&buffer[..length]);
        bytes_read = bytes_read.saturating_add(length as u64);
        let elapsed_seconds = started.elapsed().as_secs_f64();
        let elapsed_ms = elapsed_seconds * 1_000.0;
        if elapsed_ms - last_emit_ms >= interval_ms as f64 || bytes_read == total_bytes {
            sequence = sequence.saturating_add(1);
            emit_checksum_progress(
                mode,
                phase,
                sequence,
                bytes_read,
                total_bytes,
                elapsed_seconds,
            );
            last_emit_ms = elapsed_ms;
            last_reported_bytes = bytes_read;
        }
    }
    if last_reported_bytes != bytes_read {
        emit_checksum_progress(
            mode,
            phase,
            sequence.saturating_add(1),
            bytes_read,
            total_bytes,
            started.elapsed().as_secs_f64(),
        );
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[allow(clippy::cast_precision_loss)]
fn emit_checksum_progress(
    mode: BuildProgressMode,
    phase: &str,
    sequence: u64,
    bytes_read: u64,
    total_bytes: u64,
    elapsed_seconds: f64,
) {
    if mode == BuildProgressMode::Off {
        return;
    }
    let percent_complete = if total_bytes == 0 {
        100.0
    } else {
        bytes_read as f64 / total_bytes as f64 * 100.0
    };
    let bytes_per_second = if elapsed_seconds > 0.0 {
        bytes_read as f64 / elapsed_seconds
    } else {
        0.0
    };
    let estimated_seconds_remaining = (bytes_per_second > 0.0)
        .then(|| total_bytes.saturating_sub(bytes_read) as f64 / bytes_per_second);
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!(
            "[{phase}] {percent_complete:6.2}% | {}/{} | {}/s | ETA {} | elapsed {}",
            format_bytes(bytes_read),
            format_bytes(total_bytes),
            format_byte_rate(bytes_per_second),
            estimated_seconds_remaining.map_or_else(|| "unknown".into(), format_duration),
            format_duration(elapsed_seconds),
        ),
        BuildProgressMode::Json => eprintln!(
            "{}",
            json!({
                "phase": format!("{phase}_progress"),
                "sequence": sequence,
                "percent_complete": percent_complete,
                "bytes_read": bytes_read,
                "total_bytes": total_bytes,
                "bytes_per_second": bytes_per_second,
                "estimated_seconds_remaining": estimated_seconds_remaining,
                "elapsed_seconds": elapsed_seconds,
            })
        ),
    }
}

#[allow(clippy::cast_precision_loss)]
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn format_byte_rate(bytes_per_second: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * KIB;
    const GIB: f64 = 1024.0 * MIB;
    if bytes_per_second >= GIB {
        format!("{:.2} GiB", bytes_per_second / GIB)
    } else if bytes_per_second >= MIB {
        format!("{:.1} MiB", bytes_per_second / MIB)
    } else if bytes_per_second >= KIB {
        format!("{:.1} KiB", bytes_per_second / KIB)
    } else {
        format!("{:.0} B", bytes_per_second.max(0.0))
    }
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown".into();
    }
    let rounded = seconds.round() as u64;
    let hours = rounded / 3_600;
    let minutes = (rounded % 3_600) / 60;
    let seconds = rounded % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

fn process_current_rss_kib() -> Option<u64> {
    process_status_value("VmRSS:")
}

fn process_peak_rss_kib() -> Option<u64> {
    process_status_value("VmHWM:")
}

fn process_status_value(label: &str) -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix(label)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

fn option_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "unknown".into(), |value| value.to_string())
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::ReferencePathSpec;
    use gbz::FullPathName;

    #[test]
    fn scale_queries_use_the_longest_unsplit_reference() {
        let references = vec![
            ReferencePathSpec {
                name: FullPathName {
                    sample: "split".into(),
                    contig: "chr1".into(),
                    haplotype: 0,
                    fragment: 10,
                },
                start: 10,
                end: 2_000_010,
            },
            ReferencePathSpec {
                name: FullPathName {
                    sample: "reference".into(),
                    contig: "chr2".into(),
                    haplotype: 0,
                    fragment: 0,
                },
                start: 0,
                end: 2_000_000,
            },
        ];

        let queries = scale_queries(&references).unwrap();
        assert_eq!(
            queries.iter().map(QuerySpec::length).collect::<Vec<_>>(),
            QUERY_SIZES
        );
        assert!(
            queries
                .iter()
                .all(|query| query.sample == "reference" && query.contig == "chr2")
        );
    }
}
