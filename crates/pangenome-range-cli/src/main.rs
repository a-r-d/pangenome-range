use gbz::GBZ;
use gbz_base::PathIndex;
use pangenome_range_build::{
    BuildProgressMode, ChunkCodec, EncodeOptions, EncoderScaleOptions, ExperimentMode,
    ExperimentOptions, FixedArchiveConfig, QuerySpec, internal_gbz_base_query, query_fixed_archive,
    run_encode, run_encoder_scale_experiment, run_fixed_window_experiment, source_oracle,
};
use pangenome_range_format::{FileRangeSource, NetworkProfile, RangeSource, TracingRangeSource};
use simple_sds::serialize;
use std::collections::BTreeSet;
use std::error::Error;
use std::io::IsTerminal;
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
        "verify" => verify(&mut args),
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

fn print_help() {
    println!("pangenome-range research CLI");
    println!();
    println!("Usage:");
    println!("  pangenome-range encode <input.gbz> <output.pngr> [options]");
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
    println!();
    println!();
    println!("Encode options:");
    println!("  --sample NAME              select a reference sample");
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
    println!("  --scratch-dir PATH         validate/report a research scratch location");
    println!("  --keep-partial             retain the sibling temp archive on failure");
    println!("  --progress auto|plain|json|off");
    println!("  --progress-interval-seconds N  chunk progress cadence (default: 5)");
    println!("  --max-chunks N             bounded single-reference research guard");
    println!("  --report PATH              JSON build report path");
    println!();
    println!("Reserved experiment commands: build, query, benchmark");
}

fn verify(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let usage = "usage: pangenome-range verify <input.pngr> --against <input.gbz> --sample NAME --contig NAME --start BP --end BP [options]";
    let archive = PathBuf::from(args.next().ok_or(usage)?);
    let mut against = None;
    let mut sample = None;
    let mut contig = None;
    let mut start = None;
    let mut end = None;
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
    let query = QuerySpec {
        id: "cli-verify".into(),
        class: "post-build-verification".into(),
        sample: sample.ok_or("verify requires --sample")?,
        contig: contig.ok_or("verify requires --contig")?,
        start: start.ok_or("verify requires --start")?,
        end: end.ok_or("verify requires --end")?,
        context,
    };
    let graph: GBZ = serialize::load_from(&against)?;
    let path_index = PathIndex::new(&graph, 1_000, false)?;
    let oracle = source_oracle(&graph, &path_index, &query)?;
    let archive_config = FixedArchiveConfig {
        experiment_id: "cli-verify".into(),
        window_size,
        codec,
        deduplicate_chunks: false,
        max_uncompressed_chunk_bytes: 8 * 1024 * 1024,
        min_window_size: 1_024,
    };
    let measurement = query_fixed_archive(
        &archive,
        &archive_config,
        &query,
        coalescing_gap,
        &oracle,
        &graph,
        &path_index,
    )?;
    println!("{}", serde_json::to_string_pretty(&measurement)?);
    Ok(())
}

fn encode(args: &mut impl Iterator<Item = String>) -> AppResult<()> {
    let usage = "usage: pangenome-range encode <input.gbz> <output.pngr> [options]";
    let input = PathBuf::from(args.next().ok_or(usage)?);
    let output = PathBuf::from(args.next().ok_or(usage)?);
    let mut options = EncodeOptions::new(input, output);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--sample" => options.sample = Some(option_value(args, &flag)?),
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
            "--scratch-dir" => {
                options.scratch_dir = Some(PathBuf::from(option_value(args, &flag)?));
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
                        "unsupported haplotype mode '{value}'; scalable v4 encode requires anonymous-distinct-weighted-tile-paths"
                    )
                    .into());
                }
            }
            "--progress" => {
                let value = option_value(args, &flag)?;
                options.progress = match value.as_str() {
                    "auto" if std::io::stderr().is_terminal() => BuildProgressMode::Plain,
                    "auto" | "off" => BuildProgressMode::Off,
                    "plain" => BuildProgressMode::Plain,
                    "json" => BuildProgressMode::Json,
                    _ => return Err(format!("unsupported progress mode '{value}'").into()),
                };
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
    println!("archive: {}", summary.output_path.display());
    println!("archive sha256: {}", summary.output_sha256);
    println!("chunks: {}", summary.build.directory_entries);
    println!("construction: {:.3} ms", summary.build.construction_wall_ms);
    Ok(())
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
    let archive = work_root.join(&run_id).join("fixed-v4-16k-zstd3.pngr");
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
