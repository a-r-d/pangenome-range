use super::fixed::{
    ArchiveBuildMetrics, ChunkCodec, ExperimentResult, FixedArchiveConfig, OracleResult,
    QueryMeasurement, QuerySpec, build_fixed_archive, query_fixed_archive, reference_paths,
    source_oracle,
};
use gbz::{FullPathName, GBZ};
use gbz_base::{GBZBase, GraphInterface, HaplotypeOutput, PathIndex, Subgraph, SubgraphQuery};
use pangenome_range_format::NetworkProfile;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use simple_sds::serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const RNG_SEED: u64 = 0x5041_4E47_454E_4F4D;
const PATH_INDEX_INTERVAL: usize = 1_000;
const CONTEXT: u64 = 100;
const WINDOW_SIZES: [u64; 5] = [16_384, 65_536, 262_144, 1_048_576, 4_194_304];
const CODECS: [ChunkCodec; 4] = [
    ChunkCodec::None,
    ChunkCodec::Zstd1,
    ChunkCodec::Zstd3,
    ChunkCodec::Zstd6,
];
const COALESCING_GAPS: [u64; 6] = [0, 4_096, 16_384, 65_536, 262_144, 1_048_576];
const PARETO_GAP: u64 = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExperimentMode {
    FullSweep,
    SingleConfigSmoke,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExperimentOptions {
    pub input: PathBuf,
    pub results_dir: PathBuf,
    pub scratch_dir: PathBuf,
    pub run_id: String,
    pub random_queries_per_size: usize,
    pub mode: ExperimentMode,
}

#[derive(Clone, Debug)]
struct CandidateRun {
    config: FixedArchiveConfig,
    build: ArchiveBuildMetrics,
    measurements: Vec<QueryMeasurement>,
    improvement: bool,
}

#[derive(Clone, Debug, Serialize)]
struct QueryRow {
    storage_kind: String,
    experiment_id: String,
    query_id: String,
    query_class: String,
    sample: String,
    contig: String,
    start: u64,
    end: u64,
    query_size: u64,
    context: u64,
    coalescing_gap: Option<u64>,
    physical_reads: Option<u64>,
    mergeable_reads: Option<u64>,
    dependency_rounds: Option<u64>,
    total_bytes_fetched: Option<u64>,
    unique_bytes_fetched: Option<u64>,
    duplicate_bytes_fetched: Option<u64>,
    bootstrap_bytes_fetched: Option<u64>,
    logical_index_bytes: Option<u64>,
    data_bytes_fetched: Option<u64>,
    required_compressed_payload_bytes: Option<u64>,
    canonical_payload_bytes: Option<u64>,
    read_amplification: Option<f64>,
    canonical_amplification: Option<f64>,
    index_lookup_us: Option<f64>,
    decompression_us: Option<f64>,
    decode_us: Option<f64>,
    graph_reconstruction_us: Option<f64>,
    total_local_query_us: f64,
    selected_chunks: Option<u64>,
    selected_nodes: Option<u64>,
    canonical_hash: Option<String>,
    correctness: bool,
    simulated_20ms_ms: Option<f64>,
    simulated_50ms_ms: Option<f64>,
    simulated_100ms_ms: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
struct Distribution {
    count: usize,
    mean: f64,
    p50: f64,
    p90: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

#[derive(Clone, Debug, Serialize)]
struct QueryAggregate {
    experiment_id: String,
    coalescing_gap: u64,
    query_group: String,
    correctness: bool,
    physical_reads: Distribution,
    bytes_fetched: Distribution,
    read_amplification: Distribution,
    index_lookup_us: Distribution,
    decompression_us: Distribution,
    decode_us: Distribution,
    graph_reconstruction_us: Distribution,
    local_query_us: Distribution,
    simulated_20ms_ms: Distribution,
    simulated_50ms_ms: Distribution,
    simulated_100ms_ms: Distribution,
}

#[derive(Clone, Debug, Serialize)]
struct BaselineSummary {
    gbz_bytes: u64,
    gbz_load_wall_ms: f64,
    gbz_load_behavior: String,
    gbz_query_limitations: String,
    gbz_query_us: Distribution,
    gbz_base_bytes: u64,
    gbz_base_to_gbz_ratio: f64,
    gbz_base_build_wall_ms: f64,
    gbz_base_open_wall_ms: f64,
    gbz_base_query_us: Distribution,
    gbz_base_correctness: bool,
    sqlite_io_observation: StraceSummary,
}

#[derive(Clone, Debug, Serialize)]
struct BaselineQueryAggregate {
    storage_kind: String,
    query_group: String,
    local_query_us: Distribution,
}

#[derive(Clone, Debug, Serialize)]
struct StraceSummary {
    observable: bool,
    query_id: Option<String>,
    pread_calls: u64,
    bytes_returned: u64,
    nonsequential_transitions: u64,
    page_sized_reads: u64,
    sqlite_header_checks: u64,
    note: String,
}

#[derive(Clone, Debug, Serialize)]
struct ParetoPoint {
    experiment_id: String,
    archive_bytes: u64,
    expansion_ratio: f64,
    p95_physical_reads: f64,
    p95_bytes_fetched: f64,
    p95_local_query_us: f64,
    p95_simulated_20ms_ms: f64,
}

#[derive(Clone, Debug, Serialize)]
struct ImprovementSummary {
    name: String,
    evidence: String,
    before_experiment_id: String,
    after_experiment_id: String,
    archive_bytes_before: u64,
    archive_bytes_after: u64,
    bytes_saved: u64,
    duplicate_entries_removed: u64,
    p95_bytes_before: f64,
    p95_bytes_after: f64,
    p95_reads_before: f64,
    p95_reads_after: f64,
    p95_20ms_before: f64,
    p95_20ms_after: f64,
    correctness_after: bool,
}

/// Runs the complete fixed-window baseline matrix and retains its evidence files.
///
/// # Errors
///
/// Returns an error if the input cannot be decoded, a baseline or candidate
/// cannot be built, any correctness comparison fails, or results cannot be
/// written without overwriting existing evidence.
#[allow(clippy::too_many_lines)]
pub fn run_fixed_window_experiment(options: &ExperimentOptions) -> ExperimentResult<()> {
    validate_options(options)?;
    fs::create_dir_all(&options.results_dir)?;
    fs::create_dir_all(&options.scratch_dir)?;
    let source_bytes = fs::metadata(&options.input)?.len();
    let source_sha256 = file_sha256(&options.input)?;
    let candidate_plan = match options.mode {
        ExperimentMode::FullSweep => json!({
            "window_sizes": WINDOW_SIZES,
            "compressions": CODECS,
            "deduplication": "measured follow-up selected from the baseline Pareto frontier",
        }),
        ExperimentMode::SingleConfigSmoke => json!({
            "window_sizes": [262_144_u64],
            "compressions": [ChunkCodec::Zstd6],
            "deduplication": "enabled",
            "purpose": "correctness and scale smoke test of the previously selected configuration; not a layout comparison",
        }),
    };
    write_json(
        &options.results_dir.join("config.json"),
        &json!({
            "schema_version": 1,
            "project": "pangenome-range",
            "run_id": options.run_id,
            "experiment_mode": options.mode,
            "input": options.input,
            "source_gbz_bytes": source_bytes,
            "source_sha256": source_sha256,
            "benchmark_command": format!("pangenome-range benchmark-fixed-windows {} {} {}", options.input.display(), options.run_id, options.random_queries_per_size),
            "execution_environment": {
                "os": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
                "storage_api": "local positioned FileRangeSource reads",
                "cache_state": "uncontrolled; interpret local timings as warm/mixed, not cold-cache claims",
            },
            "network_profiles": [
                {"label": "A", "rtt_ms": 20, "bandwidth_mbps": 300, "max_parallel_requests": 6, "per_request_overhead_ms": 0.5},
                {"label": "B", "rtt_ms": 50, "bandwidth_mbps": 100, "max_parallel_requests": 6, "per_request_overhead_ms": 1.0},
                {"label": "C", "rtt_ms": 100, "bandwidth_mbps": 30, "max_parallel_requests": 4, "per_request_overhead_ms": 2.0},
            ],
            "fixed_rng_seed": RNG_SEED,
            "random_queries_per_available_size": options.random_queries_per_size,
            "requested_query_sizes": [1_000_u64, 10_000, 100_000, 1_000_000],
            "context": CONTEXT,
            "window_sizes": match options.mode {
                ExperimentMode::FullSweep => WINDOW_SIZES.to_vec(),
                ExperimentMode::SingleConfigSmoke => vec![262_144_u64],
            },
            "compressions": match options.mode {
                ExperimentMode::FullSweep => CODECS.to_vec(),
                ExperimentMode::SingleConfigSmoke => vec![ChunkCodec::Zstd6],
            },
            "candidate_plan": candidate_plan,
            "coalescing_gaps": COALESCING_GAPS,
            "construction_boundary_strategy": "duplicate with 100 bp graph context halo",
            "physical_order": "source reference-path order, then coordinate within each path",
            "bootstrap_prefix_bytes": 16_384,
            "local_timing_scope": "storage lookup, decompression/decode, and subgraph reconstruction; correctness serialization/hash/comparison instrumentation excluded",
            "improvement_policy": match options.mode {
                ExperimentMode::FullSweep => "after the baseline sweep, apply exact content-addressed chunk deduplication to the latency-first Pareto point that contains measured repeated payloads",
                ExperimentMode::SingleConfigSmoke => "no improvement search; measure the previously selected deduplicated configuration only",
            },
        }),
    )?;

    eprintln!("loading source GBZ {}", options.input.display());
    let load_started = Instant::now();
    let graph: GBZ = serialize::load_from(&options.input)?;
    let gbz_load_wall_ms = elapsed_ms(load_started);
    let index_started = Instant::now();
    let path_index = PathIndex::new(&graph, PATH_INDEX_INTERVAL, false)?;
    let path_index_wall_ms = elapsed_ms(index_started);
    let references = reference_paths(&graph)?;
    let (workload, skipped_sizes) =
        deterministic_workload(&references, options.random_queries_per_size);
    if workload.is_empty() {
        return Err(invalid_input("no valid benchmark queries could be generated").into());
    }
    eprintln!(
        "workload: {} queries ({} hard loci), path index {:.3} ms",
        workload.len(),
        workload
            .iter()
            .filter(|query| query.class.starts_with("hard-"))
            .count(),
        path_index_wall_ms
    );

    let mut query_rows = Vec::new();
    let mut source_json = BTreeMap::new();
    let mut gbz_query_times = Vec::new();
    let mut oracles: BTreeMap<String, OracleResult> = BTreeMap::new();
    for query in &workload {
        let (source_output, query_us) = upstream_query_json(&graph, &path_index, query)?;
        gbz_query_times.push(query_us);
        source_json.insert(query.id.clone(), source_output);
        let oracle = source_oracle(&graph, &path_index, query)?;
        query_rows.push(baseline_row(
            "gbz",
            "gbz-baseline",
            query,
            query_us,
            Some((1, 1, source_bytes)),
            true,
            Some((
                NetworkProfile::GOOD_CDN
                    .estimate_plan(1, 1, source_bytes)
                    .estimated_total_ms,
                NetworkProfile::MODERATE_INTERNET
                    .estimate_plan(1, 1, source_bytes)
                    .estimated_total_ms,
                NetworkProfile::POOR_MOBILE
                    .estimate_plan(1, 1, source_bytes)
                    .estimated_total_ms,
            )),
        ));
        oracles.insert(query.id.clone(), oracle);
    }

    let database_path = options.scratch_dir.join("gbz-base.sqlite");
    eprintln!("building GBZ-base baseline");
    let database_started = Instant::now();
    GBZBase::create_from_files(&options.input, None, &database_path)?;
    let gbz_base_build_wall_ms = elapsed_ms(database_started);
    let gbz_base_bytes = fs::metadata(&database_path)?.len();
    let open_started = Instant::now();
    let database = GBZBase::open(&database_path)?;
    let mut interface = GraphInterface::new(&database)?;
    let gbz_base_open_wall_ms = elapsed_ms(open_started);
    let mut gbz_base_query_times = Vec::new();
    let mut gbz_base_correctness = true;
    for query in &workload {
        let (output, query_us) = database_query_json(&mut interface, query)?;
        let correctness = output == source_json[&query.id];
        gbz_base_correctness &= correctness;
        gbz_base_query_times.push(query_us);
        query_rows.push(baseline_row(
            "gbz-base",
            "gbz-base-baseline",
            query,
            query_us,
            None,
            correctness,
            None,
        ));
    }
    drop(interface);
    drop(database);
    let sqlite_io_observation =
        observe_sqlite_io(&database_path, &workload[0], &options.scratch_dir);

    let mut candidate_runs = Vec::new();
    for config in planned_configs(options.mode) {
        let run = run_candidate(
            &graph,
            &path_index,
            source_bytes,
            &workload,
            &oracles,
            &options.scratch_dir,
            config,
            false,
        )?;
        append_candidate_rows(&mut query_rows, &workload, &run.measurements)?;
        candidate_runs.push(run);
    }

    let improvement_evidence = if options.mode == ExperimentMode::FullSweep {
        let baseline_aggregates = aggregates(&candidate_runs);
        let baseline_pareto = pareto_frontier(&candidate_runs, &baseline_aggregates, false);
        let improvement_source = choose_dedup_source(&candidate_runs, &baseline_pareto)?;
        let evidence = format!(
            "{} exact repeated chunk payload entries accounted for {} avoidable compressed bytes ({:.1}% of the measured baseline archive)",
            improvement_source.build.duplicate_payload_entries_observed,
            improvement_source.build.avoidable_compressed_payload_bytes,
            100.0
                * ratio(
                    improvement_source.build.avoidable_compressed_payload_bytes,
                    improvement_source.build.archive_bytes,
                )
        );
        eprintln!(
            "measured next step: exact chunk deduplication for {} ({})",
            improvement_source.config.experiment_id, evidence
        );
        let improved_config = FixedArchiveConfig {
            experiment_id: format!("{}-dedup", improvement_source.config.experiment_id),
            window_size: improvement_source.config.window_size,
            codec: improvement_source.config.codec,
            deduplicate_chunks: true,
        };
        let improved_run = run_candidate(
            &graph,
            &path_index,
            source_bytes,
            &workload,
            &oracles,
            &options.scratch_dir,
            improved_config,
            true,
        )?;
        append_candidate_rows(&mut query_rows, &workload, &improved_run.measurements)?;
        candidate_runs.push(improved_run);
        Some(evidence)
    } else {
        None
    };

    let all_aggregates = aggregates(&candidate_runs);
    let baseline_query_aggregates = baseline_query_aggregates(&query_rows);
    let final_pareto = pareto_frontier(&candidate_runs, &all_aggregates, true);
    let latency_first = final_pareto
        .iter()
        .min_by(|left, right| {
            left.p95_simulated_20ms_ms
                .total_cmp(&right.p95_simulated_20ms_ms)
                .then_with(|| left.archive_bytes.cmp(&right.archive_bytes))
        })
        .ok_or_else(|| invalid_data("Pareto frontier is empty"))?;
    let improvement = improvement_evidence
        .as_deref()
        .map(|evidence| improvement_summary(&candidate_runs, &all_aggregates, evidence))
        .transpose()?;

    let baselines = BaselineSummary {
        gbz_bytes: source_bytes,
        gbz_load_wall_ms,
        gbz_load_behavior: "The complete compressed GBZ is deserialized before interval queries; cold object access therefore requires the whole object.".into(),
        gbz_query_limitations: "GBZ is the compression oracle, but it is not range-addressable by reference interval in this prototype.".into(),
        gbz_query_us: distribution(gbz_query_times),
        gbz_base_bytes,
        gbz_base_to_gbz_ratio: ratio(gbz_base_bytes, source_bytes),
        gbz_base_build_wall_ms,
        gbz_base_open_wall_ms,
        gbz_base_query_us: distribution(gbz_base_query_times),
        gbz_base_correctness,
        sqlite_io_observation,
    };
    let process_peak_rss_kib = process_peak_rss_kib();
    let summary = json!({
        "schema_version": 1,
        "run_id": options.run_id,
        "experiment_mode": options.mode,
        "fixture_scale_warning": "Results are input-specific. Archive/index behavior must be revalidated at chromosome and whole-genome scale; smoke mode does not establish a new layout winner.",
        "workload": {
            "queries": workload,
            "skipped_query_sizes": skipped_sizes,
            "fixed_rng_seed": RNG_SEED,
        },
        "baselines": baselines,
        "baseline_query_aggregates": baseline_query_aggregates,
        "candidate_archives": candidate_runs.iter().map(|run| json!({
            "config": run.config,
            "build": run.build,
            "structural_improvement": run.improvement,
        })).collect::<Vec<_>>(),
        "query_aggregates": all_aggregates,
        "pareto_frontier": final_pareto,
        "latency_first_pareto_point": latency_first,
        "structural_improvement": improvement,
        "construction": {
            "path_index_wall_ms": path_index_wall_ms,
            "process_peak_rss_kib": process_peak_rss_kib,
            "peak_rss_scope": "whole benchmark process; per-phase RSS is not inferred",
            "temporary_disk_bytes_at_completion": directory_size(&options.scratch_dir)?,
        },
        "correctness": {
            "gbz_base_all_queries": gbz_base_correctness,
            "candidate_all_queries": candidate_runs.iter().flat_map(|run| &run.measurements).all(|measurement| measurement.correctness),
        },
    });
    write_queries_csv(&options.results_dir.join("queries.csv"), &query_rows)?;
    write_json(&options.results_dir.join("summary.json"), &summary)?;
    write_report(
        &options.results_dir.join("REPORT.md"),
        options,
        &baselines,
        &baseline_query_aggregates,
        &candidate_runs,
        &all_aggregates,
        &final_pareto,
        latency_first,
        improvement.as_ref(),
        &skipped_sizes,
        process_peak_rss_kib,
    )?;
    eprintln!("retained results in {}", options.results_dir.display());
    Ok(())
}

fn planned_configs(mode: ExperimentMode) -> Vec<FixedArchiveConfig> {
    match mode {
        ExperimentMode::FullSweep => WINDOW_SIZES
            .into_iter()
            .flat_map(|window_size| {
                CODECS.into_iter().map(move |codec| FixedArchiveConfig {
                    experiment_id: format!("fixed-w{}-{}", size_label(window_size), codec.name()),
                    window_size,
                    codec,
                    deduplicate_chunks: false,
                })
            })
            .collect(),
        ExperimentMode::SingleConfigSmoke => vec![FixedArchiveConfig {
            experiment_id: "fixed-w256k-zstd-6-dedup".into(),
            window_size: 262_144,
            codec: ChunkCodec::Zstd6,
            deduplicate_chunks: true,
        }],
    }
}

fn validate_options(options: &ExperimentOptions) -> io::Result<()> {
    if options.run_id.trim().is_empty() {
        return Err(invalid_input("run ID must not be empty"));
    }
    if options.random_queries_per_size == 0 {
        return Err(invalid_input(
            "random query count must be greater than zero",
        ));
    }
    for path in [&options.results_dir, &options.scratch_dir] {
        if path.exists() && fs::read_dir(path)?.next().transpose()?.is_some() {
            return Err(invalid_input(format!(
                "refusing to overwrite non-empty directory {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn deterministic_workload(
    references: &[super::fixed::ReferencePathSpec],
    random_queries_per_size: usize,
) -> (Vec<QuerySpec>, Vec<u64>) {
    let mut workload = Vec::new();
    let hard_loci = [
        ("MICB", "GRCh38", "chr6", 31_498_145_u64, 31_511_124_u64),
        ("KIR3DL1", "GRCh38", "chr19", 54_816_436_u64, 54_830_779_u64),
    ];
    for (name, sample, contig, start, end) in hard_loci {
        if references.iter().any(|reference| {
            reference.name.sample == sample
                && reference.name.contig == contig
                && reference.start <= start
                && reference.end >= end
        }) {
            workload.push(QuerySpec {
                id: format!("hard-{}", name.to_ascii_lowercase()),
                class: format!("hard-{name}"),
                sample: sample.into(),
                contig: contig.into(),
                start,
                end,
                context: CONTEXT,
            });
        }
    }

    let mut rng = XorShift64::new(RNG_SEED);
    let mut skipped = Vec::new();
    for size in [1_000_u64, 10_000, 100_000, 1_000_000] {
        let eligible = references
            .iter()
            .filter(|reference| reference.end.saturating_sub(reference.start) >= size)
            .collect::<Vec<_>>();
        if eligible.is_empty() {
            skipped.push(size);
            continue;
        }
        for ordinal in 0..random_queries_per_size {
            let reference = eligible[rng.index(eligible.len())];
            let max_delta = reference.end - reference.start - size;
            let start = reference.start + rng.bounded(max_delta.saturating_add(1));
            workload.push(QuerySpec {
                id: format!("random-{size}-{ordinal:05}"),
                class: format!("random-{size}"),
                sample: reference.name.sample.clone(),
                contig: reference.name.contig.clone(),
                start,
                end: start + size,
                context: CONTEXT,
            });
        }
    }
    (workload, skipped)
}

fn upstream_query_json(
    graph: &GBZ,
    path_index: &PathIndex,
    query: &QuerySpec,
) -> ExperimentResult<(Vec<u8>, f64)> {
    let name = FullPathName::reference(&query.sample, &query.contig);
    let request = SubgraphQuery::path_interval(
        &name,
        usize::try_from(query.start)?..usize::try_from(query.end)?,
    )
    .with_context(usize::try_from(query.context)?)
    .with_haplotypes(HaplotypeOutput::All);
    let mut subgraph = Subgraph::new();
    let started = Instant::now();
    subgraph.from_gbz(graph, Some(path_index), None, &request)?;
    let query_us = elapsed_us(started);
    let mut output = Vec::new();
    subgraph.write_json(&mut output, false)?;
    Ok((output, query_us))
}

fn database_query_json(
    interface: &mut GraphInterface<'_>,
    query: &QuerySpec,
) -> ExperimentResult<(Vec<u8>, f64)> {
    let name = FullPathName::reference(&query.sample, &query.contig);
    let request = SubgraphQuery::path_interval(
        &name,
        usize::try_from(query.start)?..usize::try_from(query.end)?,
    )
    .with_context(usize::try_from(query.context)?)
    .with_haplotypes(HaplotypeOutput::All);
    let mut subgraph = Subgraph::new();
    let started = Instant::now();
    subgraph.from_db(interface, &request)?;
    let query_us = elapsed_us(started);
    let mut output = Vec::new();
    subgraph.write_json(&mut output, false)?;
    Ok((output, query_us))
}

#[allow(clippy::too_many_arguments)]
fn run_candidate(
    graph: &GBZ,
    path_index: &PathIndex,
    source_bytes: u64,
    workload: &[QuerySpec],
    oracles: &BTreeMap<String, OracleResult>,
    scratch_dir: &Path,
    config: FixedArchiveConfig,
    improvement: bool,
) -> ExperimentResult<CandidateRun> {
    eprintln!("building/querying {}", config.experiment_id);
    let archive = scratch_dir.join(format!("{}.pngr", config.experiment_id));
    let build = build_fixed_archive(graph, path_index, source_bytes, &archive, &config)?;
    let mut measurements = Vec::with_capacity(workload.len() * COALESCING_GAPS.len());
    for &gap in &COALESCING_GAPS {
        for query in workload {
            let oracle = oracles
                .get(&query.id)
                .ok_or_else(|| invalid_data(format!("missing oracle for {}", query.id)))?;
            let measurement = query_fixed_archive(&archive, &config, query, gap, oracle)?;
            if !measurement.correctness {
                return Err(invalid_data(format!(
                    "candidate {} failed correctness for query {} at coalescing gap {}",
                    config.experiment_id, query.id, gap
                ))
                .into());
            }
            measurements.push(measurement);
        }
    }
    Ok(CandidateRun {
        config,
        build,
        measurements,
        improvement,
    })
}

fn append_candidate_rows(
    rows: &mut Vec<QueryRow>,
    workload: &[QuerySpec],
    measurements: &[QueryMeasurement],
) -> ExperimentResult<()> {
    let queries = workload
        .iter()
        .map(|query| (query.id.as_str(), query))
        .collect::<BTreeMap<_, _>>();
    for measurement in measurements {
        let query = queries
            .get(measurement.query_id.as_str())
            .ok_or_else(|| invalid_data(format!("unknown query {}", measurement.query_id)))?;
        rows.push(QueryRow {
            storage_kind: "fixed-window".into(),
            experiment_id: measurement.experiment_id.clone(),
            query_id: measurement.query_id.clone(),
            query_class: measurement.query_class.clone(),
            sample: query.sample.clone(),
            contig: query.contig.clone(),
            start: query.start,
            end: query.end,
            query_size: measurement.query_size,
            context: query.context,
            coalescing_gap: Some(measurement.coalescing_gap),
            physical_reads: Some(measurement.physical_reads),
            mergeable_reads: Some(measurement.mergeable_reads),
            dependency_rounds: Some(measurement.dependency_rounds),
            total_bytes_fetched: Some(measurement.total_bytes_fetched),
            unique_bytes_fetched: Some(measurement.unique_bytes_fetched),
            duplicate_bytes_fetched: Some(measurement.duplicate_bytes_fetched),
            bootstrap_bytes_fetched: Some(measurement.bootstrap_bytes_fetched),
            logical_index_bytes: Some(measurement.logical_index_bytes),
            data_bytes_fetched: Some(measurement.data_bytes_fetched),
            required_compressed_payload_bytes: Some(measurement.required_compressed_payload_bytes),
            canonical_payload_bytes: Some(measurement.canonical_payload_bytes),
            read_amplification: Some(measurement.read_amplification),
            canonical_amplification: Some(measurement.canonical_amplification),
            index_lookup_us: Some(measurement.index_lookup_us),
            decompression_us: Some(measurement.decompression_us),
            decode_us: Some(measurement.decode_us),
            graph_reconstruction_us: Some(measurement.graph_reconstruction_us),
            total_local_query_us: measurement.total_local_query_us,
            selected_chunks: Some(measurement.selected_chunks),
            selected_nodes: Some(measurement.selected_nodes),
            canonical_hash: Some(measurement.canonical_hash.clone()),
            correctness: measurement.correctness,
            simulated_20ms_ms: Some(measurement.simulated_20ms_ms),
            simulated_50ms_ms: Some(measurement.simulated_50ms_ms),
            simulated_100ms_ms: Some(measurement.simulated_100ms_ms),
        });
    }
    Ok(())
}

fn baseline_row(
    storage_kind: &str,
    experiment_id: &str,
    query: &QuerySpec,
    total_local_query_us: f64,
    io_plan: Option<(u64, u64, u64)>,
    correctness: bool,
    simulated: Option<(f64, f64, f64)>,
) -> QueryRow {
    let (physical_reads, dependency_rounds, total_bytes) = io_plan
        .map_or((None, None, None), |plan| {
            (Some(plan.0), Some(plan.1), Some(plan.2))
        });
    QueryRow {
        storage_kind: storage_kind.into(),
        experiment_id: experiment_id.into(),
        query_id: query.id.clone(),
        query_class: query.class.clone(),
        sample: query.sample.clone(),
        contig: query.contig.clone(),
        start: query.start,
        end: query.end,
        query_size: query.length(),
        context: query.context,
        coalescing_gap: None,
        physical_reads,
        mergeable_reads: None,
        dependency_rounds,
        total_bytes_fetched: total_bytes,
        unique_bytes_fetched: total_bytes,
        duplicate_bytes_fetched: total_bytes.is_some().then_some(0),
        bootstrap_bytes_fetched: None,
        logical_index_bytes: None,
        data_bytes_fetched: total_bytes,
        required_compressed_payload_bytes: None,
        canonical_payload_bytes: None,
        read_amplification: None,
        canonical_amplification: None,
        index_lookup_us: None,
        decompression_us: None,
        decode_us: None,
        graph_reconstruction_us: None,
        total_local_query_us,
        selected_chunks: None,
        selected_nodes: None,
        canonical_hash: None,
        correctness,
        simulated_20ms_ms: simulated.map(|values| values.0),
        simulated_50ms_ms: simulated.map(|values| values.1),
        simulated_100ms_ms: simulated.map(|values| values.2),
    }
}

fn baseline_query_aggregates(rows: &[QueryRow]) -> Vec<BaselineQueryAggregate> {
    let mut grouped: BTreeMap<(String, String), Vec<f64>> = BTreeMap::new();
    for row in rows
        .iter()
        .filter(|row| matches!(row.storage_kind.as_str(), "gbz" | "gbz-base"))
    {
        grouped
            .entry((row.storage_kind.clone(), "all".into()))
            .or_default()
            .push(row.total_local_query_us);
        grouped
            .entry((row.storage_kind.clone(), row.query_class.clone()))
            .or_default()
            .push(row.total_local_query_us);
    }
    grouped
        .into_iter()
        .map(
            |((storage_kind, query_group), values)| BaselineQueryAggregate {
                storage_kind,
                query_group,
                local_query_us: distribution(values),
            },
        )
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn aggregates(runs: &[CandidateRun]) -> Vec<QueryAggregate> {
    let mut result = Vec::new();
    for run in runs {
        for &gap in &COALESCING_GAPS {
            let at_gap = run
                .measurements
                .iter()
                .filter(|measurement| measurement.coalescing_gap == gap)
                .collect::<Vec<_>>();
            let mut groups = BTreeSet::from([String::from("all")]);
            groups.extend(
                at_gap
                    .iter()
                    .map(|measurement| measurement.query_class.clone()),
            );
            for group in groups {
                let selected = at_gap
                    .iter()
                    .copied()
                    .filter(|measurement| group == "all" || measurement.query_class == group)
                    .collect::<Vec<_>>();
                result.push(QueryAggregate {
                    experiment_id: run.config.experiment_id.clone(),
                    coalescing_gap: gap,
                    query_group: group,
                    correctness: selected.iter().all(|measurement| measurement.correctness),
                    physical_reads: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.physical_reads as f64)
                            .collect(),
                    ),
                    bytes_fetched: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.total_bytes_fetched as f64)
                            .collect(),
                    ),
                    read_amplification: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.read_amplification)
                            .collect(),
                    ),
                    index_lookup_us: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.index_lookup_us)
                            .collect(),
                    ),
                    decompression_us: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.decompression_us)
                            .collect(),
                    ),
                    decode_us: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.decode_us)
                            .collect(),
                    ),
                    graph_reconstruction_us: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.graph_reconstruction_us)
                            .collect(),
                    ),
                    local_query_us: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.total_local_query_us)
                            .collect(),
                    ),
                    simulated_20ms_ms: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.simulated_20ms_ms)
                            .collect(),
                    ),
                    simulated_50ms_ms: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.simulated_50ms_ms)
                            .collect(),
                    ),
                    simulated_100ms_ms: distribution(
                        selected
                            .iter()
                            .map(|measurement| measurement.simulated_100ms_ms)
                            .collect(),
                    ),
                });
            }
        }
    }
    result
}

fn pareto_frontier(
    runs: &[CandidateRun],
    aggregates: &[QueryAggregate],
    include_improvement: bool,
) -> Vec<ParetoPoint> {
    let mut points = runs
        .iter()
        .filter(|run| include_improvement || !run.improvement)
        .filter_map(|run| {
            let aggregate =
                find_aggregate(aggregates, &run.config.experiment_id, PARETO_GAP, "all")?;
            Some(ParetoPoint {
                experiment_id: run.config.experiment_id.clone(),
                archive_bytes: run.build.archive_bytes,
                expansion_ratio: run.build.expansion_ratio,
                p95_physical_reads: aggregate.physical_reads.p95,
                p95_bytes_fetched: aggregate.bytes_fetched.p95,
                p95_local_query_us: aggregate.local_query_us.p95,
                p95_simulated_20ms_ms: aggregate.simulated_20ms_ms.p95,
            })
        })
        .collect::<Vec<_>>();
    let snapshot = points.clone();
    points.retain(|candidate| {
        !snapshot.iter().any(|other| {
            other.experiment_id != candidate.experiment_id && dominates(other, candidate)
        })
    });
    points.sort_by(|left, right| {
        left.archive_bytes
            .cmp(&right.archive_bytes)
            .then_with(|| left.p95_bytes_fetched.total_cmp(&right.p95_bytes_fetched))
    });
    points
}

fn dominates(left: &ParetoPoint, right: &ParetoPoint) -> bool {
    let no_worse = left.archive_bytes <= right.archive_bytes
        && left.p95_physical_reads <= right.p95_physical_reads
        && left.p95_bytes_fetched <= right.p95_bytes_fetched
        && left.p95_local_query_us <= right.p95_local_query_us
        && left.p95_simulated_20ms_ms <= right.p95_simulated_20ms_ms;
    let strictly_better = left.archive_bytes < right.archive_bytes
        || left.p95_physical_reads < right.p95_physical_reads
        || left.p95_bytes_fetched < right.p95_bytes_fetched
        || left.p95_local_query_us < right.p95_local_query_us
        || left.p95_simulated_20ms_ms < right.p95_simulated_20ms_ms;
    no_worse && strictly_better
}

fn choose_dedup_source<'a>(
    runs: &'a [CandidateRun],
    pareto: &[ParetoPoint],
) -> ExperimentResult<&'a CandidateRun> {
    let best = pareto
        .iter()
        .filter_map(|point| {
            let run = runs
                .iter()
                .find(|run| run.config.experiment_id == point.experiment_id)?;
            (run.build.avoidable_compressed_payload_bytes > 0).then_some((point, run))
        })
        .min_by(|(left, _), (right, _)| {
            left.p95_simulated_20ms_ms
                .total_cmp(&right.p95_simulated_20ms_ms)
                .then_with(|| left.archive_bytes.cmp(&right.archive_bytes))
                .then_with(|| left.experiment_id.cmp(&right.experiment_id))
        })
        .map(|(_, run)| run)
        .or_else(|| {
            runs.iter()
                .max_by_key(|run| run.build.avoidable_compressed_payload_bytes)
        })
        .ok_or_else(|| invalid_data("no candidate run available for improvement"))?;
    if best.build.avoidable_compressed_payload_bytes == 0 {
        return Err(invalid_data(
            "baseline sweep found no exact repeated chunks; refusing an unjustified deduplication experiment",
        )
        .into());
    }
    Ok(best)
}

fn improvement_summary(
    runs: &[CandidateRun],
    aggregates: &[QueryAggregate],
    evidence: &str,
) -> ExperimentResult<ImprovementSummary> {
    let after = runs
        .iter()
        .find(|run| run.improvement)
        .ok_or_else(|| invalid_data("missing improvement run"))?;
    let before_id = after
        .config
        .experiment_id
        .strip_suffix("-dedup")
        .ok_or_else(|| invalid_data("improvement ID does not name its baseline"))?;
    let before = runs
        .iter()
        .find(|run| run.config.experiment_id == before_id)
        .ok_or_else(|| invalid_data("missing improvement baseline"))?;
    let before_query = find_aggregate(aggregates, before_id, PARETO_GAP, "all")
        .ok_or_else(|| invalid_data("missing baseline aggregate"))?;
    let after_query = find_aggregate(aggregates, &after.config.experiment_id, PARETO_GAP, "all")
        .ok_or_else(|| invalid_data("missing improvement aggregate"))?;
    Ok(ImprovementSummary {
        name: "exact content-addressed chunk deduplication".into(),
        evidence: evidence.into(),
        before_experiment_id: before_id.into(),
        after_experiment_id: after.config.experiment_id.clone(),
        archive_bytes_before: before.build.archive_bytes,
        archive_bytes_after: after.build.archive_bytes,
        bytes_saved: before
            .build
            .archive_bytes
            .saturating_sub(after.build.archive_bytes),
        duplicate_entries_removed: after.build.deduplicated_entries,
        p95_bytes_before: before_query.bytes_fetched.p95,
        p95_bytes_after: after_query.bytes_fetched.p95,
        p95_reads_before: before_query.physical_reads.p95,
        p95_reads_after: after_query.physical_reads.p95,
        p95_20ms_before: before_query.simulated_20ms_ms.p95,
        p95_20ms_after: after_query.simulated_20ms_ms.p95,
        correctness_after: after_query.correctness,
    })
}

fn find_aggregate<'a>(
    aggregates: &'a [QueryAggregate],
    experiment_id: &str,
    gap: u64,
    group: &str,
) -> Option<&'a QueryAggregate> {
    aggregates.iter().find(|aggregate| {
        aggregate.experiment_id == experiment_id
            && aggregate.coalescing_gap == gap
            && aggregate.query_group == group
    })
}

fn find_baseline_query_aggregate<'a>(
    aggregates: &'a [BaselineQueryAggregate],
    storage_kind: &str,
    group: &str,
) -> Option<&'a BaselineQueryAggregate> {
    aggregates
        .iter()
        .find(|aggregate| aggregate.storage_kind == storage_kind && aggregate.query_group == group)
}

fn observe_sqlite_io(database: &Path, query: &QuerySpec, scratch: &Path) -> StraceSummary {
    let Some(executable) = std::env::current_exe().ok() else {
        return unavailable_strace("current executable path was unavailable");
    };
    let trace_path = scratch.join("sqlite-pread.trace");
    let status = Command::new("strace")
        .args(["-qq", "-yy", "-e", "trace=pread64", "-o"])
        .arg(&trace_path)
        .arg(executable)
        .arg("internal-gbz-base-query")
        .arg(database)
        .args([
            &query.sample,
            &query.contig,
            &query.start.to_string(),
            &query.end.to_string(),
            &query.context.to_string(),
        ])
        .status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            return unavailable_strace(&format!("strace child exited with {status}"));
        }
        Err(error) => return unavailable_strace(&format!("strace unavailable: {error}")),
    }
    let Ok(contents) = fs::read_to_string(&trace_path) else {
        return unavailable_strace("strace output could not be read");
    };
    let database_name = database
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let mut calls = 0_u64;
    let mut bytes = 0_u64;
    let mut nonsequential = 0_u64;
    let mut page_sized_reads = 0_u64;
    let mut sqlite_header_checks = 0_u64;
    let mut previous_end = None;
    for line in contents.lines().filter(|line| line.contains(database_name)) {
        let Some((call, result)) = line.rsplit_once(" = ") else {
            continue;
        };
        let Ok(returned) = result
            .split_whitespace()
            .next()
            .unwrap_or("")
            .parse::<u64>()
        else {
            continue;
        };
        let offset = call
            .trim_end_matches(')')
            .rsplit(',')
            .next()
            .and_then(|value| value.trim().parse::<u64>().ok());
        if let Some(offset) = offset {
            if previous_end.is_some_and(|end| end != offset) {
                nonsequential += 1;
            }
            previous_end = Some(offset.saturating_add(returned));
            page_sized_reads += u64::from(returned == 4_096);
            sqlite_header_checks += u64::from(returned == 16 && offset == 24);
        }
        calls += 1;
        bytes = bytes.saturating_add(returned);
    }
    StraceSummary {
        observable: calls > 0,
        query_id: Some(query.id.clone()),
        pread_calls: calls,
        bytes_returned: bytes,
        nonsequential_transitions: nonsequential,
        page_sized_reads,
        sqlite_header_checks,
        note: if calls > 0 {
            "Representative process-level SQLite pread64 trace; page-cache residency is not inferred."
                .into()
        } else {
            "strace ran, but no database pread64 calls were identifiable.".into()
        },
    }
}

fn unavailable_strace(note: &str) -> StraceSummary {
    StraceSummary {
        observable: false,
        query_id: None,
        pread_calls: 0,
        bytes_returned: 0,
        nonsequential_transitions: 0,
        page_sized_reads: 0,
        sqlite_header_checks: 0,
        note: note.into(),
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn write_report(
    path: &Path,
    options: &ExperimentOptions,
    baselines: &BaselineSummary,
    baseline_query_aggregates: &[BaselineQueryAggregate],
    runs: &[CandidateRun],
    aggregates: &[QueryAggregate],
    pareto: &[ParetoPoint],
    latency_first: &ParetoPoint,
    improvement: Option<&ImprovementSummary>,
    skipped_sizes: &[u64],
    peak_rss_kib: Option<u64>,
) -> ExperimentResult<()> {
    let mut output = BufWriter::new(File::create(path)?);
    writeln!(
        output,
        "# Fixed-window cloud-layout experiment: {}\n",
        options.run_id
    )?;
    writeln!(
        output,
        "This is a measured Candidate 0 result for `{}` in `{}` mode, not a format recommendation. All candidate rows passed node, sequence, edge, path-multiplicity/orientation, and reference-coordinate canonical comparison.\n",
        options.input.display(),
        match options.mode {
            ExperimentMode::FullSweep => "full-sweep",
            ExperimentMode::SingleConfigSmoke => "single-config-smoke",
        }
    )?;
    writeln!(output, "## Baselines\n")?;
    writeln!(
        output,
        "| Baseline | Size | vs GBZ | Build/load | Query p50 / p95 | Storage behavior |"
    )?;
    writeln!(output, "|---|---:|---:|---:|---:|---|")?;
    writeln!(
        output,
        "| GBZ | {} B | 1.000x | load {:.3} ms | {:.1} / {:.1} us | whole graph must be loaded before interval extraction |",
        baselines.gbz_bytes,
        baselines.gbz_load_wall_ms,
        baselines.gbz_query_us.p50,
        baselines.gbz_query_us.p95
    )?;
    writeln!(
        output,
        "| GBZ-base | {} B | {:.3}x | build {:.3} ms; open {:.3} ms | {:.1} / {:.1} us | local SQLite random access; not a static-object range layout |\n",
        baselines.gbz_base_bytes,
        baselines.gbz_base_to_gbz_ratio,
        baselines.gbz_base_build_wall_ms,
        baselines.gbz_base_open_wall_ms,
        baselines.gbz_base_query_us.p50,
        baselines.gbz_base_query_us.p95
    )?;
    if baselines.sqlite_io_observation.observable {
        writeln!(
            output,
            "A representative GBZ-base query issued {} identifiable `pread64` calls and returned {} bytes. The trace contained {} page-sized reads, {} repeated 16-byte SQLite header checks, and {} non-sequential offset transitions. This is process-level syscall evidence, not a cold-cache byte count.\n",
            baselines.sqlite_io_observation.pread_calls,
            baselines.sqlite_io_observation.bytes_returned,
            baselines.sqlite_io_observation.page_sized_reads,
            baselines.sqlite_io_observation.sqlite_header_checks,
            baselines.sqlite_io_observation.nonsequential_transitions
        )?;
    } else {
        writeln!(
            output,
            "SQLite page I/O was not observable in this run: {}\n",
            baselines.sqlite_io_observation.note
        )?;
    }

    writeln!(output, "## Fixed-window candidates\n")?;
    writeln!(
        output,
        "The table contains every configuration executed by this mode and uses the 64 KiB coalescing threshold across all query classes. Network p95 is the 20 ms / 300 Mbps profile with dependency rounds enforced.\n"
    )?;
    writeln!(
        output,
        "| Experiment | Archive | vs GBZ | Index | Chunks | p50 / p95 bytes | p95 reads | p95 local | p95 network | Correct |"
    )?;
    writeln!(
        output,
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|"
    )?;
    for run in runs.iter().filter(|run| !run.improvement) {
        let aggregate = find_aggregate(aggregates, &run.config.experiment_id, PARETO_GAP, "all")
            .ok_or_else(|| invalid_data("missing report aggregate"))?;
        writeln!(
            output,
            "| {} | {} B | {:.3}x | {} B ({:.2}%) | {} | {:.0} / {:.0} | {:.0} | {:.1} us | {:.2} ms | {} |",
            run.config.experiment_id,
            run.build.archive_bytes,
            run.build.expansion_ratio,
            run.build.index_bytes,
            run.build.index_ratio * 100.0,
            run.build.physical_chunks,
            aggregate.bytes_fetched.p50,
            aggregate.bytes_fetched.p95,
            aggregate.physical_reads.p95,
            aggregate.local_query_us.p95,
            aggregate.simulated_20ms_ms.p95,
            yes_no(aggregate.correctness)
        )?;
    }

    writeln!(output, "\n## Query-size and hard-locus distributions\n")?;
    writeln!(
        output,
        "Results are not collapsed across size classes in `summary.json` or `queries.csv`. For the latency-first Pareto point at 64 KiB coalescing:\n"
    )?;
    writeln!(
        output,
        "| Query group | Count | p50 / p95 / p99 bytes | p50 / p95 / p99 reads | p50 / p95 / p99 local | p95 20 ms network |"
    )?;
    writeln!(output, "|---|---:|---:|---:|---:|---:|")?;
    let mut groups = aggregates
        .iter()
        .filter(|aggregate| {
            aggregate.experiment_id == latency_first.experiment_id
                && aggregate.coalescing_gap == PARETO_GAP
                && aggregate.query_group != "all"
        })
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| left.query_group.cmp(&right.query_group));
    for aggregate in groups {
        writeln!(
            output,
            "| {} | {} | {:.0} / {:.0} / {:.0} | {:.0} / {:.0} / {:.0} | {:.1} / {:.1} / {:.1} us | {:.2} ms |",
            aggregate.query_group,
            aggregate.bytes_fetched.count,
            aggregate.bytes_fetched.p50,
            aggregate.bytes_fetched.p95,
            aggregate.bytes_fetched.p99,
            aggregate.physical_reads.p50,
            aggregate.physical_reads.p95,
            aggregate.physical_reads.p99,
            aggregate.local_query_us.p50,
            aggregate.local_query_us.p95,
            aggregate.local_query_us.p99,
            aggregate.simulated_20ms_ms.p95
        )?;
    }

    writeln!(output, "\n## Local latency comparison by query size\n")?;
    writeln!(
        output,
        "Local timings use the same deterministic queries. Candidate values use the 64 KiB coalescing threshold. The final column states the direction and factor relative to GBZ-base.\n"
    )?;
    writeln!(
        output,
        "| Query group | GBZ p50 / p95 | GBZ-base p50 / p95 | Candidate p50 / p95 | Candidate vs GBZ-base p95 |"
    )?;
    writeln!(output, "|---|---:|---:|---:|---:|")?;
    let mut comparison_groups = aggregates
        .iter()
        .filter(|aggregate| {
            aggregate.experiment_id == latency_first.experiment_id
                && aggregate.coalescing_gap == PARETO_GAP
                && aggregate.query_group != "all"
        })
        .collect::<Vec<_>>();
    comparison_groups.sort_by(|left, right| left.query_group.cmp(&right.query_group));
    for candidate in comparison_groups {
        let gbz =
            find_baseline_query_aggregate(baseline_query_aggregates, "gbz", &candidate.query_group)
                .ok_or_else(|| invalid_data("missing GBZ query-size aggregate"))?;
        let gbz_base = find_baseline_query_aggregate(
            baseline_query_aggregates,
            "gbz-base",
            &candidate.query_group,
        )
        .ok_or_else(|| invalid_data("missing GBZ-base query-size aggregate"))?;
        writeln!(
            output,
            "| {} | {:.1} / {:.1} us | {:.1} / {:.1} us | {:.1} / {:.1} us | {} |",
            candidate.query_group,
            gbz.local_query_us.p50,
            gbz.local_query_us.p95,
            gbz_base.local_query_us.p50,
            gbz_base.local_query_us.p95,
            candidate.local_query_us.p50,
            candidate.local_query_us.p95,
            relative_latency(gbz_base.local_query_us.p95, candidate.local_query_us.p95)
        )?;
    }

    let selected_cpu = find_aggregate(aggregates, &latency_first.experiment_id, PARETO_GAP, "all")
        .ok_or_else(|| invalid_data("missing selected CPU aggregate"))?;
    writeln!(
        output,
        "\nCPU breakdown for the same selected point (all query classes; microseconds):\n"
    )?;
    writeln!(output, "| Component | p50 | p90 | p95 | p99 | max |")?;
    writeln!(output, "|---|---:|---:|---:|---:|---:|")?;
    for (name, values) in [
        ("index lookup", &selected_cpu.index_lookup_us),
        ("decompression", &selected_cpu.decompression_us),
        ("binary decode", &selected_cpu.decode_us),
        (
            "graph reconstruction",
            &selected_cpu.graph_reconstruction_us,
        ),
        ("total local query", &selected_cpu.local_query_us),
    ] {
        writeln!(
            output,
            "| {name} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |",
            values.p50, values.p90, values.p95, values.p99, values.max
        )?;
    }

    writeln!(output, "\n## Range coalescing\n")?;
    let coalescing_experiment = improvement
        .map_or(latency_first.experiment_id.as_str(), |summary| {
            summary.before_experiment_id.as_str()
        });
    writeln!(output, "For `{coalescing_experiment}`:\n")?;
    writeln!(
        output,
        "| Gap | p50 / p95 reads | p50 / p95 bytes | p95 20 ms network |"
    )?;
    writeln!(output, "|---:|---:|---:|---:|")?;
    for gap in COALESCING_GAPS {
        let aggregate = find_aggregate(aggregates, coalescing_experiment, gap, "all")
            .ok_or_else(|| invalid_data("missing coalescing aggregate"))?;
        writeln!(
            output,
            "| {} B | {:.0} / {:.0} | {:.0} / {:.0} | {:.2} ms |",
            gap,
            aggregate.physical_reads.p50,
            aggregate.physical_reads.p95,
            aggregate.bytes_fetched.p50,
            aggregate.bytes_fetched.p95,
            aggregate.simulated_20ms_ms.p95
        )?;
    }

    if let Some(improvement) = improvement {
        writeln!(output, "\n## Exactly one measured structural improvement\n")?;
        writeln!(
            output,
            "The baseline sweep exposed {}. I therefore implemented only exact content-addressed chunk deduplication: directory entries may share one independently decodable physical payload when their uncompressed bytes match exactly. No fuzzy or cross-chunk dictionary dependency was introduced.\n",
            improvement.evidence
        )?;
        writeln!(output, "| Metric | Before | After |")?;
        writeln!(output, "|---|---:|---:|")?;
        writeln!(
            output,
            "| Experiment | `{}` | `{}` |",
            improvement.before_experiment_id, improvement.after_experiment_id
        )?;
        writeln!(
            output,
            "| Archive bytes | {} | {} |",
            improvement.archive_bytes_before, improvement.archive_bytes_after
        )?;
        writeln!(output, "| Bytes saved | 0 | {} |", improvement.bytes_saved)?;
        writeln!(
            output,
            "| Physical chunks removed | 0 | {} |",
            improvement.duplicate_entries_removed
        )?;
        writeln!(
            output,
            "| p95 bytes fetched | {:.0} | {:.0} |",
            improvement.p95_bytes_before, improvement.p95_bytes_after
        )?;
        writeln!(
            output,
            "| p95 physical reads | {:.0} | {:.0} |",
            improvement.p95_reads_before, improvement.p95_reads_after
        )?;
        writeln!(
            output,
            "| p95 20 ms network | {:.2} ms | {:.2} ms |",
            improvement.p95_20ms_before, improvement.p95_20ms_after
        )?;
        writeln!(
            output,
            "| Correctness | pass | {} |\n",
            if improvement.correctness_after {
                "pass"
            } else {
                "FAIL"
            }
        )?;
    }

    writeln!(output, "\n## Pareto frontier\n")?;
    writeln!(
        output,
        "The frontier jointly considers archive bytes, p95 reads, bytes, local time, and simulated 20 ms latency. In smoke mode it contains only the single measured candidate and is not a comparative winner.\n"
    )?;
    writeln!(
        output,
        "| Experiment | Archive | p95 reads | p95 bytes | p95 local | p95 20 ms |"
    )?;
    writeln!(output, "|---|---:|---:|---:|---:|---:|")?;
    for point in pareto {
        writeln!(
            output,
            "| {} | {} B | {:.0} | {:.0} | {:.1} us | {:.2} ms |",
            point.experiment_id,
            point.archive_bytes,
            point.p95_physical_reads,
            point.p95_bytes_fetched,
            point.p95_local_query_us,
            point.p95_simulated_20ms_ms
        )?;
    }

    writeln!(output, "\n## What we learned\n")?;
    writeln!(
        output,
        "- Fixed windows can answer every exercised locus from a small bootstrap plus one parallel data round while preserving exact local graph semantics."
    )?;
    writeln!(
        output,
        "- Query-size and named hard-locus groups remain separate in the retained distributions rather than being collapsed into one average."
    )?;
    writeln!(
        output,
        "- The latency-first Pareto point is `{}`: {} bytes ({:.3}x GBZ), with p95 {:.0} reads, {:.0} bytes, and {:.2} ms under the 20 ms profile.",
        latency_first.experiment_id,
        latency_first.archive_bytes,
        latency_first.expansion_ratio,
        latency_first.p95_physical_reads,
        latency_first.p95_bytes_fetched,
        latency_first.p95_simulated_20ms_ms
    )?;
    writeln!(
        output,
        "- GBZ-base remains storage-competitive relative to the materialized candidates at {:.3}x GBZ, but its measured local p95 was {:.1} us and its synchronous SQLite access pattern is poorly matched to static-object range access.",
        baselines.gbz_base_to_gbz_ratio, baselines.gbz_base_query_us.p95
    )?;

    writeln!(output, "\n## What failed or remains unresolved\n")?;
    if skipped_sizes.is_empty() {
        writeln!(
            output,
            "- All requested interval sizes were exercised. The 10,000-query requirement remains deferred to a longer benchmark run."
        )?;
    } else {
        writeln!(
            output,
            "- Query sizes {skipped_sizes:?} were skipped because no reference fragment in this fixture is long enough; the 10,000-query requirement is likewise deferred until chromosome scale."
        )?;
    }
    writeln!(
        output,
        "- Archive expansion is input-specific: fixed headers, the root index, path metadata, and boundary duplication scale differently across fixtures."
    )?;
    if improvement.is_some() {
        writeln!(
            output,
            "- The best pre-improvement materialized archive had a measured expansion of {:.3}x GBZ.",
            runs.iter()
                .filter(|run| !run.improvement)
                .map(|run| run.build.expansion_ratio)
                .min_by(f64::total_cmp)
                .unwrap_or(0.0)
        )?;
    } else {
        writeln!(
            output,
            "- Single-config smoke mode cannot establish a new Pareto winner or attribute deduplication savings without a paired non-deduplicated build."
        )?;
    }
    writeln!(
        output,
        "- Peak RSS is only available as whole-process `VmHWM` ({peak_rss_kib:?} KiB); per-phase construction/query RSS and CPU time are not inferred."
    )?;
    writeln!(
        output,
        "- This materialized representation does not preserve compressed GBWT records; a GBZ-record-preserving branch remains untested."
    )?;

    writeln!(output, "\n## What surprised us\n")?;
    if let Some(improvement) = improvement {
        writeln!(
            output,
            "Exact regional payloads recurred across distinct reference directory entries often enough to save {} bytes without changing query semantics or introducing a decode dependency. Coalescing beyond adjacency had limited value because path-local coordinate ordering already made each query's required payloads contiguous.\n",
            improvement.bytes_saved
        )?;
    } else {
        writeln!(
            output,
            "Smoke mode intentionally did not search for a new structural improvement; it isolates correctness and scale behavior of the previously selected layout.\n"
        )?;
    }

    writeln!(output, "## Next highest-information experiment\n")?;
    writeln!(
        output,
        "{}",
        match options.mode {
            ExperimentMode::FullSweep =>
                "Run the same retained matrix on one HPRC chromosome, adding a GBZ-record-preserving representation beside this locally materialized encoding. That scale will reveal whether path metadata/halo duplication or decompressed regional materialization is the dominant expansion source, and it enables the required 100 kb, 1 Mb, and 10,000-query workloads.",
            ExperimentMode::SingleConfigSmoke =>
                "If this smoke run passes, use its size and construction evidence to choose a deliberately scoped comparative sweep before moving to a multi-gigabyte chromosome or whole-genome input.",
        }
    )?;
    output.flush()?;
    Ok(())
}

fn write_queries_csv(path: &Path, rows: &[QueryRow]) -> ExperimentResult<()> {
    let mut output = BufWriter::new(File::create(path)?);
    let headers = [
        "storage_kind",
        "experiment_id",
        "query_id",
        "query_class",
        "sample",
        "contig",
        "start",
        "end",
        "query_size",
        "context",
        "coalescing_gap",
        "physical_reads",
        "mergeable_reads",
        "dependency_rounds",
        "total_bytes_fetched",
        "unique_bytes_fetched",
        "duplicate_bytes_fetched",
        "bootstrap_bytes_fetched",
        "logical_index_bytes",
        "data_bytes_fetched",
        "required_compressed_payload_bytes",
        "canonical_payload_bytes",
        "read_amplification",
        "canonical_amplification",
        "index_lookup_us",
        "decompression_us",
        "decode_us",
        "graph_reconstruction_us",
        "total_local_query_us",
        "selected_chunks",
        "selected_nodes",
        "canonical_hash",
        "correctness",
        "simulated_20ms_ms",
        "simulated_50ms_ms",
        "simulated_100ms_ms",
    ];
    writeln!(output, "{}", headers.join(","))?;
    for row in rows {
        let values = [
            row.storage_kind.clone(),
            row.experiment_id.clone(),
            row.query_id.clone(),
            row.query_class.clone(),
            row.sample.clone(),
            row.contig.clone(),
            row.start.to_string(),
            row.end.to_string(),
            row.query_size.to_string(),
            row.context.to_string(),
            option_string(row.coalescing_gap),
            option_string(row.physical_reads),
            option_string(row.mergeable_reads),
            option_string(row.dependency_rounds),
            option_string(row.total_bytes_fetched),
            option_string(row.unique_bytes_fetched),
            option_string(row.duplicate_bytes_fetched),
            option_string(row.bootstrap_bytes_fetched),
            option_string(row.logical_index_bytes),
            option_string(row.data_bytes_fetched),
            option_string(row.required_compressed_payload_bytes),
            option_string(row.canonical_payload_bytes),
            option_float(row.read_amplification),
            option_float(row.canonical_amplification),
            option_float(row.index_lookup_us),
            option_float(row.decompression_us),
            option_float(row.decode_us),
            option_float(row.graph_reconstruction_us),
            format!("{:.6}", row.total_local_query_us),
            option_string(row.selected_chunks),
            option_string(row.selected_nodes),
            row.canonical_hash.clone().unwrap_or_default(),
            row.correctness.to_string(),
            option_float(row.simulated_20ms_ms),
            option_float(row.simulated_50ms_ms),
            option_float(row.simulated_100ms_ms),
        ];
        writeln!(
            output,
            "{}",
            values
                .iter()
                .map(|value| csv_escape(value))
                .collect::<Vec<_>>()
                .join(",")
        )?;
    }
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
    let mut input = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let length = input.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        hasher.update(&buffer[..length]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.into()
    }
}

fn option_string<T: ToString>(value: Option<T>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn option_float(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.6}")).unwrap_or_default()
}

#[allow(clippy::cast_precision_loss)]
fn distribution(mut values: Vec<f64>) -> Distribution {
    values.sort_by(f64::total_cmp);
    Distribution {
        count: values.len(),
        mean: if values.is_empty() {
            0.0
        } else {
            values.iter().sum::<f64>() / values.len() as f64
        },
        p50: percentile(&values, 0.50),
        p90: percentile(&values, 0.90),
        p95: percentile(&values, 0.95),
        p99: percentile(&values, 0.99),
        max: values.last().copied().unwrap_or(0.0),
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let scaled = (values.len().saturating_sub(1)) as f64 * percentile;
    let index = scaled.ceil() as usize;
    values[index.min(values.len() - 1)]
}

fn process_peak_rss_kib() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

fn directory_size(path: &Path) -> io::Result<u64> {
    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            total = total.saturating_add(directory_size(&entry.path())?);
        } else {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

fn size_label(size: u64) -> &'static str {
    match size {
        16_384 => "16k",
        65_536 => "64k",
        262_144 => "256k",
        1_048_576 => "1m",
        4_194_304 => "4m",
        _ => "custom",
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

fn elapsed_us(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000_000.0
}

#[allow(clippy::cast_precision_loss)]
fn ratio(numerator: u64, denominator: u64) -> f64 {
    numerator as f64 / denominator as f64
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "NO" }
}

fn relative_latency(baseline_us: f64, candidate_us: f64) -> String {
    if baseline_us <= 0.0 || candidate_us <= 0.0 {
        return "undefined".into();
    }
    let baseline_over_candidate = baseline_us / candidate_us;
    if baseline_over_candidate >= 1.0 {
        format!("{baseline_over_candidate:.2}x faster")
    } else {
        format!("{:.2}x slower", candidate_us / baseline_us)
    }
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

struct XorShift64(u64);

impl XorShift64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn bounded(&mut self, upper_exclusive: u64) -> u64 {
        if upper_exclusive == 0 {
            0
        } else {
            self.next() % upper_exclusive
        }
    }

    fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.bounded(u64::try_from(len).unwrap_or(u64::MAX))).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_mode_runs_only_the_selected_deduplicated_layout() {
        let configs = planned_configs(ExperimentMode::SingleConfigSmoke);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].experiment_id, "fixed-w256k-zstd-6-dedup");
        assert_eq!(configs[0].window_size, 262_144);
        assert_eq!(configs[0].codec, ChunkCodec::Zstd6);
        assert!(configs[0].deduplicate_chunks);
    }

    #[test]
    fn full_sweep_retains_the_twenty_layout_matrix() {
        let configs = planned_configs(ExperimentMode::FullSweep);
        assert_eq!(configs.len(), WINDOW_SIZES.len() * CODECS.len());
        assert!(configs.iter().all(|config| !config.deduplicate_chunks));
        assert_eq!(
            configs
                .iter()
                .map(|config| config.experiment_id.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            configs.len()
        );
    }

    #[test]
    fn medium_reference_enables_every_requested_query_size() {
        let references = vec![super::super::fixed::ReferencePathSpec {
            name: FullPathName {
                sample: "MHC-GRCh38".into(),
                contig: "MHC".into(),
                haplotype: 0,
                fragment: 0,
            },
            start: 0,
            end: 5_000_000,
        }];
        let (workload, skipped) = deterministic_workload(&references, 2);
        assert!(skipped.is_empty());
        assert_eq!(workload.len(), 8);
        assert_eq!(
            workload
                .iter()
                .map(QuerySpec::length)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([1_000, 10_000, 100_000, 1_000_000])
        );
    }

    #[test]
    fn relative_latency_reports_direction() {
        assert_eq!(relative_latency(200.0, 100.0), "2.00x faster");
        assert_eq!(relative_latency(100.0, 400.0), "4.00x slower");
    }
}
