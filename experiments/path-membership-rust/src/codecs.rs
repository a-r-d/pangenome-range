use crate::model::{GroupCodecSizes, NamedTraversalGroup, PathMembership};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

const DELTA_TAG: u8 = 0xA0;
const RUN_TAG: u8 = 0xB0;
const DENSE_TAG: u8 = 0xC0;
const ROARING_TAG: u8 = 0xD0;
const COMPLEMENT_TAG: u8 = 0xE0;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn put_varint(output: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn get_varint(input: &[u8], position: &mut usize) -> io::Result<u64> {
    let mut result = 0_u64;
    let mut shift = 0_u32;
    loop {
        let byte = *input
            .get(*position)
            .ok_or_else(|| invalid("truncated varint"))?;
        *position += 1;
        let payload = u64::from(byte & 0x7f);
        if shift >= 64 || payload > (u64::MAX >> shift) {
            return Err(invalid("varint overflow"));
        }
        result |= payload << shift;
        if byte & 0x80 == 0 {
            if shift != 0 && payload == 0 {
                return Err(invalid("non-minimal varint"));
            }
            return Ok(result);
        }
        shift += 7;
    }
}

fn finish(input: &[u8], position: usize) -> io::Result<()> {
    if position != input.len() {
        return Err(invalid("membership codec has trailing bytes"));
    }
    Ok(())
}

fn sorted(memberships: &[PathMembership]) -> Vec<PathMembership> {
    let mut result = memberships.to_vec();
    result.sort();
    result
}

pub fn encode_delta(memberships: &[PathMembership]) -> Vec<u8> {
    let memberships = sorted(memberships);
    let mut output = vec![DELTA_TAG];
    put_varint(&mut output, memberships.len() as u64);
    let mut previous = 0_u64;
    for item in memberships {
        put_varint(&mut output, item.path_id - previous);
        put_varint(&mut output, item.multiplicity);
        output.push(u8::from(item.reversed_relative_to_group));
        previous = item.path_id;
    }
    output
}

pub fn decode_delta(input: &[u8]) -> io::Result<Vec<PathMembership>> {
    if input.first() != Some(&DELTA_TAG) {
        return Err(invalid("wrong delta-varint tag"));
    }
    let mut position = 1;
    let count = usize::try_from(get_varint(input, &mut position)?)
        .map_err(|_| invalid("membership count is too large"))?;
    let mut result = Vec::with_capacity(count);
    let mut previous = 0_u64;
    for _ in 0..count {
        let path_id = previous
            .checked_add(get_varint(input, &mut position)?)
            .ok_or_else(|| invalid("path delta overflow"))?;
        let multiplicity = get_varint(input, &mut position)?;
        if multiplicity == 0 {
            return Err(invalid("zero membership multiplicity"));
        }
        let reversed = *input
            .get(position)
            .ok_or_else(|| invalid("missing orientation"))?;
        position += 1;
        if reversed > 1 {
            return Err(invalid("invalid orientation bit"));
        }
        result.push(PathMembership {
            path_id,
            multiplicity,
            reversed_relative_to_group: reversed != 0,
        });
        previous = path_id;
    }
    finish(input, position)?;
    Ok(result)
}

#[derive(Debug)]
enum RunRecord {
    Run {
        start: u64,
        length: u64,
        reverse: bool,
    },
    Single(PathMembership),
}

pub fn encode_runs(memberships: &[PathMembership]) -> Vec<u8> {
    let values = sorted(memberships);
    let mut records = Vec::new();
    let mut index = 0;
    while index < values.len() {
        let mut end = index + 1;
        if values[index].multiplicity == 1 {
            while end < values.len()
                && values[end].multiplicity == 1
                && values[end].reversed_relative_to_group
                    == values[index].reversed_relative_to_group
                && values[end].path_id == values[end - 1].path_id + 1
            {
                end += 1;
            }
        }
        if end - index >= 2 {
            records.push(RunRecord::Run {
                start: values[index].path_id,
                length: (end - index) as u64,
                reverse: values[index].reversed_relative_to_group,
            });
        } else {
            records.push(RunRecord::Single(values[index].clone()));
        }
        index = end;
    }
    let mut output = vec![RUN_TAG];
    put_varint(&mut output, values.len() as u64);
    put_varint(&mut output, records.len() as u64);
    let mut previous_end = 0_u64;
    for record in records {
        match record {
            RunRecord::Run {
                start,
                length,
                reverse,
            } => {
                output.push(1);
                put_varint(&mut output, start - previous_end);
                put_varint(&mut output, length);
                output.push(u8::from(reverse));
                previous_end = start + length - 1;
            }
            RunRecord::Single(item) => {
                output.push(0);
                put_varint(&mut output, item.path_id - previous_end);
                put_varint(&mut output, item.multiplicity);
                output.push(u8::from(item.reversed_relative_to_group));
                previous_end = item.path_id;
            }
        }
    }
    output
}

pub fn decode_runs(input: &[u8]) -> io::Result<Vec<PathMembership>> {
    if input.first() != Some(&RUN_TAG) {
        return Err(invalid("wrong interval-run tag"));
    }
    let mut position = 1;
    let expected = usize::try_from(get_varint(input, &mut position)?)
        .map_err(|_| invalid("membership count is too large"))?;
    let records = get_varint(input, &mut position)?;
    let mut result = Vec::with_capacity(expected);
    let mut previous_end = 0_u64;
    for _ in 0..records {
        let kind = *input
            .get(position)
            .ok_or_else(|| invalid("missing run kind"))?;
        position += 1;
        let path_id = previous_end
            .checked_add(get_varint(input, &mut position)?)
            .ok_or_else(|| invalid("path delta overflow"))?;
        if kind == 0 {
            let multiplicity = get_varint(input, &mut position)?;
            let reverse = *input
                .get(position)
                .ok_or_else(|| invalid("missing orientation"))?;
            position += 1;
            result.push(PathMembership {
                path_id,
                multiplicity,
                reversed_relative_to_group: reverse != 0,
            });
            previous_end = path_id;
        } else if kind == 1 {
            let length = get_varint(input, &mut position)?;
            let reverse = *input
                .get(position)
                .ok_or_else(|| invalid("missing orientation"))?;
            position += 1;
            for offset in 0..length {
                result.push(PathMembership {
                    path_id: path_id + offset,
                    multiplicity: 1,
                    reversed_relative_to_group: reverse != 0,
                });
            }
            previous_end = path_id + length - 1;
        } else {
            return Err(invalid("invalid run kind"));
        }
    }
    finish(input, position)?;
    if result.len() != expected {
        return Err(invalid("decoded interval-run count mismatch"));
    }
    Ok(result)
}

fn by_path(memberships: &[PathMembership]) -> BTreeMap<u64, Vec<PathMembership>> {
    let mut result = BTreeMap::new();
    for item in sorted(memberships) {
        result
            .entry(item.path_id)
            .or_insert_with(Vec::new)
            .push(item);
    }
    result
}

pub fn encode_dense(memberships: &[PathMembership], universe_size: u64) -> io::Result<Vec<u8>> {
    let paths = by_path(memberships);
    if paths
        .keys()
        .next_back()
        .is_some_and(|path| *path >= universe_size)
    {
        return Err(invalid("path id exceeds dense universe"));
    }
    let byte_len = usize::try_from(universe_size.div_ceil(8))
        .map_err(|_| invalid("dense universe is too large"))?;
    let mut bits = vec![0_u8; byte_len];
    let mut exceptions = Vec::new();
    for (&path_id, entries) in &paths {
        let index = usize::try_from(path_id).map_err(|_| invalid("path id is too large"))?;
        bits[index / 8] |= 1 << (index % 8);
        if entries.len() != 1
            || entries[0].multiplicity != 1
            || entries[0].reversed_relative_to_group
        {
            exceptions.push((path_id, entries));
        }
    }
    let mut output = vec![DENSE_TAG];
    put_varint(&mut output, universe_size);
    output.extend_from_slice(&bits);
    put_varint(&mut output, exceptions.len() as u64);
    let mut previous = 0_u64;
    for (path_id, entries) in exceptions {
        put_varint(&mut output, path_id - previous);
        put_varint(&mut output, entries.len() as u64);
        for entry in entries {
            put_varint(&mut output, entry.multiplicity);
            output.push(u8::from(entry.reversed_relative_to_group));
        }
        previous = path_id;
    }
    Ok(output)
}

pub fn decode_dense(input: &[u8]) -> io::Result<Vec<PathMembership>> {
    if input.first() != Some(&DENSE_TAG) {
        return Err(invalid("wrong dense tag"));
    }
    let mut position = 1;
    let universe = get_varint(input, &mut position)?;
    let byte_len = usize::try_from(universe.div_ceil(8))
        .map_err(|_| invalid("dense universe is too large"))?;
    let bits = input
        .get(position..position + byte_len)
        .ok_or_else(|| invalid("truncated dense bitset"))?;
    position += byte_len;
    let exception_count = get_varint(input, &mut position)?;
    let mut exceptions = BTreeMap::new();
    let mut previous = 0_u64;
    for _ in 0..exception_count {
        let path_id = previous + get_varint(input, &mut position)?;
        let count = get_varint(input, &mut position)?;
        let mut entries = Vec::new();
        for _ in 0..count {
            let multiplicity = get_varint(input, &mut position)?;
            let reverse = *input
                .get(position)
                .ok_or_else(|| invalid("missing dense exception orientation"))?;
            position += 1;
            entries.push(PathMembership {
                path_id,
                multiplicity,
                reversed_relative_to_group: reverse != 0,
            });
        }
        exceptions.insert(path_id, entries);
        previous = path_id;
    }
    finish(input, position)?;
    let mut result = Vec::new();
    for path_id in 0..universe {
        let index = usize::try_from(path_id).unwrap();
        if bits[index / 8] & (1 << (index % 8)) == 0 {
            continue;
        }
        if let Some(entries) = exceptions.remove(&path_id) {
            result.extend(entries);
        } else {
            result.push(PathMembership {
                path_id,
                multiplicity: 1,
                reversed_relative_to_group: false,
            });
        }
    }
    if !exceptions.is_empty() {
        return Err(invalid("dense exception refers to an absent bit"));
    }
    Ok(result)
}

pub fn encode_roaring(memberships: &[PathMembership]) -> Vec<u8> {
    let mut blocks = BTreeMap::<u64, Vec<PathMembership>>::new();
    for item in sorted(memberships) {
        blocks.entry(item.path_id >> 16).or_default().push(item);
    }
    let mut output = vec![ROARING_TAG];
    put_varint(&mut output, blocks.len() as u64);
    let mut previous_high = 0_u64;
    for (high, entries) in blocks {
        put_varint(&mut output, high - previous_high);
        output.push(0); // Array block; deterministic and browser-trivial.
        put_varint(&mut output, entries.len() as u64);
        for entry in entries {
            let low = (entry.path_id & 0xffff) as u16;
            output.extend_from_slice(&low.to_le_bytes());
            put_varint(&mut output, entry.multiplicity);
            output.push(u8::from(entry.reversed_relative_to_group));
        }
        previous_high = high;
    }
    output
}

pub fn decode_roaring(input: &[u8]) -> io::Result<Vec<PathMembership>> {
    if input.first() != Some(&ROARING_TAG) {
        return Err(invalid("wrong roaring-block tag"));
    }
    let mut position = 1;
    let block_count = get_varint(input, &mut position)?;
    let mut previous_high = 0_u64;
    let mut result = Vec::new();
    for _ in 0..block_count {
        let high = previous_high + get_varint(input, &mut position)?;
        let kind = *input
            .get(position)
            .ok_or_else(|| invalid("missing roaring block kind"))?;
        position += 1;
        if kind != 0 {
            return Err(invalid("unsupported roaring block kind"));
        }
        let count = get_varint(input, &mut position)?;
        for _ in 0..count {
            let low_bytes: [u8; 2] = input
                .get(position..position + 2)
                .ok_or_else(|| invalid("truncated roaring low id"))?
                .try_into()
                .unwrap();
            position += 2;
            let path_id = (high << 16) | u64::from(u16::from_le_bytes(low_bytes));
            let multiplicity = get_varint(input, &mut position)?;
            let reverse = *input
                .get(position)
                .ok_or_else(|| invalid("missing roaring orientation"))?;
            position += 1;
            result.push(PathMembership {
                path_id,
                multiplicity,
                reversed_relative_to_group: reverse != 0,
            });
        }
        previous_high = high;
    }
    finish(input, position)?;
    Ok(result)
}

pub fn encode_complement(memberships: &[PathMembership], universe: &[u64]) -> io::Result<Vec<u8>> {
    let memberships = sorted(memberships);
    let universe_set = universe.iter().copied().collect::<BTreeSet<_>>();
    let member_set = memberships
        .iter()
        .map(|item| item.path_id)
        .collect::<BTreeSet<_>>();
    if memberships.iter().any(|item| item.multiplicity != 1)
        || member_set.len() != memberships.len()
        || !member_set.is_subset(&universe_set)
    {
        return Err(invalid("complement preconditions do not hold"));
    }
    let missing = universe_set
        .difference(&member_set)
        .copied()
        .collect::<Vec<_>>();
    let mut output = vec![COMPLEMENT_TAG];
    put_varint(&mut output, missing.len() as u64);
    let mut previous = 0_u64;
    for path_id in missing {
        put_varint(&mut output, path_id - previous);
        previous = path_id;
    }
    let orientation_bytes = memberships.len().div_ceil(8);
    put_varint(&mut output, orientation_bytes as u64);
    let start = output.len();
    output.resize(start + orientation_bytes, 0);
    for (index, item) in memberships.iter().enumerate() {
        if item.reversed_relative_to_group {
            output[start + index / 8] |= 1 << (index % 8);
        }
    }
    Ok(output)
}

pub fn decode_complement(input: &[u8], universe: &[u64]) -> io::Result<Vec<PathMembership>> {
    if input.first() != Some(&COMPLEMENT_TAG) {
        return Err(invalid("wrong complement tag"));
    }
    let mut position = 1;
    let missing_count = get_varint(input, &mut position)?;
    let mut missing = BTreeSet::new();
    let mut previous = 0_u64;
    for _ in 0..missing_count {
        let path_id = previous + get_varint(input, &mut position)?;
        missing.insert(path_id);
        previous = path_id;
    }
    let carriers = universe
        .iter()
        .copied()
        .filter(|path| !missing.contains(path))
        .collect::<Vec<_>>();
    let orientation_len = usize::try_from(get_varint(input, &mut position)?)
        .map_err(|_| invalid("orientation bitset is too large"))?;
    if orientation_len != carriers.len().div_ceil(8) {
        return Err(invalid("complement orientation bitset has wrong length"));
    }
    let orientations = input
        .get(position..position + orientation_len)
        .ok_or_else(|| invalid("truncated complement orientation bits"))?;
    position += orientation_len;
    finish(input, position)?;
    Ok(carriers
        .into_iter()
        .enumerate()
        .map(|(index, path_id)| PathMembership {
            path_id,
            multiplicity: 1,
            reversed_relative_to_group: orientations[index / 8] & (1 << (index % 8)) != 0,
        })
        .collect())
}

pub fn measure_group(
    group: &NamedTraversalGroup,
    global_universe_size: u64,
    complement_universe: Option<&[u64]>,
) -> io::Result<GroupCodecSizes> {
    let expected = sorted(&group.memberships);
    let delta = encode_delta(&expected);
    let runs = encode_runs(&expected);
    let dense = encode_dense(&expected, global_universe_size)?;
    let roaring = encode_roaring(&expected);
    if decode_delta(&delta)? != expected
        || decode_runs(&runs)? != expected
        || decode_dense(&dense)? != expected
        || decode_roaring(&roaring)? != expected
    {
        return Err(invalid("membership codec round trip failed"));
    }
    let complement = complement_universe
        .map(|universe| {
            let encoded = encode_complement(&expected, universe)?;
            if decode_complement(&encoded, universe)? != expected {
                return Err(invalid("complement codec round trip failed"));
            }
            Ok(encoded)
        })
        .transpose()?;
    let mut choices = vec![
        ("delta-varint", delta.len()),
        ("interval-run", runs.len()),
        ("dense-bitset", dense.len()),
        ("roaring-blocks", roaring.len()),
    ];
    if let Some(value) = &complement {
        choices.push(("complement", value.len()));
    }
    choices.sort_by_key(|(name, bytes)| (*bytes, *name));
    Ok(GroupCodecSizes {
        delta_varint: delta.len() as u64,
        interval_run: runs.len() as u64,
        dense_bitset: dense.len() as u64,
        roaring_blocks: roaring.len() as u64,
        complement: complement.map(|value| value.len() as u64),
        adaptive_codec: choices[0].0.to_string(),
        adaptive_bytes: choices[0].1 as u64 + 1, // Deterministic hybrid codec tag.
    })
}

pub fn encode_adaptive(
    group: &NamedTraversalGroup,
    global_universe_size: u64,
    complement_universe: Option<&[u64]>,
) -> io::Result<Vec<u8>> {
    let expected = sorted(&group.memberships);
    let mut choices = vec![
        (0_u8, encode_delta(&expected)),
        (1_u8, encode_runs(&expected)),
        (2_u8, encode_dense(&expected, global_universe_size)?),
        (3_u8, encode_roaring(&expected)),
    ];
    if let Some(universe) = complement_universe {
        choices.push((4_u8, encode_complement(&expected, universe)?));
    }
    choices.sort_by_key(|(tag, bytes)| (bytes.len(), *tag));
    let (tag, bytes) = choices.remove(0);
    let mut result = Vec::with_capacity(bytes.len() + 1);
    result.push(tag);
    result.extend_from_slice(&bytes);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Vec<PathMembership> {
        vec![
            PathMembership {
                path_id: 1,
                multiplicity: 1,
                reversed_relative_to_group: false,
            },
            PathMembership {
                path_id: 2,
                multiplicity: 3,
                reversed_relative_to_group: false,
            },
            PathMembership {
                path_id: 2,
                multiplicity: 1,
                reversed_relative_to_group: true,
            },
            PathMembership {
                path_id: 70_000,
                multiplicity: 1,
                reversed_relative_to_group: true,
            },
        ]
    }

    #[test]
    fn general_codecs_are_exact() {
        let expected = sorted(&example());
        assert_eq!(decode_delta(&encode_delta(&expected)).unwrap(), expected);
        assert_eq!(decode_runs(&encode_runs(&expected)).unwrap(), expected);
        assert_eq!(
            decode_dense(&encode_dense(&expected, 70_001).unwrap()).unwrap(),
            expected
        );
        assert_eq!(
            decode_roaring(&encode_roaring(&expected)).unwrap(),
            expected
        );
    }

    #[test]
    fn complement_is_exact_when_preconditions_hold() {
        let memberships = vec![
            PathMembership {
                path_id: 1,
                multiplicity: 1,
                reversed_relative_to_group: false,
            },
            PathMembership {
                path_id: 3,
                multiplicity: 1,
                reversed_relative_to_group: true,
            },
        ];
        let universe = vec![1, 2, 3, 4];
        let encoded = encode_complement(&memberships, &universe).unwrap();
        assert_eq!(decode_complement(&encoded, &universe).unwrap(), memberships);
    }
}
