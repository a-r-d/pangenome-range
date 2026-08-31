mod catalog;
mod codecs;
#[cfg(feature = "unpublished-fork-locate")]
mod locate;
mod model;
mod reconstruct;

use crate::codecs::{encode_adaptive, measure_group};
use crate::model::{LocatedPosition, PreparedManifest, PreparedTile, TileAnalysis};
use crate::reconstruct::{named_groups, traversal_starts};
use gbz::GBZ;
use pangenome_range_format::{
    DIRECTORY_PAGE_BYTES, FileRangeSource, RangeSource, RecordRegionalPayload, bootstrap,
    decode_directory_page, decompress, directory_page_offset,
};
use serde::Deserialize;
use serde_json::{Value, json};
use simple_sds::serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const POSITIONS_MAGIC: &[u8; 8] = b"PMPO0001";
const LOCATED_MAGIC: &[u8; 8] = b"PMLO0001";

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn parse_args() -> io::Result<(String, HashMap<String, String>)> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let mut values = HashMap::new();
    while let Some(key) = args.next() {
        if !key.starts_with("--") {
            return Err(invalid(format!("unexpected positional argument {key}")));
        }
        let value = args
            .next()
            .ok_or_else(|| invalid(format!("missing value for {key}")))?;
        values.insert(key, value);
    }
    Ok((command, values))
}

fn required<'a>(values: &'a HashMap<String, String>, key: &str) -> io::Result<&'a str> {
    values
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| invalid(format!("missing required argument {key}")))
}

fn parse_u64(values: &HashMap<String, String>, key: &str) -> io::Result<u64> {
    required(values, key)?
        .parse()
        .map_err(|_| invalid(format!("invalid unsigned integer for {key}")))
}

fn write_u64(output: &mut impl Write, value: u64) -> io::Result<()> {
    output.write_all(&value.to_le_bytes())
}

fn read_u64(input: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0_u8; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn u64_to_usize(value: u64) -> io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid("u64 does not fit in usize"))
}

fn path_string(path: &Path) -> io::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("path is not valid UTF-8"))
}

fn extract_gbwt(values: &HashMap<String, String>) -> io::Result<()> {
    let input = Path::new(required(values, "--gbz")?);
    let output = Path::new(required(values, "--output")?);
    if output.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "GBWT output already exists",
        ));
    }
    let graph: GBZ = serialize::load_from(input)?;
    let index: &gbz::GBWT = graph.as_ref();
    serialize::serialize_to(index, output)?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "schema_version": 1,
            "gbz": input,
            "output": output,
            "sequences": index.sequences(),
            "paths": if index.is_bidirectional() { index.sequences() / 2 } else { index.sequences() },
        }))?
    );
    Ok(())
}

fn range_union_bytes(mut ranges: Vec<(u64, u64)>) -> io::Result<u64> {
    ranges.sort_unstable();
    let mut total = 0_u64;
    let mut current: Option<(u64, u64)> = None;
    for (start, end) in ranges {
        if start > end {
            return Err(invalid("invalid byte range"));
        }
        match current {
            None => current = Some((start, end)),
            Some((current_start, current_end)) if start <= current_end => {
                current = Some((current_start, current_end.max(end)));
            }
            Some((current_start, current_end)) => {
                total = total
                    .checked_add(current_end - current_start)
                    .ok_or_else(|| invalid("range union overflow"))?;
                current = Some((start, end));
            }
        }
    }
    if let Some((start, end)) = current {
        total = total
            .checked_add(end - start)
            .ok_or_else(|| invalid("range union overflow"))?;
    }
    Ok(total)
}

#[allow(clippy::too_many_lines)]
fn prepare(values: &HashMap<String, String>) -> io::Result<()> {
    let archive_path = PathBuf::from(required(values, "--archive")?);
    let sample = required(values, "--sample")?.to_owned();
    let contig = required(values, "--contig")?.to_owned();
    let start = parse_u64(values, "--start")?;
    let end = parse_u64(values, "--end")?;
    if start >= end {
        return Err(invalid("query start must be below query end"));
    }
    let output_dir = PathBuf::from(required(values, "--payload-dir")?);
    let positions_path = PathBuf::from(required(values, "--positions")?);
    let manifest_path = PathBuf::from(required(values, "--manifest")?);
    fs::create_dir_all(&output_dir)?;

    let source = FileRangeSource::open(&archive_path)?;
    let archive_bytes = source.len()?;
    let archive_bootstrap = bootstrap(&source)?;
    let manifests = archive_bootstrap
        .root
        .manifests
        .iter()
        .filter(|manifest| {
            manifest.sample == sample
                && manifest.contig == contig
                && manifest.start < end
                && manifest.end > start
        })
        .collect::<Vec<_>>();
    if manifests.is_empty() {
        return Err(invalid("no archive manifest covers the query"));
    }

    let mut directory_ranges = BTreeSet::new();
    let mut entries = BTreeMap::new();
    for manifest in manifests {
        let selected_start = start.max(manifest.start);
        let selected_end = end.min(manifest.end);
        let first_bucket = (selected_start - manifest.grid_start) / manifest.bucket_span;
        let last_bucket = (selected_end - 1 - manifest.grid_start) / manifest.bucket_span;
        for bucket in first_bucket..=last_bucket {
            let page_offset = directory_page_offset(manifest, bucket)?;
            directory_ranges.insert((page_offset, DIRECTORY_PAGE_BYTES as u64));
            let page = source.read_range(page_offset, DIRECTORY_PAGE_BYTES)?;
            for entry in decode_directory_page(&page, manifest, bucket)? {
                if entry.start < end && entry.end > start {
                    entries.insert((entry.offset, entry.compressed_len), entry);
                }
            }
        }
    }
    if entries.is_empty() {
        return Err(invalid("query selected no regional payloads"));
    }

    let mut prepared_tiles = Vec::new();
    let mut positions = Vec::new();
    let mut payload_ranges = BTreeSet::new();
    for entry in entries.values() {
        let encoded = source.read_range(entry.offset, u64_to_usize(entry.compressed_len)?)?;
        let digest = blake3::hash(&encoded);
        if digest.as_bytes()[..16] != entry.integrity {
            return Err(invalid("regional payload integrity mismatch"));
        }
        let raw = decompress(entry.codec, &encoded, entry.uncompressed_len)?;
        let payload = RecordRegionalPayload::decode(&raw)?;
        if payload.core_start != entry.start || payload.core_end != entry.end {
            return Err(invalid("payload provenance differs from directory entry"));
        }
        let short_digest = &digest.to_hex().to_string()[..16];
        let payload_path = output_dir.join(format!(
            "tile-{}-{}-{short_digest}.raw",
            payload.core_start, payload.core_end
        ));
        fs::write(&payload_path, &raw)?;
        for traversal in traversal_starts(&payload)? {
            positions.push((traversal.handle, traversal.offset));
        }
        prepared_tiles.push(PreparedTile {
            core_start: payload.core_start,
            core_end: payload.core_end,
            payload_path: path_string(&payload_path)?,
            encoded_graph_bytes: entry.compressed_len,
            decoded_graph_bytes: entry.uncompressed_len,
        });
        payload_ranges.insert((
            entry.offset,
            entry
                .offset
                .checked_add(entry.compressed_len)
                .ok_or_else(|| invalid("payload range overflow"))?,
        ));
    }
    positions.sort_unstable();

    let mut positions_output = BufWriter::new(File::create(positions_path)?);
    positions_output.write_all(POSITIONS_MAGIC)?;
    write_u64(&mut positions_output, positions.len() as u64)?;
    for (node, offset) in positions {
        write_u64(&mut positions_output, node)?;
        write_u64(&mut positions_output, offset)?;
    }
    positions_output.flush()?;

    let bootstrap_end = archive_bootstrap.bytes.len() as u64;
    let mut graph_ranges = vec![(0, bootstrap_end)];
    graph_ranges.extend(
        directory_ranges
            .iter()
            .map(|(offset, length)| (*offset, (offset + length).min(archive_bytes))),
    );
    graph_ranges.extend(
        payload_ranges
            .iter()
            .map(|(offset, end)| (*offset, (*end).min(archive_bytes))),
    );
    let graph_query_bytes = range_union_bytes(graph_ranges)?;
    let external_directory_ranges = directory_ranges
        .iter()
        .filter(|(offset, length)| offset + length > bootstrap_end)
        .count() as u64;
    let external_payload_ranges = payload_ranges
        .iter()
        .filter(|(_, range_end)| *range_end > bootstrap_end)
        .count() as u64;
    let graph_query_ranges =
        archive_bootstrap.dependency_rounds + external_directory_ranges + external_payload_ranges;
    let prepared = PreparedManifest {
        schema_version: 1,
        archive: path_string(&archive_path)?,
        archive_bytes,
        sample,
        contig,
        start,
        end,
        graph_query_bytes,
        graph_query_ranges,
        tiles: prepared_tiles,
    };
    fs::write(manifest_path, serde_json::to_vec_pretty(&prepared)?)?;
    println!("{}", serde_json::to_string(&prepared)?);
    Ok(())
}

fn read_located(path: &Path) -> io::Result<BTreeMap<(u64, u64), LocatedPosition>> {
    let mut input = BufReader::new(File::open(path)?);
    let mut magic = [0_u8; 8];
    input.read_exact(&mut magic)?;
    if &magic != LOCATED_MAGIC {
        return Err(invalid("invalid located-position magic"));
    }
    let count = read_u64(&mut input)?;
    let mut result = BTreeMap::new();
    for _ in 0..count {
        let node = read_u64(&mut input)?;
        let offset = read_u64(&mut input)?;
        let sequence_id = read_u64(&mut input)?;
        let path_id = read_u64(&mut input)?;
        let mut flags = [0_u8; 8];
        input.read_exact(&mut flags)?;
        if flags[0] > 1 || flags[1..].iter().any(|value| *value != 0) {
            return Err(invalid("invalid located-position flags"));
        }
        let position = LocatedPosition {
            node,
            offset,
            sequence_id,
            path_id,
            reversed: flags[0] != 0,
        };
        if result
            .insert((node, offset), position.clone())
            .is_some_and(|old| old != position)
        {
            return Err(invalid("conflicting locate results for one GBWT position"));
        }
    }
    if input.fill_buf()?.is_empty() {
        Ok(result)
    } else {
        Err(invalid("located-position file has trailing bytes"))
    }
}

#[derive(Clone, Debug, Deserialize, serde::Serialize, PartialEq, Eq)]
struct CatalogPath {
    canonical_path_id: u64,
    raw_name: String,
    sample: String,
    contig: String,
    haplotype: u64,
    fragment: u64,
    path_sense: String,
}

fn varint_len(mut value: u64) -> u64 {
    let mut result = 1;
    while value >= 0x80 {
        value >>= 7;
        result += 1;
    }
    result
}

fn common_prefix(first: &[u8], second: &[u8]) -> usize {
    first
        .iter()
        .zip(second)
        .take_while(|(left, right)| left == right)
        .count()
}

fn percentile_u64(values: &[u64], percentile: u64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let numerator = percentile * (sorted.len() as u64 - 1);
    let index = usize::try_from(numerator.div_ceil(100)).unwrap();
    sorted[index]
}

fn percentile_f64(values: &[f64], percentile: u64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let numerator = percentile * (sorted.len() as u64 - 1);
    sorted[usize::try_from(numerator.div_ceil(100)).unwrap()]
}

fn distribution_u64(values: &[u64]) -> Value {
    json!({
        "count": values.len(),
        "p50": percentile_u64(values, 50),
        "p90": percentile_u64(values, 90),
        "p95": percentile_u64(values, 95),
        "p99": percentile_u64(values, 99),
        "max": values.iter().copied().max().unwrap_or(0),
    })
}

fn distribution_f64(values: &[f64]) -> Value {
    json!({
        "count": values.len(),
        "p50": percentile_f64(values, 50),
        "p90": percentile_f64(values, 90),
        "p95": percentile_f64(values, 95),
        "p99": percentile_f64(values, 99),
        "max": values.iter().copied().fold(0.0, f64::max),
    })
}

#[allow(clippy::cast_precision_loss)]
fn membership_shape(groups: &[crate::model::NamedTraversalGroup], universe_size: u64) -> Value {
    let mut occurrence_weights = Vec::new();
    let mut unique_path_counts = Vec::new();
    let mut run_lengths = Vec::new();
    let mut deltas = Vec::new();
    let mut densities = Vec::new();
    for group in groups {
        occurrence_weights.push(group.occurrence_weight);
        unique_path_counts.push(group.unique_path_count);
        densities.push(if universe_size == 0 {
            0.0
        } else {
            group.unique_path_count as f64 / universe_size as f64
        });
        let ids = group
            .memberships
            .iter()
            .map(|item| item.path_id)
            .collect::<Vec<_>>();
        if let Some((&first, rest)) = ids.split_first() {
            deltas.push(first);
            let mut run = 1_u64;
            let mut previous = first;
            for &path_id in rest {
                deltas.push(path_id - previous);
                if path_id == previous + 1 {
                    run += 1;
                } else {
                    run_lengths.push(run);
                    run = 1;
                }
                previous = path_id;
            }
            run_lengths.push(run);
        }
    }
    json!({
        "occurrence_weight": distribution_u64(&occurrence_weights),
        "unique_path_count": distribution_u64(&unique_path_counts),
        "path_id_run_length": distribution_u64(&run_lengths),
        "path_id_delta": distribution_u64(&deltas),
        "density_relative_to_catalog": distribution_f64(&densities),
    })
}

#[allow(clippy::too_many_lines)]
fn catalog_metrics(path: Option<&str>, fallback_paths: u64) -> io::Result<Value> {
    let Some(path) = path else {
        return Ok(json!({
            "path_count": fallback_paths,
            "plain_utf8_bytes": null,
            "front_coded_bytes": null,
            "columnar_bytes": null,
        }));
    };
    let input = BufReader::new(File::open(path)?);
    let mut declared_paths = None;
    let mut paths = Vec::new();
    for line in input.lines() {
        let line = line?;
        let value: Value = serde_json::from_str(&line)?;
        match value.get("type").and_then(Value::as_str) {
            Some("metadata") => declared_paths = value.get("paths").and_then(Value::as_u64),
            Some("path") => paths.push(serde_json::from_value::<CatalogPath>(value)?),
            _ => {}
        }
    }
    if declared_paths != Some(paths.len() as u64) {
        return Err(invalid("catalog metadata path count mismatch"));
    }
    paths.sort_by_key(|path| path.canonical_path_id);
    let samples = paths
        .iter()
        .map(|path| path.sample.as_str())
        .collect::<BTreeSet<_>>();
    let contigs = paths
        .iter()
        .map(|path| path.contig.as_str())
        .collect::<BTreeSet<_>>();
    let senses = paths
        .iter()
        .map(|path| path.path_sense.as_str())
        .collect::<BTreeSet<_>>();
    let string_tables = samples
        .iter()
        .map(|value| value.len() as u64 + 1)
        .sum::<u64>()
        + contigs
            .iter()
            .map(|value| value.len() as u64 + 1)
            .sum::<u64>()
        + senses
            .iter()
            .map(|value| value.len() as u64 + 1)
            .sum::<u64>();
    let sample_ids = samples
        .iter()
        .enumerate()
        .map(|(id, value)| (*value, id as u64))
        .collect::<BTreeMap<_, _>>();
    let contig_ids = contigs
        .iter()
        .enumerate()
        .map(|(id, value)| (*value, id as u64))
        .collect::<BTreeMap<_, _>>();
    let sense_ids = senses
        .iter()
        .enumerate()
        .map(|(id, value)| (*value, id as u64))
        .collect::<BTreeMap<_, _>>();
    let row_columns = |path: &CatalogPath| {
        varint_len(path.canonical_path_id)
            + varint_len(sample_ids[path.sample.as_str()])
            + varint_len(contig_ids[path.contig.as_str()])
            + varint_len(path.haplotype)
            + varint_len(path.fragment)
            + varint_len(sense_ids[path.path_sense.as_str()])
    };
    let columns = paths.iter().map(row_columns).sum::<u64>();
    let plain = paths
        .iter()
        .map(|path| {
            row_columns(path)
                + varint_len(path.raw_name.len() as u64)
                + path.raw_name.len() as u64
                + varint_len(path.sample.len() as u64)
                + path.sample.len() as u64
                + varint_len(path.contig.len() as u64)
                + path.contig.len() as u64
                + varint_len(path.path_sense.len() as u64)
                + path.path_sense.len() as u64
        })
        .sum::<u64>();
    let front_coded = |ordered: &[&CatalogPath]| {
        let mut total = 0_u64;
        let mut previous: &[u8] = &[];
        for path in ordered {
            let name = path.raw_name.as_bytes();
            let prefix = common_prefix(previous, name);
            let suffix = name.len() - prefix;
            total += varint_len(prefix as u64)
                + varint_len(suffix as u64)
                + suffix as u64
                + row_columns(path);
            previous = name;
        }
        total + string_tables
    };
    let by_path_id = paths.iter().collect::<Vec<_>>();
    let mut by_name = by_path_id.clone();
    by_name.sort_by_key(|path| path.raw_name.as_str());
    let front = front_coded(&by_name);
    let columnar = front_coded(&by_path_id);
    Ok(json!({
        "path_count": paths.len(),
        "plain_utf8_bytes": plain,
        "front_coded_bytes": front,
        "columnar_bytes": columnar,
        "integer_metadata_column_bytes": columns,
        "deduplicated_string_table_bytes": string_tables,
        "sample_count": samples.len(),
        "contig_count": contigs.len(),
    }))
}

fn read_catalog_paths(path: &Path) -> io::Result<BTreeMap<u64, CatalogPath>> {
    let input = BufReader::new(File::open(path)?);
    let mut result = BTreeMap::new();
    for line in input.lines() {
        let value: Value = serde_json::from_str(&line?)?;
        if value.get("type").and_then(Value::as_str) != Some("path") {
            continue;
        }
        let entry: CatalogPath = serde_json::from_value(value)?;
        if result.insert(entry.canonical_path_id, entry).is_some() {
            return Err(invalid("duplicate path id in catalog"));
        }
    }
    Ok(result)
}

fn answer(values: &HashMap<String, String>) -> io::Result<()> {
    let summary: Value = serde_json::from_slice(&fs::read(required(values, "--summary")?)?)?;
    let catalog = read_catalog_paths(Path::new(required(values, "--catalog")?))?;
    let requested_group = values
        .get("--group-index")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| invalid("invalid --group-index"))
        })
        .transpose()?;
    let requested_node = values
        .get("--node")
        .map(|value| value.parse::<u64>().map_err(|_| invalid("invalid --node")))
        .transpose()?;
    if requested_group.is_some() == requested_node.is_some() {
        return Err(invalid("specify exactly one of --group-index or --node"));
    }

    let mut global_index = 0_usize;
    let mut matched_groups = Vec::new();
    let mut path_occurrences = BTreeMap::<u64, u64>::new();
    for tile in summary["tiles"]
        .as_array()
        .ok_or_else(|| invalid("summary has no tiles"))?
    {
        for group in tile["groups"]
            .as_array()
            .ok_or_else(|| invalid("tile has no groups"))?
        {
            let matches = requested_group == Some(global_index)
                || requested_node.is_some_and(|node| {
                    group["traversal"].as_array().is_some_and(|handles| {
                        handles.iter().any(|handle| handle.as_u64() == Some(node))
                    })
                });
            if matches {
                matched_groups.push(global_index);
                for member in group["memberships"]
                    .as_array()
                    .ok_or_else(|| invalid("group has no memberships"))?
                {
                    let path_id = member["path_id"]
                        .as_u64()
                        .ok_or_else(|| invalid("invalid path id"))?;
                    let multiplicity = member["multiplicity"]
                        .as_u64()
                        .ok_or_else(|| invalid("invalid multiplicity"))?;
                    *path_occurrences.entry(path_id).or_default() += multiplicity;
                }
            }
            global_index += 1;
        }
    }
    let mut samples = BTreeSet::new();
    let metadata = path_occurrences
        .iter()
        .map(|(&path_id, &local_multiplicity)| {
            let entry = catalog
                .get(&path_id)
                .ok_or_else(|| invalid(format!("catalog lacks path {path_id}")))?;
            samples.insert(entry.sample.clone());
            Ok(json!({
                "path_id": path_id,
                "raw_name": entry.raw_name,
                "sample": entry.sample,
                "contig": entry.contig,
                "haplotype": entry.haplotype,
                "fragment": entry.fragment,
                "path_sense": entry.path_sense,
                "local_multiplicity": local_multiplicity,
            }))
        })
        .collect::<io::Result<Vec<_>>>()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "matched_group_indices": matched_groups,
            "unique_path_count": path_occurrences.len(),
            "local_traversal_occurrence_count": path_occurrences.values().sum::<u64>(),
            "unique_sample_count": samples.len(),
            "samples": samples,
            "paths": metadata,
        }))?
    );
    Ok(())
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn analyze(values: &HashMap<String, String>) -> io::Result<()> {
    let manifest: PreparedManifest =
        serde_json::from_slice(&fs::read(required(values, "--manifest")?)?)?;
    let located = read_located(Path::new(required(values, "--located")?))?;
    let max_path_id = located
        .values()
        .map(|position| position.path_id)
        .max()
        .unwrap_or(0);
    let catalog = catalog_metrics(values.get("--catalog").map(String::as_str), max_path_id + 1)?;
    let universe_size = catalog["path_count"]
        .as_u64()
        .unwrap_or(max_path_id + 1)
        .max(max_path_id + 1);

    let encode_started = Instant::now();
    let mut analyses = Vec::new();
    let mut membership_bytes = 0_u64;
    let mut all_codec_rows = Vec::new();
    let mut all_groups = Vec::new();
    let mut adaptive_corpus = Vec::new();
    for tile in &manifest.tiles {
        let payload = RecordRegionalPayload::decode(&fs::read(&tile.payload_path)?)?;
        let groups = named_groups(&payload, &located)?;
        let total_weight = groups
            .iter()
            .map(|group| group.occurrence_weight)
            .sum::<u64>();
        let mut path_group_counts = BTreeMap::<u64, BTreeSet<usize>>::new();
        let mut path_occurrences = BTreeMap::<u64, u64>::new();
        for (group_index, group) in groups.iter().enumerate() {
            for membership in &group.memberships {
                path_group_counts
                    .entry(membership.path_id)
                    .or_default()
                    .insert(group_index);
                *path_occurrences.entry(membership.path_id).or_default() += membership.multiplicity;
            }
        }
        let tile_universe = path_group_counts.keys().copied().collect::<Vec<_>>();
        let disjoint_partition = path_occurrences.values().all(|count| *count == 1)
            && path_group_counts.values().all(|groups| groups.len() == 1);
        let mut codec_sizes = Vec::new();
        for group in &groups {
            let measured = measure_group(
                group,
                universe_size,
                disjoint_partition.then_some(tile_universe.as_slice()),
            )?;
            let encoded = encode_adaptive(
                group,
                universe_size,
                disjoint_partition.then_some(tile_universe.as_slice()),
            )?;
            if encoded.len() as u64 != measured.adaptive_bytes {
                return Err(invalid(
                    "adaptive codec measurement differs from encoded bytes",
                ));
            }
            adaptive_corpus.push(encoded);
            membership_bytes += measured.adaptive_bytes;
            all_codec_rows.push((
                tile.core_start,
                tile.core_end,
                group.clone(),
                measured.clone(),
            ));
            codec_sizes.push(measured);
        }
        all_groups.extend(groups.iter().cloned());
        let dominant = groups
            .iter()
            .map(|group| group.occurrence_weight)
            .max()
            .unwrap_or(0);
        analyses.push(TileAnalysis {
            core_start: tile.core_start,
            core_end: tile.core_end,
            distinct_groups: groups.len() as u64,
            total_occurrence_weight: total_weight,
            unique_paths: path_group_counts.len() as u64,
            paths_in_multiple_groups: path_group_counts
                .values()
                .filter(|groups| groups.len() > 1)
                .count() as u64,
            paths_with_multiplicity_gt_one: path_occurrences
                .values()
                .filter(|count| **count > 1)
                .count() as u64,
            disjoint_partition,
            dominant_group_share: if total_weight == 0 {
                0.0
            } else {
                dominant as f64 / total_weight as f64
            },
            groups,
            codecs: codec_sizes,
        });
    }

    let catalog_bytes = ["columnar_bytes", "front_coded_bytes", "plain_utf8_bytes"]
        .into_iter()
        .filter_map(|key| catalog[key].as_u64())
        .min()
        .unwrap_or(0);
    let extension_index_bytes = 64 + analyses.len() as u64 * 56;
    let identity_bytes = catalog_bytes + membership_bytes + extension_index_bytes;
    let query_identity_bytes = membership_bytes + catalog_bytes;
    let membership_encoding_wall_ms = encode_started.elapsed().as_secs_f64() * 1_000.0;
    let base = manifest.archive_bytes;
    let benchmark = values
        .get("--benchmark-json")
        .map(|path| -> io::Result<Value> { Ok(serde_json::from_slice(&fs::read(path)?)?) })
        .transpose()?;
    let summary = json!({
        "schema_version": 1,
        "manifest": manifest,
        "catalog": catalog,
        "tiles": analyses,
        "membership": {
            "adaptive_bytes": membership_bytes,
            "catalog_bytes": catalog_bytes,
            "extension_index_bytes": extension_index_bytes,
            "identity_total_bytes": identity_bytes,
            "overhead_percent": if base == 0 { 0.0 } else { identity_bytes as f64 * 100.0 / base as f64 },
            "encoding_wall_ms": membership_encoding_wall_ms,
        },
        "membership_structure": membership_shape(&all_groups, universe_size),
        "placement": {
            "inline": {
                "base_graph_archive_bytes": base,
                "total_identity_aware_bytes": base + catalog_bytes + membership_bytes,
                "query_bytes_without_identities": manifest.graph_query_bytes + membership_bytes,
                "query_bytes_with_identities": manifest.graph_query_bytes + query_identity_bytes,
                "additional_range_requests": 0,
                "additional_dependency_rounds": 0,
            },
            "same_object_extension": {
                "base_graph_archive_bytes": base,
                "total_identity_aware_bytes": base + identity_bytes,
                "query_bytes_without_identities": manifest.graph_query_bytes,
                "query_bytes_with_identities": manifest.graph_query_bytes + query_identity_bytes,
                "additional_range_requests": 2,
                "additional_dependency_rounds": 1,
                "type_ids": ["path-catalog-v1-", "tile-members-v1-"],
            },
            "sidecar": {
                "base_graph_archive_bytes": base,
                "sidecar_bytes": identity_bytes,
                "total_identity_aware_bytes": base + identity_bytes,
                "query_bytes_without_identities": manifest.graph_query_bytes,
                "query_bytes_with_identities": manifest.graph_query_bytes + query_identity_bytes,
                "additional_range_requests": 2,
                "additional_dependency_rounds": 1,
                "requires_archive_digest_binding": true,
            },
        },
        "correctness": {
            "anonymous_weight_equals_membership_multiplicity_sum": true,
            "source_identity_locate_results": located.len(),
        },
        "benchmark": benchmark,
    });
    fs::write(
        required(values, "--summary")?,
        serde_json::to_vec_pretty(&summary)?,
    )?;
    if let Some(path) = values.get("--codec-corpus") {
        let mut output = BufWriter::new(File::create(path)?);
        output.write_all(b"PMAC0001")?;
        write_u64(&mut output, adaptive_corpus.len() as u64)?;
        for encoded in adaptive_corpus {
            write_u64(&mut output, encoded.len() as u64)?;
            output.write_all(&encoded)?;
        }
        output.flush()?;
    }

    let mut tiles = BufWriter::new(File::create(required(values, "--tiles")?)?);
    writeln!(
        tiles,
        "core_start,core_end,distinct_groups,total_occurrence_weight,unique_paths,paths_in_multiple_groups,paths_with_multiplicity_gt_one,disjoint_partition,dominant_group_share"
    )?;
    for tile in summary["tiles"].as_array().unwrap() {
        writeln!(
            tiles,
            "{},{},{},{},{},{},{},{},{}",
            tile["core_start"],
            tile["core_end"],
            tile["distinct_groups"],
            tile["total_occurrence_weight"],
            tile["unique_paths"],
            tile["paths_in_multiple_groups"],
            tile["paths_with_multiplicity_gt_one"],
            tile["disjoint_partition"],
            tile["dominant_group_share"]
        )?;
    }
    let mut codecs = BufWriter::new(File::create(required(values, "--codecs")?)?);
    writeln!(
        codecs,
        "core_start,core_end,group_index,occurrence_weight,unique_path_count,delta_varint,interval_run,dense_bitset,roaring_blocks,complement,adaptive_codec,adaptive_bytes"
    )?;
    for (index, (core_start, core_end, group, measured)) in all_codec_rows.iter().enumerate() {
        writeln!(
            codecs,
            "{core_start},{core_end},{index},{},{},{},{},{},{},{},{},{}",
            group.occurrence_weight,
            group.unique_path_count,
            measured.delta_varint,
            measured.interval_run,
            measured.dense_bitset,
            measured.roaring_blocks,
            measured
                .complement
                .map_or_else(String::new, |value| value.to_string()),
            measured.adaptive_codec,
            measured.adaptive_bytes
        )?;
    }
    if let Some(path) = values.get("--queries") {
        let mut queries = BufWriter::new(File::create(path)?);
        writeln!(
            queries,
            "sample,contig,start,end,graph_bytes,identity_bytes,total_bytes,additional_ranges,additional_rounds"
        )?;
        writeln!(
            queries,
            "{},{},{},{},{},{},{},2,1",
            manifest.sample,
            manifest.contig,
            manifest.start,
            manifest.end,
            manifest.graph_query_bytes,
            query_identity_bytes,
            manifest.graph_query_bytes + query_identity_bytes
        )?;
    }
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

fn print_help() {
    println!("path-membership-rust commands:");
    println!(
        "  prepare --archive FILE --sample NAME --contig NAME --start N --end N --payload-dir DIR --positions FILE --manifest FILE"
    );
    println!(
        "  analyze --manifest FILE --located FILE [--catalog NDJSON] --summary FILE --tiles CSV --codecs CSV [--queries CSV] [--codec-corpus FILE] [--benchmark-json FILE]"
    );
    println!("  answer --summary FILE --catalog NDJSON (--group-index N | --node ORIENTED_HANDLE)");
    #[cfg(feature = "unpublished-fork-locate")]
    {
        println!(
            "  locate-rust --gbwt FILE --positions FILE --output FILE --stats JSON [--max-lf-steps N]"
        );
        println!("  sample-positions --gbwt FILE --count N --seed N --output FILE");
        println!("  verify-locate-tsv --gbwt FILE --expected TSV --stats JSON [--max-lf-steps N]");
        println!(
            "  verify-sampled-sequences --gbwt FILE --count N --seed N --max-walk-steps N --stats JSON [--max-lf-steps N]"
        );
    }
    println!("  export-catalog --gbwt FILE --output NDJSON");
    println!("  extract-gbwt --gbz FILE --output FILE");
    println!(
        "  build-paged-catalog --catalog NDJSON --output FILE --records-per-page N --stats JSON"
    );
    println!(
        "  verify-paged-catalog --catalog NDJSON --paged FILE --query-ids FILE --max-data-ranges N --stats JSON"
    );
}

fn main() -> io::Result<()> {
    let (command, values) = parse_args()?;
    match command.as_str() {
        "prepare" => prepare(&values),
        "analyze" => analyze(&values),
        "answer" => answer(&values),
        #[cfg(feature = "unpublished-fork-locate")]
        "locate-rust" => locate::locate_positions(&values),
        #[cfg(feature = "unpublished-fork-locate")]
        "sample-positions" => locate::sample_positions(&values),
        #[cfg(feature = "unpublished-fork-locate")]
        "verify-locate-tsv" => locate::verify_tsv(&values),
        #[cfg(feature = "unpublished-fork-locate")]
        "verify-sampled-sequences" => locate::verify_sampled_sequences(&values),
        "export-catalog" => catalog::export_catalog(&values),
        "extract-gbwt" => extract_gbwt(&values),
        "build-paged-catalog" => catalog::build_paged_catalog(&values),
        "verify-paged-catalog" => catalog::verify_paged_catalog(&values),
        "" | "help" | "--help" => {
            print_help();
            Ok(())
        }
        _ => Err(invalid(format!("unknown command {command}"))),
    }
}
