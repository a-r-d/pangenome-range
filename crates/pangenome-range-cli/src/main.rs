use gbz::GBZ;
use gbz_base::PathIndex;
use pangenome_range_build::{
    BuildProgressMode, ChunkCodec, EncodeOptions, EncodeSourceMode, EncoderScaleOptions,
    ExperimentMode, ExperimentOptions, FixedArchiveConfig, FixedArchiveReader, QueryMeasurement,
    QuerySpec, build_persistent_source_cache, export_conformance_fixtures,
    inspect_persistent_source_cache, internal_gbz_base_query, prune_persistent_source_cache,
    run_encode, run_encoder_scale_experiment, run_fixed_window_experiment, source_oracle,
    source_oracle_for_haplotype, validate_fixed_archive_with_options,
};
use pangenome_range_format::{
    FileRangeSource, NetworkProfile, RangeSource, TracingRangeSource, ValidationMode,
    ValidationOptions, evaluate_integrity_options,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use simple_sds::serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::{BufReader, IsTerminal, Read};
use std::path::{Path, PathBuf};

type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

fn main() {
    if let Err(error) = run(std::env::args().skip(1)) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(mut args: impl Iterator<Item = String>) -> AppResult<()> {
    let Some(command) = args.next() else {
        print_help();
        return Ok(());
    };

    match command.as_str() {
        "encode" => encode(&mut args),
        "source-cache" => source_cache(&mut args),
        "verify" => verify(&mut args),
        "validate" => validate_archive(&mut args),
        "evaluate-integrity" => evaluate_integrity(&mut args),
        "fixtures" => fixtures(&mut args),
        "inspect" => {
            let path = one_path_argument(&mut args, "inspect")?;
            inspect_gbz(&path)
        }
        "benchmark-source" => {
            let path = one_path_argument(&mut args, "benchmark-source")?;
            benchmark_source(&path)
        }
        "benchmark-fixed-windows" => benchmark_fixed_windows(&mut args, ExperimentMode::FullSweep),
        "benchmark-fixed-window-smoke" => {
            benchmark_fixed_windows(&mut args, ExperimentMode::SingleConfigSmoke)
        }
        "benchmark-encoder-scale" => benchmark_encoder_scale(&mut args),
        "internal-gbz-base-query" => run_internal_gbz_base_query(&mut args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "version" | "--version" | "-V" => {
            print_version();
            Ok(())
        }
        "build" | "query" | "benchmark" => Err(format!(
            "'{command}' is reserved for a future experiment; use 'inspect' or 'benchmark-source'"
        )
        .into()),
        _ => Err(format!("unknown command '{command}' (run 'pangenome-range help')").into()),
    }
}

fn one_path_argument(args: &mut impl Iterator<Item = String>, command: &str) -> AppResult<String> {
    let path = args
        .next()
        .ok_or_else(|| format!("usage: pangenome-range {command} <file>"))?;
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument '{extra}'").into());
    }
    Ok(path)
}

fn inspect_gbz(path: impl AsRef<Path>) -> AppResult<()> {
    let path = path.as_ref();
    if !GBZ::is_gbz(path) {
        return Err(format!("{} is not recognized as a GBZ file", path.display()).into());
    }

    let file_size = std::fs::metadata(path)?.len();
    let graph: GBZ = serialize::load_from(path)?;

    println!("file: {}", path.display());
    println!("file size: {file_size} bytes");
    println!("nodes: {}", graph.nodes());
    println!("node id range: {}..={}", graph.min_node(), graph.max_node());
    println!("paths: {}", graph.paths());
    println!("node-to-segment translation: {}", graph.has_translation());

    if let Some(metadata) = graph.metadata() {
        println!("samples: {}", metadata.samples());
        println!("haplotypes: {}", metadata.haplotypes());
        println!("contigs: {}", metadata.contigs());
        let contigs: Vec<_> = (0..metadata.contigs())
            .map(|id| metadata.contig_name(id))
            .collect();
        print_named_list("contig names", &contigs, 20);

        let reference_ids: BTreeSet<_> = graph.reference_sample_ids(true).into_iter().collect();
        let reference_samples = graph.reference_sample_names(true);
        println!("reference samples: {}", join_or_none(&reference_samples));

        let mut reference_paths = Vec::new();
        for path_name in metadata.path_iter() {
            if reference_ids.contains(&path_name.sample()) {
                reference_paths.push(format!(
                    "{}#{}#{}@{}",
                    metadata.sample_name(path_name.sample()),
                    path_name.phase(),
                    metadata.contig_name(path_name.contig()),
                    path_name.fragment()
                ));
            }
        }
        println!("reference paths: {}", reference_paths.len());
        for name in reference_paths.iter().take(20) {
            println!("  {name}");
        }
        if reference_paths.len() > 20 {
            println!("  ... {} more", reference_paths.len() - 20);
        }
    } else {
        println!("metadata: absent");
    }

    if graph.tags().is_empty() {
        println!("tags: none");
    } else {
        println!("tags:");
        for (key, value) in graph.tags().iter() {
            println!("  {key}={value}");
        }
    }
    Ok(())
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

fn print_named_list(label: &str, values: &[String], limit: usize) {
    if values.is_empty() {
        println!("{label}: none");
        return;
    }
    println!("{label}:");
    for value in values.iter().take(limit) {
        println!("  {value}");
    }
    if values.len() > limit {
        println!("  ... {} more", values.len() - limit);
    }
}

fn benchmark_source(path: impl AsRef<Path>) -> AppResult<()> {
    let path = path.as_ref();
    let source = TracingRangeSource::new(FileRangeSource::open(path)?);
    let len = source.len()?;
    if len == 0 {
        println!("file: {}", path.display());
        println!("source length: 0 bytes");
        println!("no reads issued for an empty source");
        return Ok(());
    }

    let probe_length = len.min(4_096);
    read_probe(&source, 0, probe_length)?;
    read_probe(&source, (len - probe_length) / 2, probe_length)?;
    read_probe(&source, len - probe_length, probe_length)?;
    read_probe(&source, 0, len.min(256))?;

    let trace = source.summary();
    println!("file: {}", path.display());
    println!("source length: {len} bytes");
    println!("read operations: {}", trace.read_operations);
    println!("successful operations: {}", trace.successful_operations);
    println!("total bytes requested: {}", trace.total_bytes_requested);
    println!("unique bytes requested: {}", trace.unique_bytes_requested);
    println!(
        "duplicate bytes requested: {}",
        trace.duplicate_bytes_requested
    );
    println!("mergeable reads: {}", trace.mergeable_reads);
    println!("coalesced ranges: {}", trace.coalesced_ranges);
    println!("smallest read: {} bytes", trace.smallest_read.unwrap_or(0));
    println!("largest read: {} bytes", trace.largest_read.unwrap_or(0));
    println!("ranges:");
    for read in &trace.reads {
        println!(
            "  #{} offset={} length={} success={}",
            read.sequence, read.offset, read.length, read.succeeded
        );
    }
    println!("simulated costs (idealized, not a browser benchmark):");
    for profile in NetworkProfile::STANDARD {
        let cost = profile.estimate(&trace);
        println!(
            "  {}: {:.3} ms ({} request waves, {:.3} ms transfer)",
            profile.name, cost.estimated_total_ms, cost.request_rounds, cost.transfer_ms
        );
    }
    Ok(())
}

fn read_probe(source: &impl RangeSource, offset: u64, length: u64) -> AppResult<()> {
    let length = usize::try_from(length)?;
    let _bytes = source.read_range(offset, length)?;
    Ok(())
}

fn source_cache(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let usage = "usage: pangenome-range source-cache build <input.gbz> <cache-dir> [--rebuild]\n       pangenome-range source-cache inspect <cache-dir>\n       pangenome-range source-cache prune <cache-dir>";
    let command = args.next().ok_or(usage)?;
    match command.as_str() {
        "build" => {
            let input = PathBuf::from(args.next().ok_or(usage)?);
            let cache = PathBuf::from(args.next().ok_or(usage)?);
            let mut rebuild = false;
            for flag in args {
                match flag.as_str() {
                    "--rebuild" => rebuild = true,
                    "--help" | "-h" => {
                        println!("{usage}");
                        return Ok(());
                    }
                    _ => return Err(format!("unknown source-cache build option '{flag}'").into()),
                }
            }
            let persistent = build_persistent_source_cache(&input, &cache, rebuild)?;
            println!("{}", serde_json::to_string_pretty(&persistent.manifest)?);
            Ok(())
        }
        "inspect" => {
            let path = PathBuf::from(args.next().ok_or(usage)?);
            if let Some(extra) = args.next() {
                return Err(format!("unexpected source-cache inspect argument '{extra}'").into());
            }
            let manifest = inspect_persistent_source_cache(&path)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            Ok(())
        }
        "prune" => {
            let path = PathBuf::from(args.next().ok_or(usage)?);
            if let Some(extra) = args.next() {
                return Err(format!("unexpected source-cache prune argument '{extra}'").into());
            }
            let manifest = prune_persistent_source_cache(&path)?;
            println!(
                "pruned cache for {} bytes / {}",
                manifest.source_gbz_bytes, manifest.source_gbz_sha256
            );
            Ok(())
        }
        "--help" | "-h" => {
            println!("{usage}");
            Ok(())
        }
        _ => Err(format!("unknown source-cache command '{command}'\n{usage}").into()),
    }
}

fn print_help() {
    println!("pangenome-range research CLI");
    println!();
    println!("Usage:");
    println!("  pangenome-range encode <input.gbz> <output.pngr> [options]");
    println!("  pangenome-range source-cache build <input.gbz> <cache-dir> [--rebuild]");
    println!("  pangenome-range source-cache inspect <cache-dir>");
    println!("  pangenome-range source-cache prune <cache-dir>");
    println!("  pangenome-range validate <input.pngr>");
    println!("  pangenome-range evaluate-integrity <input.pngr> [--report PATH]");
    println!("  pangenome-range fixtures export <directory>");
    println!(
        "  pangenome-range verify <input.pngr> --against <input.gbz> --sample NAME --contig NAME --start BP --end BP [options]"
    );
    println!("  pangenome-range inspect <graph.gbz>");
    println!("  pangenome-range benchmark-source <file>");
    println!(
        "  pangenome-range benchmark-fixed-windows <graph.gbz> <run-id> [random-queries-per-size]"
    );
    println!(
        "  pangenome-range benchmark-fixed-window-smoke <graph.gbz> <run-id> [random-queries-per-size]"
    );
    println!("  pangenome-range benchmark-encoder-scale <graph.gbz> <run-id> <external-work-root>");
    println!("  pangenome-range --version");
    println!();
    println!();
    println!("Encode options:");
    println!("  --sample NAME              select a reference sample");
    println!("  --reference-haplotype N    explicitly anchor to this real haplotype of --sample");
    println!("  --contig NAME              select a reference contig");
    println!("  --start BP --end BP        restrict the selected contig interval");
    println!("  --window-size BP           base window size (default: 16384)");
    println!("  --codec NAME               none|zstd-1|zstd-3|zstd-6");
    println!("  --haplotypes MODE          anonymous-distinct-weighted-tile-paths");
    println!("  --max-uncompressed-chunk-bytes N");
    println!("  --min-window-size BP");
    println!(
        "  --threads N                bounded tile/compression workers (default: up to 8 available cores)"
    );
    println!("  --max-queued-bytes N       raw+compressed queue cap");
    println!("  --source-access MODE       disk|loaded (default: disk)");
    println!("  --scratch-dir PATH         source-cache and research scratch location");
    println!("  --source-cache PATH        reuse an authenticated persistent source cache");
    println!("  --annotations PATH         populate the default named-locus index from GFF3");
    println!("  --annotation-sample NAME   bind GFF3 coordinates to this reference sample");
    println!("  --annotation-feature-type TYPE  repeatable exact GFF3 type (default: gene)");
    println!("  --reference-assembly ID    user-supplied reference assembly identifier");
    println!("  --dataset-title TEXT       deterministic archive title metadata");
    println!("  --dataset-description TEXT deterministic archive description metadata");
    println!("  --source-uri URI           canonical source URI (never a local path)");
    println!("  --annotation-release ID    user-supplied annotation release identifier");
    println!("  --annotation-assembly ID   user-supplied annotation assembly identifier");
    println!("  --path-membership          preserve named source-path membership");
    println!("  --path-locate-max-lf-steps N  bounded LF guard (default: 1000000)");
    println!("  --keep-partial             retain the sibling temp archive on failure");
    println!("  --progress auto|plain|json|off");
    println!("  --progress-interval-seconds N  chunk progress cadence (default: 5)");
    println!("  --max-chunks N             bounded single-reference research guard");
    println!("  --report PATH              JSON build report path");
    println!();
    println!("Verify options:");
    println!("  --against PATH             independent source GBZ oracle");
    println!("  --sample NAME --contig NAME --start BP --end BP");
    println!("  --workload PATH            versioned benchmark workload or legacy query array");
    println!("  --report PATH              write the JSON verification result");
    println!("  --context BP               source-oracle context (default: 100)");
    println!("  --coalescing-gap BYTES     archive read coalescing gap");
    println!();
    println!("Validate options:");
    println!("  --mode standard|full       default integrity gate or exhaustive reconstruction");
    println!("  --workers N                bounded payload-validation workers (default: up to 8)");
    println!("  --max-queued-bytes N       validation memory budget (default: 512 MiB)");
    println!("  --progress auto|plain|json|off");
    println!("  --progress-interval-seconds N  validation progress cadence (default: 5)");
    println!();
    println!("Reserved experiment commands: build, query, benchmark");
}

fn evaluate_integrity(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let usage = "usage: pangenome-range evaluate-integrity <input.pngr> [--report PATH]";
    let archive = PathBuf::from(args.next().ok_or(usage)?);
    let mut report_path = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--report" => report_path = Some(PathBuf::from(option_value(args, &flag)?)),
            _ => return Err(format!("unknown evaluate-integrity option '{flag}'").into()),
        }
    }
    let report = evaluate_integrity_options(&archive)?;
    let encoded = serde_json::to_vec_pretty(&report)?;
    if let Some(path) = report_path {
        std::fs::write(path, &encoded)?;
    }
    println!("{}", String::from_utf8(encoded)?);
    Ok(())
}

fn print_version() {
    println!("pangenome-range {}", env!("CARGO_PKG_VERSION"));
}

fn fixtures(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let action = args
        .next()
        .ok_or("usage: pangenome-range fixtures export <directory>")?;
    if action != "export" {
        return Err(format!("unknown fixtures action '{action}' (expected 'export')").into());
    }
    let directory = PathBuf::from(
        args.next()
            .ok_or("usage: pangenome-range fixtures export <directory>")?,
    );
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument '{extra}'").into());
    }
    export_conformance_fixtures(&directory)?;
    println!(
        "exported deterministic conformance fixtures to {}",
        directory.display()
    );
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "keeping the flat verifier option table in one place makes its oracle contract auditable"
)]
fn verify(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let usage = "usage: pangenome-range verify <input.pngr> --against <input.gbz> (--workload queries.json | --sample NAME --contig NAME --start BP --end BP) [--reference-haplotype N] [options]";
    let archive = PathBuf::from(args.next().ok_or(usage)?);
    let mut against = None;
    let mut sample = None;
    let mut contig = None;
    let mut start = None;
    let mut end = None;
    let mut workload = None;
    let mut report = None;
    let mut reference_haplotype = None;
    let mut context = 100_u64;
    let mut coalescing_gap = 65_536_u64;
    let mut window_size = 16_384_u64;
    let mut codec = ChunkCodec::Zstd3;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--against" => against = Some(PathBuf::from(option_value(args, &flag)?)),
            "--sample" => sample = Some(option_value(args, &flag)?),
            "--contig" => contig = Some(option_value(args, &flag)?),
            "--start" => start = Some(parse_option(args, &flag)?),
            "--end" => end = Some(parse_option(args, &flag)?),
            "--workload" => workload = Some(PathBuf::from(option_value(args, &flag)?)),
            "--report" => report = Some(PathBuf::from(option_value(args, &flag)?)),
            "--reference-haplotype" => {
                reference_haplotype = Some(parse_option(args, &flag)?);
            }
            "--context" => context = parse_option(args, &flag)?,
            "--coalescing-gap" => coalescing_gap = parse_option(args, &flag)?,
            "--window-size" => window_size = parse_option(args, &flag)?,
            "--codec" => {
                codec = match option_value(args, &flag)?.as_str() {
                    "none" => ChunkCodec::None,
                    "zstd-1" => ChunkCodec::Zstd1,
                    "zstd-3" => ChunkCodec::Zstd3,
                    "zstd-6" => ChunkCodec::Zstd6,
                    value => return Err(format!("unsupported codec '{value}'").into()),
                };
            }
            "--help" | "-h" => {
                println!("{usage}");
                return Ok(());
            }
            _ => return Err(format!("unknown verify option '{flag}'").into()),
        }
    }
    let against = against.ok_or("verify requires --against <input.gbz>")?;
    let mut expected_hashes = BTreeMap::new();
    let mut skipped_negative_queries = Vec::new();
    let queries = if let Some(workload) = &workload {
        if sample.is_some() || contig.is_some() || start.is_some() || end.is_some() {
            return Err("--workload cannot be combined with a single-query coordinate".into());
        }
        let loaded = load_verification_workload(workload, &archive)?;
        expected_hashes = loaded.expected_hashes;
        skipped_negative_queries = loaded.skipped_negative_queries;
        let queries = loaded.queries;
        if queries.is_empty() {
            return Err("verification workload contains no queries".into());
        }
        queries
    } else {
        vec![QuerySpec {
            id: "cli-verify".into(),
            class: "post-build-verification".into(),
            sample: sample.ok_or("verify requires --sample or --workload")?,
            contig: contig.ok_or("verify requires --contig or --workload")?,
            start: start.ok_or("verify requires --start or --workload")?,
            end: end.ok_or("verify requires --end or --workload")?,
            context,
        }]
    };
    let mut graph: GBZ = serialize::load_from(&against)?;
    select_explicit_reference_samples(&mut graph, &queries, reference_haplotype)?;
    let path_index = PathIndex::new(&graph, 1_000, false)?;
    let oracle = VerificationOracle {
        graph: &graph,
        path_index: &path_index,
        reference_haplotype,
    };
    let archive_config = verification_archive_config(window_size, codec);
    let mut reader = FixedArchiveReader::open(&archive)?;
    let measurements = verify_queries(
        &oracle,
        &mut reader,
        &archive_config,
        &queries,
        coalescing_gap,
        &expected_hashes,
    )?;
    let output = verification_output(
        workload.is_some(),
        &archive,
        &against,
        &measurements,
        &skipped_negative_queries,
    )?;
    if let Some(report) = report {
        std::fs::write(report, output.as_bytes())?;
    }
    println!("{output}");
    Ok(())
}

fn verification_archive_config(window_size: u64, codec: ChunkCodec) -> FixedArchiveConfig {
    FixedArchiveConfig {
        experiment_id: "cli-verify".into(),
        window_size,
        codec,
        deduplicate_chunks: false,
        max_uncompressed_chunk_bytes: 8 * 1024 * 1024,
        min_window_size: 1_024,
    }
}

fn select_explicit_reference_samples(
    graph: &mut GBZ,
    queries: &[QuerySpec],
    reference_haplotype: Option<usize>,
) -> AppResult<()> {
    if reference_haplotype.is_none() {
        return Ok(());
    }
    let reference_samples = queries
        .iter()
        .map(|query| query.sample.clone())
        .collect::<BTreeSet<_>>();
    let requested = reference_samples.len();
    let reference_samples = reference_samples.into_iter().collect::<Vec<_>>();
    let selected = graph.set_reference_samples(&reference_samples);
    if selected != requested {
        return Err(format!(
            "could not select every explicit reference sample from the GBZ (requested {requested}, selected {selected})"
        )
        .into());
    }
    Ok(())
}

fn verification_output(
    is_workload: bool,
    archive: &Path,
    against: &Path,
    measurements: &[QueryMeasurement],
    skipped_negative_queries: &[String],
) -> AppResult<String> {
    if is_workload {
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "archive": archive,
            "against": against,
            "measurements": measurements,
            "skipped_negative_queries": skipped_negative_queries,
        }))?)
    } else {
        Ok(serde_json::to_string_pretty(&measurements[0])?)
    }
}

struct VerificationOracle<'a> {
    graph: &'a GBZ,
    path_index: &'a PathIndex,
    reference_haplotype: Option<usize>,
}

impl VerificationOracle<'_> {
    fn extract(&self, query: &QuerySpec) -> AppResult<pangenome_range_build::OracleResult> {
        Ok(if let Some(haplotype) = self.reference_haplotype {
            source_oracle_for_haplotype(self.graph, self.path_index, query, haplotype)?
        } else {
            source_oracle(self.graph, self.path_index, query)?
        })
    }
}

fn verify_queries(
    oracle: &VerificationOracle<'_>,
    reader: &mut FixedArchiveReader,
    archive_config: &FixedArchiveConfig,
    queries: &[QuerySpec],
    coalescing_gap: u64,
    expected_hashes: &BTreeMap<String, String>,
) -> AppResult<Vec<QueryMeasurement>> {
    let mut measurements = Vec::with_capacity(queries.len());
    for query in queries {
        let expected = oracle.extract(query)?;
        let measurement = reader.query(
            archive_config,
            query,
            coalescing_gap,
            &expected,
            Some((oracle.graph, oracle.path_index)),
        )?;
        if let Some(expected) = expected_hashes.get(&query.id)
            && measurement.canonical_hash != *expected
        {
            return Err(format!(
                "query {} canonical hash {} does not match workload {}",
                query.id, measurement.canonical_hash, expected
            )
            .into());
        }
        measurements.push(measurement);
    }
    Ok(measurements)
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum VerificationWorkloadFile {
    Legacy(Vec<QuerySpec>),
    Versioned(VersionedVerificationWorkload),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionedVerificationWorkload {
    schema_version: u32,
    archive_sha256: String,
    queries: Vec<VersionedVerificationQuery>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionedVerificationQuery {
    id: String,
    class: String,
    sample: String,
    contig: String,
    start: u64,
    end: u64,
    context: u64,
    expected_canonical_hash: Option<String>,
    expected_error: Option<String>,
}

struct LoadedVerificationWorkload {
    queries: Vec<QuerySpec>,
    expected_hashes: BTreeMap<String, String>,
    skipped_negative_queries: Vec<String>,
}

fn load_verification_workload(
    workload_path: &Path,
    archive_path: &Path,
) -> AppResult<LoadedVerificationWorkload> {
    let parsed: VerificationWorkloadFile = serde_json::from_slice(&std::fs::read(workload_path)?)?;
    match parsed {
        VerificationWorkloadFile::Legacy(queries) => Ok(LoadedVerificationWorkload {
            queries,
            expected_hashes: BTreeMap::new(),
            skipped_negative_queries: Vec::new(),
        }),
        VerificationWorkloadFile::Versioned(workload) => {
            if workload.schema_version != 1 {
                return Err(format!(
                    "unsupported verification workload schemaVersion {}",
                    workload.schema_version
                )
                .into());
            }
            if workload.archive_sha256.len() != 64
                || !workload
                    .archive_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err("workload archiveSha256 must be 64 lowercase hex characters".into());
            }
            let actual_sha256 = sha256_path(archive_path)?;
            if actual_sha256 != workload.archive_sha256 {
                return Err(format!(
                    "archive SHA-256 {actual_sha256} does not match workload {}",
                    workload.archive_sha256
                )
                .into());
            }
            let mut queries = Vec::new();
            let mut expected_hashes = BTreeMap::new();
            let mut skipped_negative_queries = Vec::new();
            for query in workload.queries {
                match (&query.expected_canonical_hash, &query.expected_error) {
                    (Some(expected), None) => {
                        if expected.len() != 64
                            || !expected
                                .bytes()
                                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                        {
                            return Err(format!(
                                "query {} expectedCanonicalHash must be 64 lowercase hex characters",
                                query.id
                            )
                            .into());
                        }
                        expected_hashes.insert(query.id.clone(), expected.clone());
                        queries.push(QuerySpec {
                            id: query.id,
                            class: query.class,
                            sample: query.sample,
                            contig: query.contig,
                            start: query.start,
                            end: query.end,
                            context: query.context,
                        });
                    }
                    (None, Some(expected_error)) => skipped_negative_queries.push(format!(
                        "{} ({expected_error}; TypeScript reader-specific negative case)",
                        query.id
                    )),
                    _ => {
                        return Err(format!(
                            "query {} must declare exactly one of expectedCanonicalHash or expectedError",
                            query.id
                        )
                        .into());
                    }
                }
            }
            Ok(LoadedVerificationWorkload {
                queries,
                expected_hashes,
                skipped_negative_queries,
            })
        }
    }
}

fn sha256_path(path: &Path) -> AppResult<String> {
    let mut reader = BufReader::new(std::fs::File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_archive(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let usage = "usage: pangenome-range validate <input.pngr> [--mode standard|full] [--workers N] [--max-queued-bytes N] [--progress auto|plain|json|off] [--progress-interval-seconds N]";
    let first = args.next().ok_or(usage)?;
    if matches!(first.as_str(), "--help" | "-h") {
        println!("{usage}");
        return Ok(());
    }
    let path = PathBuf::from(first);
    let mut progress = automatic_progress_mode();
    let mut progress_interval_ms = 5_000_u64;
    let mut mode = ValidationMode::Standard;
    let mut workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(8);
    let mut max_queued_bytes = 512 * 1024 * 1024_u64;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--progress" => {
                progress = parse_progress_mode(&option_value(args, &flag)?)?;
            }
            "--mode" => {
                mode = match option_value(args, &flag)?.as_str() {
                    "standard" => ValidationMode::Standard,
                    "full" => ValidationMode::Full,
                    value => return Err(format!("unknown validation mode '{value}'").into()),
                };
            }
            "--workers" => workers = parse_option(args, &flag)?,
            "--max-queued-bytes" => max_queued_bytes = parse_option(args, &flag)?,
            "--progress-interval-seconds" => {
                let seconds: u64 = parse_option(args, &flag)?;
                progress_interval_ms = seconds
                    .checked_mul(1_000)
                    .ok_or("progress interval is too large")?;
            }
            "--help" | "-h" => {
                println!("{usage}");
                return Ok(());
            }
            _ => return Err(format!("unknown validate option '{flag}'").into()),
        }
    }
    emit_cli_progress(
        progress,
        "archive_validation",
        "validating all directory entries and physical payloads",
    );
    let summary = validate_fixed_archive_with_options(
        &path,
        progress,
        ValidationOptions {
            mode,
            workers,
            max_queued_bytes,
            progress_interval_ms,
        },
    )?;
    emit_cli_progress(
        progress,
        "archive_validation_complete",
        &format!(
            "validated {} directory pages and {} physical payloads in {}",
            format_cli_integer(summary.directory_pages),
            format_cli_integer(summary.physical_payloads),
            format_cli_duration(summary.validation_wall_ms / 1_000.0),
        ),
    );
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "keeping the flat CLI option table in one place makes the public contract auditable"
)]
fn encode(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let usage = "usage: pangenome-range encode <input.gbz> <output.pngr> [options]";
    let first = args.next().ok_or(usage)?;
    if matches!(first.as_str(), "--help" | "-h") {
        print_help();
        return Ok(());
    }
    let input = PathBuf::from(first);
    let output = PathBuf::from(args.next().ok_or(usage)?);
    let mut options = EncodeOptions::new(input, output);
    options.progress = automatic_progress_mode();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--sample" => options.sample = Some(option_value(args, &flag)?),
            "--reference-haplotype" => {
                options.reference_haplotype = Some(parse_option(args, &flag)?);
            }
            "--contig" => options.contig = Some(option_value(args, &flag)?),
            "--start" => options.start = Some(parse_option(args, &flag)?),
            "--end" => options.end = Some(parse_option(args, &flag)?),
            "--window-size" => options.window_size = parse_option(args, &flag)?,
            "--max-uncompressed-chunk-bytes" => {
                options.max_uncompressed_chunk_bytes = parse_option(args, &flag)?;
            }
            "--min-window-size" => options.min_window_size = parse_option(args, &flag)?,
            "--threads" => options.threads = parse_option(args, &flag)?,
            "--max-queued-bytes" => options.max_queued_bytes = parse_option(args, &flag)?,
            "--max-chunks" => options.max_chunks = Some(parse_option(args, &flag)?),
            "--source-access" => {
                let value = option_value(args, &flag)?;
                options.source_mode = match value.as_str() {
                    "disk" => EncodeSourceMode::Disk,
                    "loaded" => EncodeSourceMode::Loaded,
                    _ => return Err(format!("unsupported source access mode '{value}'").into()),
                };
            }
            "--scratch-dir" => {
                options.scratch_dir = Some(PathBuf::from(option_value(args, &flag)?));
            }
            "--source-cache" => {
                options.source_cache = Some(PathBuf::from(option_value(args, &flag)?));
            }
            "--annotations" => {
                options.annotations = Some(PathBuf::from(option_value(args, &flag)?));
            }
            "--annotation-sample" => {
                options.annotation_sample = Some(option_value(args, &flag)?);
            }
            "--annotation-feature-type" => {
                options
                    .annotation_feature_types
                    .push(option_value(args, &flag)?);
            }
            "--reference-assembly" => {
                options.reference_assembly = Some(option_value(args, &flag)?);
            }
            "--dataset-title" => options.dataset_title = Some(option_value(args, &flag)?),
            "--dataset-description" => {
                options.dataset_description = Some(option_value(args, &flag)?);
            }
            "--source-uri" => options.source_uri = Some(option_value(args, &flag)?),
            "--annotation-release" => {
                options.annotation_release = Some(option_value(args, &flag)?);
            }
            "--annotation-assembly" => {
                options.annotation_assembly = Some(option_value(args, &flag)?);
            }
            "--experimental-path-membership-summary" => {
                options.path_membership_summary = Some(PathBuf::from(option_value(args, &flag)?));
            }
            "--experimental-path-catalog" => {
                options.path_membership_catalog = Some(PathBuf::from(option_value(args, &flag)?));
            }
            "--path-membership" | "--experimental-direct-path-membership" => {
                options.path_membership = true;
            }
            "--path-locate-max-lf-steps" | "--experimental-path-locate-max-lf-steps" => {
                options.path_locate_max_lf_steps = parse_option(args, &flag)?;
            }
            "--report" => options.report = Some(PathBuf::from(option_value(args, &flag)?)),
            "--keep-partial" => options.keep_partial = true,
            "--codec" => {
                let value = option_value(args, &flag)?;
                options.codec = match value.as_str() {
                    "none" => ChunkCodec::None,
                    "zstd-1" => ChunkCodec::Zstd1,
                    "zstd-3" => ChunkCodec::Zstd3,
                    "zstd-6" => ChunkCodec::Zstd6,
                    _ => return Err(format!("unsupported codec '{value}'").into()),
                };
            }
            "--haplotypes" => {
                let value = option_value(args, &flag)?;
                if !matches!(
                    value.as_str(),
                    "anonymous-distinct-weighted-tile-paths" | "distinct"
                ) {
                    return Err(format!(
                        "unsupported haplotype mode '{value}'; scalable v1 encode requires anonymous-distinct-weighted-tile-paths"
                    )
                    .into());
                }
            }
            "--progress" => {
                let value = option_value(args, &flag)?;
                options.progress = parse_progress_mode(&value)?;
            }
            "--progress-interval-seconds" => {
                let seconds: u64 = parse_option(args, &flag)?;
                options.progress_interval_ms = seconds
                    .checked_mul(1_000)
                    .ok_or("progress interval is too large")?;
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => return Err(format!("unknown encode option '{flag}'").into()),
        }
    }
    let summary = run_encode(&options)?;
    println!("completed successfully");
    println!("archive: {}", summary.output_path.display());
    println!(
        "archive bytes: {}",
        format_cli_integer(summary.build.archive_bytes)
    );
    println!("archive sha256: {}", summary.output_sha256);
    println!(
        "chunks: {}",
        format_cli_integer(summary.build.directory_entries)
    );
    println!(
        "construction: {} ({:.3} ms)",
        format_cli_duration(summary.build.construction_wall_ms / 1_000.0),
        summary.build.construction_wall_ms,
    );
    Ok(())
}

fn automatic_progress_mode() -> BuildProgressMode {
    if std::io::stderr().is_terminal() {
        BuildProgressMode::Plain
    } else {
        BuildProgressMode::Off
    }
}

fn parse_progress_mode(value: &str) -> AppResult<BuildProgressMode> {
    match value {
        "auto" => Ok(automatic_progress_mode()),
        "off" => Ok(BuildProgressMode::Off),
        "plain" => Ok(BuildProgressMode::Plain),
        "json" => Ok(BuildProgressMode::Json),
        _ => Err(format!("unsupported progress mode '{value}'").into()),
    }
}

fn emit_cli_progress(mode: BuildProgressMode, phase: &str, message: &str) {
    match mode {
        BuildProgressMode::Off => {}
        BuildProgressMode::Plain => eprintln!("[{phase}] {message}"),
        BuildProgressMode::Json => eprintln!(
            "{}",
            serde_json::json!({ "phase": phase, "message": message })
        ),
    }
}

fn format_cli_integer(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    let first_group = digits.len() % 3;
    for (index, byte) in digits.bytes().enumerate() {
        if index > 0 && index % 3 == first_group {
            formatted.push(',');
        }
        formatted.push(char::from(byte));
    }
    formatted
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn format_cli_duration(seconds: f64) -> String {
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

fn option_value(args: &mut impl Iterator<Item = String>, flag: &str) -> AppResult<String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn parse_option<T>(args: &mut impl Iterator<Item = String>, flag: &str) -> AppResult<T>
where
    T: std::str::FromStr,
    T::Err: Error + Send + Sync + 'static,
{
    Ok(option_value(args, flag)?.parse::<T>()?)
}

fn benchmark_encoder_scale(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let usage =
        "usage: pangenome-range benchmark-encoder-scale <graph.gbz> <run-id> <external-work-root>";
    let input = PathBuf::from(args.next().ok_or(usage)?);
    let run_id = args.next().ok_or(usage)?;
    validate_run_id(&run_id)?;
    let work_root = PathBuf::from(args.next().ok_or(usage)?);
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument '{extra}'").into());
    }
    let archive = work_root.join(&run_id).join("fixed-v1-16k-zstd3.pngr");
    let options = EncoderScaleOptions {
        input,
        archive,
        results_dir: PathBuf::from("results").join(&run_id),
        run_id,
    };
    run_encoder_scale_experiment(&options)?;
    println!("archive: {}", options.archive.display());
    println!("results: {}", options.results_dir.display());
    Ok(())
}

fn benchmark_fixed_windows(
    args: &mut impl Iterator<Item = String>,
    mode: ExperimentMode,
) -> AppResult<()> {
    let command = match mode {
        ExperimentMode::FullSweep => "benchmark-fixed-windows",
        ExperimentMode::SingleConfigSmoke => "benchmark-fixed-window-smoke",
    };
    let usage =
        format!("usage: pangenome-range {command} <graph.gbz> <run-id> [random-queries-per-size]");
    let input = PathBuf::from(args.next().ok_or_else(|| usage.clone())?);
    let run_id = args.next().ok_or_else(|| usage.clone())?;
    validate_run_id(&run_id)?;
    let random_queries_per_size = args
        .next()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(match mode {
            ExperimentMode::FullSweep => 100,
            ExperimentMode::SingleConfigSmoke => 10,
        });
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument '{extra}'").into());
    }
    let options = ExperimentOptions {
        input,
        results_dir: PathBuf::from("results").join(&run_id),
        scratch_dir: PathBuf::from("target/experiment-scratch").join(&run_id),
        run_id,
        random_queries_per_size,
        mode,
    };
    run_fixed_window_experiment(&options)?;
    println!("results: {}", options.results_dir.display());
    Ok(())
}

fn validate_run_id(run_id: &str) -> AppResult<()> {
    if run_id.is_empty()
        || !run_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err("run ID may contain only ASCII letters, digits, '-', '_', and '.'".into());
    }
    Ok(())
}

fn run_internal_gbz_base_query(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let database = PathBuf::from(args.next().ok_or("missing database path")?);
    let sample = args.next().ok_or("missing reference sample")?;
    let contig = args.next().ok_or("missing reference contig")?;
    let start = args.next().ok_or("missing start")?.parse()?;
    let end = args.next().ok_or("missing end")?.parse()?;
    let context = args.next().ok_or("missing context")?.parse()?;
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument '{extra}'").into());
    }
    internal_gbz_base_query(&database, &sample, &contig, start, end, context)
}
