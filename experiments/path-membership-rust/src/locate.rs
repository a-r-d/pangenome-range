use gbz::bwt::BWT;
use gbz::support::{self, Orientation};
use gbz::{GBWT, Pos};
use serde::Serialize as SerdeSerialize;
use simple_sds::serialize;
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::Instant;

const POSITIONS_MAGIC: &[u8; 8] = b"PMPO0001";
const LOCATED_MAGIC: &[u8; 8] = b"PMLO0001";
const DEFAULT_MAX_LF_STEPS: usize = 1_000_000;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Located {
    node: usize,
    offset: usize,
    sequence_id: usize,
    lf_steps: usize,
}

struct LocateIndex {
    gbwt: GBWT,
}

impl LocateIndex {
    fn load(path: &Path) -> io::Result<Self> {
        let gbwt: GBWT = serialize::load_from(path)?;
        if gbwt.document_array_samples() == 0 {
            return Err(invalid("GBWT contains no document-array samples"));
        }
        Ok(Self { gbwt })
    }

    fn locate(&self, initial: Pos, max_lf_steps: usize) -> io::Result<Located> {
        let (sequence_id, lf_steps) = self.gbwt.locate(initial, max_lf_steps).ok_or_else(|| {
            invalid(format!(
                "invalid GBWT position or locate exceeded the {max_lf_steps}-step safety limit at node {} offset {}",
                initial.node, initial.offset
            ))
        })?;
        Ok(Located {
            node: initial.node,
            offset: initial.offset,
            sequence_id,
            lf_steps,
        })
    }
}

#[derive(Debug, SerdeSerialize)]
struct StepDistribution {
    p50: usize,
    p90: usize,
    p95: usize,
    p99: usize,
    max: usize,
}

#[derive(Debug, SerdeSerialize)]
struct LocateStats {
    schema_version: u32,
    gbwt: String,
    gbwt_bytes: u64,
    stored_sequences: usize,
    gbwt_records: usize,
    sampled_records: usize,
    da_samples: usize,
    da_serialized_bytes: usize,
    requested_positions: usize,
    max_lf_steps_limit: usize,
    total_lf_steps: usize,
    lf_steps: StepDistribution,
    load_wall_ms: f64,
    locate_wall_ms: f64,
}

#[derive(Debug, SerdeSerialize)]
struct SampledSequenceVerifyStats {
    #[serde(flatten)]
    locate: LocateStats,
    seed: u64,
    max_walk_steps: usize,
    oracle_walk_wall_ms: f64,
    exact_sequence_matches: usize,
}

fn percentile(sorted: &[usize], percent: usize) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let rank = sorted.len().saturating_mul(percent).saturating_add(99) / 100;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn stats(
    index: &LocateIndex,
    gbwt_path: &Path,
    requested_positions: usize,
    max_lf_steps: usize,
    mut steps: Vec<usize>,
    load_wall_ms: f64,
    locate_wall_ms: f64,
) -> io::Result<LocateStats> {
    steps.sort_unstable();
    let bwt: &BWT = index.gbwt.as_ref();
    Ok(LocateStats {
        schema_version: 1,
        gbwt: gbwt_path
            .to_str()
            .ok_or_else(|| invalid("GBWT path is not UTF-8"))?
            .to_owned(),
        gbwt_bytes: std::fs::metadata(gbwt_path)?.len(),
        stored_sequences: index.gbwt.sequences(),
        gbwt_records: bwt.len(),
        sampled_records: index.gbwt.document_array_sampled_records(),
        da_samples: index.gbwt.document_array_samples(),
        da_serialized_bytes: index.gbwt.document_array_sample_bytes(),
        requested_positions,
        max_lf_steps_limit: max_lf_steps,
        total_lf_steps: steps.iter().sum(),
        lf_steps: StepDistribution {
            p50: percentile(&steps, 50),
            p90: percentile(&steps, 90),
            p95: percentile(&steps, 95),
            p99: percentile(&steps, 99),
            max: steps.last().copied().unwrap_or(0),
        },
        load_wall_ms,
        locate_wall_ms,
    })
}

fn parse_limit(values: &HashMap<String, String>) -> io::Result<usize> {
    values
        .get("--max-lf-steps")
        .map_or(Ok(DEFAULT_MAX_LF_STEPS), |value| {
            value
                .parse()
                .map_err(|_| invalid("invalid unsigned integer for --max-lf-steps"))
        })
}

fn required<'a>(values: &'a HashMap<String, String>, key: &str) -> io::Result<&'a str> {
    values
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| invalid(format!("missing required argument {key}")))
}

fn parse_usize(values: &HashMap<String, String>, key: &str) -> io::Result<usize> {
    required(values, key)?
        .parse()
        .map_err(|_| invalid(format!("invalid unsigned integer for {key}")))
}

fn next_random(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

fn read_u64(input: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0_u8; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn write_u64(output: &mut impl Write, value: usize) -> io::Result<()> {
    let value = u64::try_from(value).map_err(|_| invalid("usize does not fit in u64"))?;
    output.write_all(&value.to_le_bytes())
}

fn write_located(output: &mut impl Write, located: Located, bidirectional: bool) -> io::Result<()> {
    write_u64(output, located.node)?;
    write_u64(output, located.offset)?;
    write_u64(output, located.sequence_id)?;
    let path_id = if bidirectional {
        support::path_id(located.sequence_id)
    } else {
        located.sequence_id
    };
    write_u64(output, path_id)?;
    let reversed =
        bidirectional && support::path_orientation(located.sequence_id) == Orientation::Reverse;
    output.write_all(&[u8::from(reversed), 0, 0, 0, 0, 0, 0, 0])
}

pub fn locate_positions(values: &HashMap<String, String>) -> io::Result<()> {
    let gbwt_path = Path::new(required(values, "--gbwt")?);
    let max_lf_steps = parse_limit(values)?;
    let load_started = Instant::now();
    let index = LocateIndex::load(gbwt_path)?;
    let load_wall_ms = load_started.elapsed().as_secs_f64() * 1_000.0;

    let mut input = BufReader::new(File::open(required(values, "--positions")?)?);
    let mut magic = [0_u8; 8];
    input.read_exact(&mut magic)?;
    if &magic != POSITIONS_MAGIC {
        return Err(invalid("invalid located-position input magic"));
    }
    let count = usize::try_from(read_u64(&mut input)?)
        .map_err(|_| invalid("position count does not fit in usize"))?;
    let locate_started = Instant::now();
    let mut located = Vec::with_capacity(count);
    let mut steps = Vec::with_capacity(count);
    for _ in 0..count {
        let node = usize::try_from(read_u64(&mut input)?)
            .map_err(|_| invalid("node does not fit in usize"))?;
        let offset = usize::try_from(read_u64(&mut input)?)
            .map_err(|_| invalid("offset does not fit in usize"))?;
        let value = index.locate(Pos::new(node, offset), max_lf_steps)?;
        steps.push(value.lf_steps);
        located.push(value);
    }
    if !input.fill_buf()?.is_empty() {
        return Err(invalid("position input has trailing bytes"));
    }
    let locate_wall_ms = locate_started.elapsed().as_secs_f64() * 1_000.0;

    let mut output = BufWriter::new(File::create(required(values, "--output")?)?);
    output.write_all(LOCATED_MAGIC)?;
    write_u64(&mut output, count)?;
    for value in located {
        write_located(&mut output, value, index.gbwt.is_bidirectional())?;
    }
    output.flush()?;

    let result = stats(
        &index,
        gbwt_path,
        count,
        max_lf_steps,
        steps,
        load_wall_ms,
        locate_wall_ms,
    )?;
    let encoded = serde_json::to_vec_pretty(&result)?;
    std::fs::write(required(values, "--stats")?, &encoded)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

pub fn sample_positions(values: &HashMap<String, String>) -> io::Result<()> {
    let gbwt_path = Path::new(required(values, "--gbwt")?);
    let count: usize = required(values, "--count")?
        .parse()
        .map_err(|_| invalid("invalid unsigned integer for --count"))?;
    if count == 0 || count > 1_000_000 {
        return Err(invalid("--count must be between 1 and 1000000"));
    }
    let mut state: u64 = required(values, "--seed")?
        .parse()
        .map_err(|_| invalid("invalid unsigned integer for --seed"))?;
    let gbwt: GBWT = serialize::load_from(gbwt_path)?;
    let bwt: &BWT = gbwt.as_ref();
    let node_span = gbwt
        .alphabet_size()
        .checked_sub(gbwt.first_node())
        .ok_or_else(|| invalid("GBWT has an invalid effective alphabet"))?;
    if node_span == 0 {
        return Err(invalid("cannot sample positions from an empty GBWT"));
    }
    let node_span_u64 =
        u64::try_from(node_span).map_err(|_| invalid("GBWT alphabet span does not fit in u64"))?;

    let max_attempts = count
        .checked_mul(10_000)
        .ok_or_else(|| invalid("position sampling attempt limit overflow"))?;
    let mut positions = BTreeSet::new();
    for _ in 0..max_attempts {
        let node_offset = usize::try_from(next_random(&mut state) % node_span_u64)
            .map_err(|_| invalid("sampled node offset does not fit in usize"))?;
        let node = gbwt.first_node() + node_offset;
        let Some(record) = bwt.record(gbwt.node_to_record(node)) else {
            continue;
        };
        let record_len = record.len();
        if record_len == 0 {
            continue;
        }
        let record_len_u64 = u64::try_from(record_len)
            .map_err(|_| invalid("GBWT record length does not fit in u64"))?;
        let offset = usize::try_from(next_random(&mut state) % record_len_u64)
            .map_err(|_| invalid("sampled record offset does not fit in usize"))?;
        positions.insert((node, offset));
        if positions.len() == count {
            break;
        }
    }
    if positions.len() != count {
        return Err(invalid(format!(
            "found only {} unique positions after {max_attempts} attempts",
            positions.len()
        )));
    }

    let mut output = BufWriter::new(File::create(required(values, "--output")?)?);
    output.write_all(POSITIONS_MAGIC)?;
    write_u64(&mut output, positions.len())?;
    for (node, offset) in positions {
        write_u64(&mut output, node)?;
        write_u64(&mut output, offset)?;
    }
    output.flush()?;
    Ok(())
}

pub fn verify_sampled_sequences(values: &HashMap<String, String>) -> io::Result<()> {
    let gbwt_path = Path::new(required(values, "--gbwt")?);
    let count = parse_usize(values, "--count")?;
    if count == 0 || count > 4096 {
        return Err(invalid("--count must be between 1 and 4096"));
    }
    let max_walk_steps = parse_usize(values, "--max-walk-steps")?;
    if max_walk_steps == 0 || max_walk_steps > 1_000_000 {
        return Err(invalid("--max-walk-steps must be between 1 and 1000000"));
    }
    let seed: u64 = required(values, "--seed")?
        .parse()
        .map_err(|_| invalid("invalid unsigned integer for --seed"))?;
    let max_lf_steps = parse_limit(values)?;

    let load_started = Instant::now();
    let index = LocateIndex::load(gbwt_path)?;
    let load_wall_ms = load_started.elapsed().as_secs_f64() * 1_000.0;
    if index.gbwt.sequences() == 0 {
        return Err(invalid("cannot sample an empty GBWT"));
    }
    let sequences = u64::try_from(index.gbwt.sequences())
        .map_err(|_| invalid("GBWT sequence count does not fit in u64"))?;
    let walk_span = u64::try_from(max_walk_steps)
        .map_err(|_| invalid("walk step limit does not fit in u64"))?;

    let oracle_started = Instant::now();
    let max_attempts = count
        .checked_mul(1_000)
        .ok_or_else(|| invalid("sequence sampling attempt limit overflow"))?;
    let mut state = seed;
    let mut selected_sequences = BTreeSet::new();
    let mut expected = Vec::with_capacity(count);
    for _ in 0..max_attempts {
        let sequence_id = usize::try_from(next_random(&mut state) % sequences)
            .map_err(|_| invalid("sampled sequence ID does not fit in usize"))?;
        if !selected_sequences.insert(sequence_id) {
            continue;
        }
        let Some(mut pos) = index.gbwt.start(sequence_id) else {
            continue;
        };
        let target_steps = usize::try_from(next_random(&mut state) % walk_span)
            .map_err(|_| invalid("sampled walk length does not fit in usize"))?;
        for _ in 0..target_steps {
            let Some(next) = index.gbwt.forward(pos) else {
                break;
            };
            pos = next;
        }
        expected.push((pos, sequence_id));
        if expected.len() == count {
            break;
        }
    }
    if expected.len() != count {
        return Err(invalid(format!(
            "found only {} non-empty unique sequences after {max_attempts} attempts",
            expected.len()
        )));
    }
    let oracle_walk_wall_ms = oracle_started.elapsed().as_secs_f64() * 1_000.0;

    let locate_started = Instant::now();
    let mut steps = Vec::with_capacity(count);
    for (pos, expected_sequence) in expected {
        let actual = index.locate(pos, max_lf_steps)?;
        if actual.sequence_id != expected_sequence {
            return Err(invalid(format!(
                "locate mismatch at node {} offset {}: got sequence {}, expected {expected_sequence}",
                pos.node, pos.offset, actual.sequence_id
            )));
        }
        steps.push(actual.lf_steps);
    }
    let locate_wall_ms = locate_started.elapsed().as_secs_f64() * 1_000.0;
    let result = SampledSequenceVerifyStats {
        locate: stats(
            &index,
            gbwt_path,
            count,
            max_lf_steps,
            steps,
            load_wall_ms,
            locate_wall_ms,
        )?,
        seed,
        max_walk_steps,
        oracle_walk_wall_ms,
        exact_sequence_matches: count,
    };
    let encoded = serde_json::to_vec_pretty(&result)?;
    std::fs::write(required(values, "--stats")?, &encoded)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn parse_oriented_node(value: &str) -> io::Result<usize> {
    let (id, orientation) = value.split_at(value.len().saturating_sub(1));
    let id: usize = id
        .parse()
        .map_err(|_| invalid(format!("invalid oriented node {value}")))?;
    let orientation = match orientation {
        "+" => Orientation::Forward,
        "-" => Orientation::Reverse,
        _ => return Err(invalid(format!("invalid oriented node {value}"))),
    };
    Ok(support::encode_node(id, orientation))
}

pub fn verify_tsv(values: &HashMap<String, String>) -> io::Result<()> {
    let gbwt_path = Path::new(required(values, "--gbwt")?);
    let max_lf_steps = parse_limit(values)?;
    let load_started = Instant::now();
    let index = LocateIndex::load(gbwt_path)?;
    let load_wall_ms = load_started.elapsed().as_secs_f64() * 1_000.0;
    let expected = BufReader::new(File::open(required(values, "--expected")?)?);
    let locate_started = Instant::now();
    let mut checked = 0_usize;
    let mut steps = Vec::new();
    for (line_number, line) in expected.lines().enumerate() {
        let line = line?;
        if line_number == 0 || line.trim().is_empty() {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 5 {
            return Err(invalid(format!(
                "expected TSV line {} has {} fields",
                line_number + 1,
                fields.len()
            )));
        }
        let node = parse_oriented_node(fields[0])?;
        let offset: usize = fields[1]
            .parse()
            .map_err(|_| invalid("invalid expected record offset"))?;
        let expected_sequence: usize = fields[2]
            .parse()
            .map_err(|_| invalid("invalid expected sequence ID"))?;
        let expected_path: usize = fields[3]
            .parse()
            .map_err(|_| invalid("invalid expected path ID"))?;
        let expected_reversed = match fields[4] {
            "forward" => false,
            "reverse" => true,
            _ => return Err(invalid("invalid expected sequence orientation")),
        };
        let actual = index.locate(Pos::new(node, offset), max_lf_steps)?;
        let actual_path = if index.gbwt.is_bidirectional() {
            support::path_id(actual.sequence_id)
        } else {
            actual.sequence_id
        };
        let actual_reversed = index.gbwt.is_bidirectional()
            && support::path_orientation(actual.sequence_id) == Orientation::Reverse;
        if actual.sequence_id != expected_sequence
            || actual_path != expected_path
            || actual_reversed != expected_reversed
        {
            return Err(invalid(format!(
                "locate mismatch at {} offset {offset}: got sequence {}, path {}, reversed {}; expected sequence {expected_sequence}, path {expected_path}, reversed {expected_reversed}",
                fields[0], actual.sequence_id, actual_path, actual_reversed
            )));
        }
        steps.push(actual.lf_steps);
        checked += 1;
    }
    let locate_wall_ms = locate_started.elapsed().as_secs_f64() * 1_000.0;
    let result = stats(
        &index,
        gbwt_path,
        checked,
        max_lf_steps,
        steps,
        load_wall_ms,
        locate_wall_ms,
    )?;
    let encoded = serde_json::to_vec_pretty(&result)?;
    std::fs::write(required(values, "--stats")?, &encoded)?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::percentile;

    #[test]
    fn nearest_rank_percentiles_are_deterministic() {
        let values = (0..100).collect::<Vec<_>>();
        assert_eq!(percentile(&values, 50), 49);
        assert_eq!(percentile(&values, 90), 89);
        assert_eq!(percentile(&values, 99), 98);
        assert_eq!(percentile(&values, 100), 99);
        assert_eq!(percentile(&[], 50), 0);
    }
}
