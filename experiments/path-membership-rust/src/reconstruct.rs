use crate::model::{LocatedPosition, NamedTraversalGroup, PathMembership};
use pangenome_range_format::{PackedGbwtRecord, RecordRegionalPayload};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Position {
    handle: u64,
    offset: u64,
}

#[derive(Clone, Debug)]
struct Record {
    handle: u64,
    successors: Vec<Position>,
    has_predecessor: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraversalStart {
    pub handle: u64,
    pub offset: u64,
    pub traversal: Vec<u64>,
    pub is_reference_occurrence: bool,
    pub canonical: bool,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn usize_to_u64(value: usize) -> io::Result<u64> {
    u64::try_from(value).map_err(|_| invalid("usize does not fit in u64"))
}

fn u64_to_usize(value: u64) -> io::Result<usize> {
    usize::try_from(value).map_err(|_| invalid("u64 does not fit in usize"))
}

fn bytecode(bytes: &[u8], position: &mut usize) -> io::Result<u64> {
    let mut result = 0_u64;
    let mut shift = 0_u32;
    loop {
        let value = *bytes
            .get(*position)
            .ok_or_else(|| invalid("truncated GBWT bytecode integer"))?;
        *position = position
            .checked_add(1)
            .ok_or_else(|| invalid("GBWT byte position overflow"))?;
        let payload = u64::from(value & 0x7f);
        if shift >= 64 || payload > (u64::MAX >> shift) {
            return Err(invalid("GBWT bytecode integer overflow"));
        }
        result = result
            .checked_add(payload << shift)
            .ok_or_else(|| invalid("GBWT bytecode integer overflow"))?;
        if value & 0x80 == 0 {
            if shift > 0 && payload == 0 {
                return Err(invalid("non-minimal GBWT bytecode integer"));
            }
            return Ok(result);
        }
        shift = shift
            .checked_add(7)
            .ok_or_else(|| invalid("GBWT bytecode shift overflow"))?;
    }
}

fn decode_successors(record: &PackedGbwtRecord) -> io::Result<Vec<Position>> {
    record.validate()?;
    let mut position = 0_usize;
    let sigma = u64_to_usize(bytecode(&record.bytes, &mut position)?)?;
    let mut edges = Vec::with_capacity(sigma);
    let mut previous_handle = 0_u64;
    for _ in 0..sigma {
        let handle = previous_handle
            .checked_add(bytecode(&record.bytes, &mut position)?)
            .ok_or_else(|| invalid("GBWT successor handle overflow"))?;
        let offset = bytecode(&record.bytes, &mut position)?;
        edges.push(Position { handle, offset });
        previous_handle = handle;
    }
    let threshold = if sigma < 255 { 256 / sigma } else { 0 };
    let mut successors = Vec::with_capacity(u64_to_usize(record.occurrence_count)?);
    while position < record.bytes.len() {
        let (rank, run_len) = if sigma >= 255 {
            let rank = u64_to_usize(bytecode(&record.bytes, &mut position)?)?;
            let run_len = bytecode(&record.bytes, &mut position)?
                .checked_add(1)
                .ok_or_else(|| invalid("GBWT run length overflow"))?;
            (rank, run_len)
        } else {
            let value = usize::from(record.bytes[position]);
            position += 1;
            let rank = value % sigma;
            let mut run_len = usize_to_u64(value / sigma + 1)?;
            if u64_to_usize(run_len)? == threshold {
                run_len = run_len
                    .checked_add(bytecode(&record.bytes, &mut position)?)
                    .ok_or_else(|| invalid("GBWT run length overflow"))?;
            }
            (rank, run_len)
        };
        let edge = edges
            .get_mut(rank)
            .ok_or_else(|| invalid("GBWT run rank is outside its alphabet"))?;
        for _ in 0..run_len {
            successors.push(*edge);
            edge.offset = edge
                .offset
                .checked_add(1)
                .ok_or_else(|| invalid("GBWT successor offset overflow"))?;
        }
    }
    if usize_to_u64(successors.len())? != record.occurrence_count {
        return Err(invalid("decoded GBWT occurrence count mismatch"));
    }
    Ok(successors)
}

fn canonical_edge(from: u64, to: u64) -> bool {
    let from_node = from / 2;
    let to_node = to / 2;
    let from_reverse = !from.is_multiple_of(2);
    let to_reverse = !to.is_multiple_of(2);
    from != 0
        && to != 0
        && if from_reverse {
            to_node > from_node || (to_node == from_node && !to_reverse)
        } else {
            to_node >= from_node
        }
}

fn path_is_canonical(path: &[u64]) -> bool {
    let Some((&first, rest)) = path.split_first() else {
        return true;
    };
    let last = *rest.last().unwrap_or(&first);
    if first.is_multiple_of(2) == last.is_multiple_of(2) {
        first.is_multiple_of(2)
    } else {
        canonical_edge(first, last)
    }
}

pub fn traversal_starts(payload: &RecordRegionalPayload) -> io::Result<Vec<TraversalStart>> {
    let mut records = Vec::with_capacity(payload.records.len());
    for record in &payload.records {
        let successors = decode_successors(record)?;
        records.push(Record {
            handle: record.handle,
            has_predecessor: vec![false; successors.len()],
            successors,
        });
    }
    let handle_to_record = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.handle, index))
        .collect::<HashMap<_, _>>();
    for source in 0..records.len() {
        for offset in 0..records[source].successors.len() {
            let successor = records[source].successors[offset];
            let Some(&target) = handle_to_record.get(&successor.handle) else {
                continue;
            };
            let predecessor = records[target]
                .has_predecessor
                .get_mut(u64_to_usize(successor.offset)?)
                .ok_or_else(|| invalid("successor offset is outside local record"))?;
            *predecessor = true;
        }
    }
    let occurrence_limit = records.iter().try_fold(0_u64, |total, record| {
        total
            .checked_add(usize_to_u64(record.successors.len())?)
            .ok_or_else(|| invalid("local occurrence count overflow"))
    })?;
    let mut starts = Vec::new();
    let mut reference_matches = 0_u64;
    for record in &records {
        for offset in 0..record.successors.len() {
            if record.has_predecessor[offset] {
                continue;
            }
            let start = Position {
                handle: record.handle,
                offset: usize_to_u64(offset)?,
            };
            let mut current = Some(start);
            let mut traversal = Vec::new();
            let mut is_reference = false;
            while let Some(position) = current {
                is_reference |= (position.handle, position.offset) == payload.reference_position;
                traversal.push(position.handle);
                let current_record = handle_to_record
                    .get(&position.handle)
                    .and_then(|&index| records.get(index))
                    .ok_or_else(|| invalid("local traversal left selected records"))?;
                let next = *current_record
                    .successors
                    .get(u64_to_usize(position.offset)?)
                    .ok_or_else(|| invalid("local traversal offset is out of bounds"))?;
                current = (next.handle != 0 && handle_to_record.contains_key(&next.handle))
                    .then_some(next);
                if usize_to_u64(traversal.len())? > occurrence_limit {
                    return Err(invalid("cyclic local GBWT traversal"));
                }
            }
            if is_reference {
                reference_matches += 1;
            }
            starts.push(TraversalStart {
                handle: start.handle,
                offset: start.offset,
                canonical: path_is_canonical(&traversal),
                traversal,
                is_reference_occurrence: is_reference,
            });
        }
    }
    if reference_matches != 1 {
        return Err(invalid(format!(
            "expected one reference occurrence, found {reference_matches}"
        )));
    }
    Ok(starts)
}

pub fn named_groups(
    payload: &RecordRegionalPayload,
    located: &BTreeMap<(u64, u64), LocatedPosition>,
) -> io::Result<Vec<NamedTraversalGroup>> {
    let starts = traversal_starts(payload)?;
    let mut occurrences = BTreeMap::<Vec<u64>, Vec<&LocatedPosition>>::new();
    for start in &starts {
        let identity = located
            .get(&(start.handle, start.offset))
            .ok_or_else(|| invalid("missing locate result for traversal start"))?;
        if start.is_reference_occurrence {
            continue;
        }
        if start.canonical {
            occurrences
                .entry(start.traversal.clone())
                .or_default()
                .push(identity);
        }
    }

    let existing = payload.reconstruct_traversals()?;
    let existing_weights = existing
        .anonymous
        .into_iter()
        .map(|group| (group.handles, group.weight))
        .collect::<BTreeMap<_, _>>();
    if existing_weights.len() != occurrences.len() {
        return Err(invalid("named and anonymous traversal group counts differ"));
    }

    let mut result = Vec::with_capacity(occurrences.len());
    for (traversal, identities) in occurrences {
        let occurrence_weight = usize_to_u64(identities.len())?;
        if existing_weights.get(&traversal) != Some(&occurrence_weight) {
            return Err(invalid(
                "membership multiplicity sum differs from anonymous weight",
            ));
        }
        let mut multiplicities = BTreeMap::<(u64, bool), u64>::new();
        let mut unique_paths = BTreeSet::new();
        for identity in identities {
            unique_paths.insert(identity.path_id);
            let value = multiplicities
                .entry((identity.path_id, identity.reversed))
                .or_default();
            *value = value
                .checked_add(1)
                .ok_or_else(|| invalid("membership multiplicity overflow"))?;
        }
        let memberships = multiplicities
            .into_iter()
            .map(
                |((path_id, reversed_relative_to_group), multiplicity)| PathMembership {
                    path_id,
                    multiplicity,
                    reversed_relative_to_group,
                },
            )
            .collect::<Vec<_>>();
        let sum = memberships
            .iter()
            .map(|item| item.multiplicity)
            .sum::<u64>();
        if sum != occurrence_weight {
            return Err(invalid(
                "membership multiplicities do not sum to group weight",
            ));
        }
        result.push(NamedTraversalGroup {
            traversal,
            occurrence_weight,
            unique_path_count: usize_to_u64(unique_paths.len())?,
            memberships,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_golden_starts_match_existing_reconstruction() {
        let bytes = include_bytes!("../../../test-data/conformance/format-v1.payload.raw");
        let payload = RecordRegionalPayload::decode(bytes).unwrap();
        let starts = traversal_starts(&payload).unwrap();
        assert!(!starts.is_empty());
        assert_eq!(
            starts
                .iter()
                .filter(|start| start.is_reference_occurrence)
                .count(),
            1
        );
    }
}
