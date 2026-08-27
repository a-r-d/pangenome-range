use crate::binary::{
    BinaryReader, count_bounded_by_bytes, invalid_data, put_bytes, put_string, put_u32, put_u64,
    u64_to_usize, usize_to_u64,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;

pub const REGION_MAGIC: &[u8; 8] = b"PNGRGN01";
pub const REGION_VERSION: u32 = 1;
const HAPLOTYPE_SEMANTICS: u8 = 2;
pub const CONSTRUCTION_CONTEXT: u64 = 100;
pub const MAX_DECODED_OCCURRENCES_PER_TILE: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackedEdge {
    pub from: u64,
    pub to: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedGbwtRecord {
    pub handle: u64,
    pub occurrence_count: u64,
    pub bytes: Vec<u8>,
}

impl PackedGbwtRecord {
    /// Validates the packed successor alphabet and run-length stream without
    /// allocating the decoded occurrence list.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid occurrence bounds, bytecode, successor
    /// ordering/offsets, or run totals.
    pub fn validate(&self) -> io::Result<()> {
        validate_packed_record(self, false).map(drop)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordRegionalPayload {
    pub core_start: u64,
    pub core_end: u64,
    pub context: u64,
    pub reference_sample: String,
    pub reference_contig: String,
    pub reference_haplotype: u64,
    pub reference_fragment_start: u64,
    pub reference_query_offset: u64,
    pub reference_node_offset: u64,
    pub reference_position: (u64, u64),
    pub nodes: BTreeMap<u64, Vec<u8>>,
    pub edges: BTreeSet<PackedEdge>,
    pub records: Vec<PackedGbwtRecord>,
    pub total_occurrences: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegionalWeightedTraversal {
    pub weight: u64,
    pub handles: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconstructedTraversals {
    pub reference_handles: Vec<u64>,
    pub reference_start: u64,
    pub reference_end: u64,
    pub anonymous: Vec<RegionalWeightedTraversal>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Position {
    handle: u64,
    offset: u64,
}

#[derive(Debug)]
struct DecodedRecord {
    handle: u64,
    successors: Vec<Position>,
    has_predecessor: Vec<bool>,
    sequence_len: u64,
}

impl RecordRegionalPayload {
    /// Encodes one complete regional payload in canonical v1 order.
    ///
    /// # Errors
    ///
    /// Returns an error when provenance, counts, ordering, occurrence bounds,
    /// handles, records, or integer lengths violate the v1 contract.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        if self.total_occurrences == 0 || self.total_occurrences > MAX_DECODED_OCCURRENCES_PER_TILE
        {
            return Err(invalid_data(
                "record payload occurrence total exceeds its safety bound",
            ));
        }
        let mut output = Vec::new();
        output.extend_from_slice(REGION_MAGIC);
        put_u32(&mut output, REGION_VERSION);
        put_u32(&mut output, 1);
        output.push(HAPLOTYPE_SEMANTICS);
        output.extend_from_slice(&[0_u8; 7]);
        put_u64(&mut output, usize_to_u64(self.nodes.len())?);
        put_u64(&mut output, usize_to_u64(self.edges.len())?);
        put_u64(&mut output, usize_to_u64(self.records.len())?);
        put_u64(&mut output, self.total_occurrences);
        put_u64(&mut output, self.core_start);
        put_u64(&mut output, self.core_end);
        put_u64(&mut output, self.context);
        put_u64(&mut output, self.reference_haplotype);
        put_u64(&mut output, self.reference_fragment_start);
        put_u64(&mut output, self.reference_query_offset);
        put_u64(&mut output, self.reference_node_offset);
        put_u64(&mut output, self.reference_position.0);
        put_u64(&mut output, self.reference_position.1);
        put_string(&mut output, &self.reference_sample)?;
        put_string(&mut output, &self.reference_contig)?;

        let mut previous_node_id = 0_u64;
        for (node_id, sequence) in &self.nodes {
            if *node_id == 0 || *node_id <= previous_node_id || sequence.is_empty() {
                return Err(invalid_data(
                    "record payload nodes are not strictly sorted or contain an empty sequence",
                ));
            }
            put_u64(
                &mut output,
                node_id
                    .checked_sub(previous_node_id)
                    .ok_or_else(|| invalid_data("record payload nodes are not sorted"))?,
            );
            put_bytes(&mut output, sequence)?;
            previous_node_id = *node_id;
        }
        for edge in &self.edges {
            if !canonical_edge(edge.from, edge.to) || !self.nodes.contains_key(&(edge.from / 2)) {
                return Err(invalid_data(
                    "record payload edge is not canonical or local",
                ));
            }
            put_u64(&mut output, edge.from);
            put_u64(&mut output, edge.to);
        }
        let mut previous_handle = None;
        let mut total_occurrences = 0_u64;
        for record in &self.records {
            if record.handle == 0
                || previous_handle.is_some_and(|handle| record.handle <= handle)
                || !self.nodes.contains_key(&(record.handle / 2))
            {
                return Err(invalid_data(
                    "record payload handles are not strictly sorted or local",
                ));
            }
            validate_packed_record(record, false)?;
            previous_handle = Some(record.handle);
            total_occurrences = total_occurrences
                .checked_add(record.occurrence_count)
                .ok_or_else(|| invalid_data("record payload occurrence count overflow"))?;
            put_u64(&mut output, record.handle);
            put_u64(&mut output, record.occurrence_count);
            put_bytes(&mut output, &record.bytes)?;
        }
        if total_occurrences != self.total_occurrences {
            return Err(invalid_data(
                "record payload occurrence total differs from records",
            ));
        }
        Ok(output)
    }

    #[allow(clippy::too_many_lines)]
    /// Decodes and structurally validates one complete regional payload.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed identity/version, invalid counts or
    /// allocation bounds, truncation, ordering, packed-record corruption, or
    /// trailing bytes.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut reader = BinaryReader::new(bytes);
        if reader.take(8)? != REGION_MAGIC {
            return Err(invalid_data("invalid record regional chunk magic"));
        }
        let version = reader.u32()?;
        if version != REGION_VERSION {
            return Err(invalid_data(format!(
                "unsupported regional version {version}"
            )));
        }
        if reader.u32()? != 1 {
            return Err(invalid_data("unsupported record regional flags"));
        }
        if reader.u8()? != HAPLOTYPE_SEMANTICS {
            return Err(invalid_data(
                "record payload must use distinct weighted semantics",
            ));
        }
        if reader.take(7)? != [0_u8; 7] {
            return Err(invalid_data("record payload reserved bytes are nonzero"));
        }
        let node_count = reader.u64()?;
        let edge_count = reader.u64()?;
        let record_count = reader.u64()?;
        let total_occurrences = reader.u64()?;
        let core_start = reader.u64()?;
        let core_end = reader.u64()?;
        let context = reader.u64()?;
        let reference_haplotype = reader.u64()?;
        let reference_fragment_start = reader.u64()?;
        let reference_query_offset = reader.u64()?;
        let reference_node_offset = reader.u64()?;
        let reference_position = (reader.u64()?, reader.u64()?);
        if core_start >= core_end {
            return Err(invalid_data("record payload has an invalid core interval"));
        }
        if context != CONSTRUCTION_CONTEXT {
            return Err(invalid_data(format!(
                "unsupported record payload construction context {context}"
            )));
        }
        if reference_fragment_start.checked_add(reference_query_offset) != Some(core_start) {
            return Err(invalid_data(
                "record payload query offset does not match core start",
            ));
        }
        if total_occurrences == 0 || total_occurrences > MAX_DECODED_OCCURRENCES_PER_TILE {
            return Err(invalid_data(
                "record payload occurrence total exceeds its safety bound",
            ));
        }
        if record_count
            != node_count
                .checked_mul(2)
                .ok_or_else(|| invalid_data("record count overflow"))?
        {
            return Err(invalid_data(
                "record payload must contain both handles for every node",
            ));
        }
        let reference_sample = reader.string()?;
        let reference_contig = reader.string()?;
        if reference_sample.is_empty() || reference_contig.is_empty() {
            return Err(invalid_data("record payload reference provenance is empty"));
        }

        let mut nodes = BTreeMap::new();
        let mut previous_node_id = 0_u64;
        for _ in
            0..count_bounded_by_bytes(node_count, reader.remaining(), 16, "record payload nodes")?
        {
            let node_id = previous_node_id
                .checked_add(reader.u64()?)
                .ok_or_else(|| invalid_data("record payload node delta overflow"))?;
            if node_id == 0 || node_id <= previous_node_id {
                return Err(invalid_data("record payload nodes are not strictly sorted"));
            }
            let sequence = reader.bytes()?;
            if sequence.is_empty() {
                return Err(invalid_data(
                    "record payload contains an empty node sequence",
                ));
            }
            nodes.insert(node_id, sequence);
            previous_node_id = node_id;
        }

        let mut edges = BTreeSet::new();
        let mut previous_edge = None;
        for _ in
            0..count_bounded_by_bytes(edge_count, reader.remaining(), 16, "record payload edges")?
        {
            let edge = PackedEdge {
                from: reader.u64()?,
                to: reader.u64()?,
            };
            if !nodes.contains_key(&(edge.from / 2)) {
                return Err(invalid_data("record payload edge source is not local"));
            }
            if !canonical_edge(edge.from, edge.to) {
                return Err(invalid_data("record payload edge is not canonical"));
            }
            if previous_edge.is_some_and(|previous| edge <= previous) {
                return Err(invalid_data(
                    "record payload edges are not strictly ordered",
                ));
            }
            previous_edge = Some(edge);
            edges.insert(edge);
        }

        let capacity = count_bounded_by_bytes(
            record_count,
            reader.remaining(),
            24,
            "record payload GBWT records",
        )?;
        let mut records = Vec::with_capacity(capacity);
        let mut previous_handle = None;
        let mut decoded_occurrences = 0_u64;
        for _ in 0..record_count {
            let record = PackedGbwtRecord {
                handle: reader.u64()?,
                occurrence_count: reader.u64()?,
                bytes: reader.bytes()?,
            };
            if record.handle == 0 || previous_handle.is_some_and(|handle| record.handle <= handle) {
                return Err(invalid_data(
                    "record payload handles are not strictly sorted",
                ));
            }
            if !nodes.contains_key(&(record.handle / 2)) {
                return Err(invalid_data(
                    "record payload handle refers to an absent node",
                ));
            }
            validate_packed_record(&record, false)?;
            decoded_occurrences = decoded_occurrences
                .checked_add(record.occurrence_count)
                .ok_or_else(|| invalid_data("record payload occurrence total overflow"))?;
            previous_handle = Some(record.handle);
            records.push(record);
        }
        reader.finish()?;
        if decoded_occurrences != total_occurrences {
            return Err(invalid_data(
                "record payload occurrence total differs from records",
            ));
        }
        let reference_record = records
            .binary_search_by_key(&reference_position.0, |record| record.handle)
            .ok()
            .and_then(|index| records.get(index))
            .ok_or_else(|| invalid_data("record payload reference handle is not local"))?;
        if reference_position.1 >= reference_record.occurrence_count {
            return Err(invalid_data(
                "record payload reference offset is out of bounds",
            ));
        }
        let reference_node = nodes
            .get(&(reference_position.0 / 2))
            .ok_or_else(|| invalid_data("record payload reference node is absent"))?;
        if reference_node_offset >= usize_to_u64(reference_node.len())? {
            return Err(invalid_data(
                "record payload reference node offset is out of bounds",
            ));
        }
        Ok(Self {
            core_start,
            core_end,
            context,
            reference_sample,
            reference_contig,
            reference_haplotype,
            reference_fragment_start,
            reference_query_offset,
            reference_node_offset,
            reference_position,
            nodes,
            edges,
            records,
            total_occurrences,
        })
    }

    #[allow(clippy::too_many_lines)]
    /// Reconstructs the anchored reference and weighted anonymous local paths.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid local positions, cycles, missing or
    /// ambiguous reference anchors, coordinate overflow, or weight overflow.
    pub fn reconstruct_traversals(&self) -> io::Result<ReconstructedTraversals> {
        let mut records = Vec::with_capacity(self.records.len());
        for record in &self.records {
            let successors = validate_packed_record(record, true)?;
            let sequence_len = self
                .nodes
                .get(&(record.handle / 2))
                .ok_or_else(|| invalid_data("record payload handle has no local sequence"))?
                .len();
            records.push(DecodedRecord {
                handle: record.handle,
                has_predecessor: vec![false; successors.len()],
                successors,
                sequence_len: usize_to_u64(sequence_len)?,
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
                    .ok_or_else(|| {
                        invalid_data("GBWT successor offset is outside the local record")
                    })?;
                *predecessor = true;
            }
        }
        let local_occurrences = records.iter().try_fold(0_u64, |total, record| {
            total
                .checked_add(usize_to_u64(record.successors.len())?)
                .ok_or_else(|| invalid_data("local GBWT occurrence count overflow"))
        })?;
        let mut paths = Vec::<Vec<u64>>::new();
        let mut reference_path = None::<Vec<u64>>;
        let mut reference_path_offset = None::<usize>;
        for record in &records {
            for offset in 0..record.successors.len() {
                if record.has_predecessor[offset] {
                    continue;
                }
                let mut position = Some(Position {
                    handle: record.handle,
                    offset: usize_to_u64(offset)?,
                });
                let mut path = Vec::new();
                let mut matched_reference = None;
                while let Some(current) = position {
                    if (current.handle, current.offset) == self.reference_position {
                        matched_reference = Some(path.len());
                    }
                    path.push(current.handle);
                    let current_record = handle_to_record
                        .get(&current.handle)
                        .and_then(|&index| records.get(index))
                        .ok_or_else(|| {
                            invalid_data("local GBWT traversal left the selected records")
                        })?;
                    let next = *current_record
                        .successors
                        .get(u64_to_usize(current.offset)?)
                        .ok_or_else(|| {
                            invalid_data("local GBWT traversal offset is out of bounds")
                        })?;
                    position = (next.handle != 0 && handle_to_record.contains_key(&next.handle))
                        .then_some(next);
                    if usize_to_u64(path.len())? > local_occurrences {
                        return Err(invalid_data("cyclic local GBWT traversal"));
                    }
                }
                if let Some(offset) = matched_reference {
                    reference_path_offset = Some(offset);
                    reference_path = Some(path.clone());
                    paths.push(path);
                } else if encoded_path_is_canonical(&path) {
                    paths.push(path);
                }
            }
        }
        let reference_path = reference_path
            .ok_or_else(|| invalid_data("could not find the reference path in record payload"))?;
        let reference_path_offset = reference_path_offset.ok_or_else(|| {
            invalid_data("record payload reference has no matching GBWT position")
        })?;
        let reference_len = reference_path.iter().try_fold(0_u64, |total, handle| {
            let sequence_len = handle_to_record
                .get(handle)
                .and_then(|&index| records.get(index))
                .map(|record| record.sequence_len)
                .ok_or_else(|| invalid_data("reference traversal handle is not local"))?;
            total
                .checked_add(sequence_len)
                .ok_or_else(|| invalid_data("reference traversal length overflow"))
        })?;
        let prefix_len = reference_path.iter().take(reference_path_offset).try_fold(
            self.reference_node_offset,
            |total, handle| {
                let sequence_len = handle_to_record
                    .get(handle)
                    .and_then(|&index| records.get(index))
                    .map(|record| record.sequence_len)
                    .ok_or_else(|| invalid_data("reference prefix handle is not local"))?;
                total
                    .checked_add(sequence_len)
                    .ok_or_else(|| invalid_data("reference prefix length overflow"))
            },
        )?;
        let relative_start = self
            .reference_query_offset
            .checked_sub(prefix_len)
            .ok_or_else(|| invalid_data("reference context starts before its fragment"))?;
        let reference_start = self
            .reference_fragment_start
            .checked_add(relative_start)
            .ok_or_else(|| invalid_data("reference interval start overflow"))?;
        let reference_end = reference_start
            .checked_add(reference_len)
            .ok_or_else(|| invalid_data("reference interval end overflow"))?;

        paths.sort_unstable();
        let mut anonymous = Vec::new();
        let mut index = 0;
        while index < paths.len() {
            let mut end = index + 1;
            while end < paths.len() && paths[end] == paths[index] {
                end += 1;
            }
            let count = usize_to_u64(end - index)?;
            let anonymous_weight = if paths[index] == reference_path {
                count.saturating_sub(1)
            } else {
                count
            };
            if anonymous_weight > 0 {
                anonymous.push(RegionalWeightedTraversal {
                    weight: anonymous_weight,
                    handles: paths[index].clone(),
                });
            }
            index = end;
        }
        anonymous.sort();
        Ok(ReconstructedTraversals {
            reference_handles: reference_path,
            reference_start,
            reference_end,
            anonymous,
        })
    }
}

fn decode_bytecode_integer(bytes: &[u8], position: &mut usize) -> io::Result<u64> {
    let mut result = 0_u64;
    let mut shift = 0_u32;
    loop {
        let byte = *bytes
            .get(*position)
            .ok_or_else(|| invalid_data("truncated GBWT bytecode integer"))?;
        *position = position
            .checked_add(1)
            .ok_or_else(|| invalid_data("GBWT bytecode position overflow"))?;
        let payload = u64::from(byte & 0x7f);
        if shift >= 64 || payload > (u64::MAX >> shift) {
            return Err(invalid_data("GBWT bytecode integer overflow"));
        }
        result = result
            .checked_add(payload << shift)
            .ok_or_else(|| invalid_data("GBWT bytecode integer overflow"))?;
        if byte & 0x80 == 0 {
            if shift > 0 && payload == 0 {
                return Err(invalid_data("non-minimal GBWT bytecode integer"));
            }
            return Ok(result);
        }
        shift = shift
            .checked_add(7)
            .ok_or_else(|| invalid_data("GBWT bytecode shift overflow"))?;
    }
}

fn validate_packed_record(record: &PackedGbwtRecord, expand: bool) -> io::Result<Vec<Position>> {
    if record.occurrence_count == 0 {
        return Err(invalid_data("GBWT record has no occurrences"));
    }
    if record.occurrence_count > MAX_DECODED_OCCURRENCES_PER_TILE {
        return Err(invalid_data(
            "GBWT record exceeds the decoded occurrence safety limit",
        ));
    }
    let mut position = 0_usize;
    let sigma = decode_bytecode_integer(&record.bytes, &mut position)?;
    if sigma == 0 {
        return Err(invalid_data("GBWT record has an empty edge alphabet"));
    }
    let sigma = u64_to_usize(sigma)?;
    if sigma > record.bytes.len().saturating_sub(position) / 2 {
        return Err(invalid_data("GBWT edge count exceeds the record bytes"));
    }
    let mut edges = Vec::with_capacity(sigma);
    let mut previous_handle = 0_u64;
    for edge_index in 0..sigma {
        let delta = decode_bytecode_integer(&record.bytes, &mut position)?;
        let handle = previous_handle
            .checked_add(delta)
            .ok_or_else(|| invalid_data("GBWT successor handle overflow"))?;
        if edge_index > 0 && handle <= previous_handle {
            return Err(invalid_data(
                "GBWT successor handles are not strictly sorted",
            ));
        }
        let offset = decode_bytecode_integer(&record.bytes, &mut position)?;
        edges.push(Position { handle, offset });
        previous_handle = handle;
    }
    if position >= record.bytes.len() {
        return Err(invalid_data("GBWT record has no run-length data"));
    }
    let threshold = if sigma < 255 { 256 / sigma } else { 0 };
    let mut decoded = 0_u64;
    let mut successors = Vec::new();
    if expand {
        successors
            .try_reserve_exact(u64_to_usize(record.occurrence_count)?)
            .map_err(|error| invalid_data(format!("cannot allocate GBWT successors: {error}")))?;
    }
    while position < record.bytes.len() {
        let (rank, run_len) = if sigma >= 255 {
            let rank = u64_to_usize(decode_bytecode_integer(&record.bytes, &mut position)?)?;
            let run_len = decode_bytecode_integer(&record.bytes, &mut position)?
                .checked_add(1)
                .ok_or_else(|| invalid_data("GBWT run length overflow"))?;
            (rank, run_len)
        } else {
            let byte = usize::from(
                *record
                    .bytes
                    .get(position)
                    .ok_or_else(|| invalid_data("truncated GBWT run"))?,
            );
            position += 1;
            let rank = byte % sigma;
            let mut run_len = usize_to_u64(byte / sigma + 1)?;
            if u64_to_usize(run_len)? == threshold {
                run_len = run_len
                    .checked_add(decode_bytecode_integer(&record.bytes, &mut position)?)
                    .ok_or_else(|| invalid_data("GBWT run length overflow"))?;
            }
            (rank, run_len)
        };
        let edge = edges
            .get_mut(rank)
            .ok_or_else(|| invalid_data("GBWT run rank is outside its edge alphabet"))?;
        decoded = decoded
            .checked_add(run_len)
            .filter(|count| *count <= record.occurrence_count)
            .ok_or_else(|| invalid_data("GBWT runs exceed the declared occurrence count"))?;
        if expand {
            for _ in 0..run_len {
                successors.push(*edge);
                edge.offset = edge
                    .offset
                    .checked_add(1)
                    .ok_or_else(|| invalid_data("GBWT successor offset overflow"))?;
            }
        } else {
            edge.offset = edge
                .offset
                .checked_add(run_len)
                .ok_or_else(|| invalid_data("GBWT successor offset overflow"))?;
        }
    }
    if decoded != record.occurrence_count {
        return Err(invalid_data(
            "GBWT runs differ from the declared occurrence count",
        ));
    }
    Ok(successors)
}

fn canonical_edge(from: u64, to: u64) -> bool {
    let from_node = from / 2;
    let to_node = to / 2;
    let from_reverse = from % 2 != 0;
    let to_reverse = to % 2 != 0;
    from != 0
        && to != 0
        && if from_reverse {
            to_node > from_node || (to_node == from_node && !to_reverse)
        } else {
            to_node >= from_node
        }
}

fn encoded_path_is_canonical(path: &[u64]) -> bool {
    let Some((&first, rest)) = path.split_first() else {
        return true;
    };
    let last = *rest.last().unwrap_or(&first);
    let first_reverse = first % 2 != 0;
    let last_reverse = last % 2 != 0;
    if first_reverse == last_reverse {
        !first_reverse
    } else {
        canonical_edge(first, last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN: &[u8] = include_bytes!("../../../test-data/conformance/format-v1.payload.raw");

    fn bytecode(mut value: u64) -> Vec<u8> {
        let mut result = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            result.push(byte);
            if value == 0 {
                return result;
            }
        }
    }

    fn terminal_record(handle: u64, occurrences: u64) -> PackedGbwtRecord {
        let mut bytes = vec![1, 0, 0, 255];
        bytes.extend_from_slice(&bytecode(occurrences - 256));
        PackedGbwtRecord {
            handle,
            occurrence_count: occurrences,
            bytes,
        }
    }

    #[test]
    fn golden_payload_round_trips_byte_identically() {
        let payload = RecordRegionalPayload::decode(GOLDEN).unwrap();
        assert_eq!(payload.encode().unwrap(), GOLDEN);
        let reconstructed = payload.reconstruct_traversals().unwrap();
        assert_eq!(reconstructed.reference_handles, vec![2, 4]);
        assert_eq!(reconstructed.anonymous.len(), 1);
        assert_eq!(reconstructed.anonymous[0].handles, vec![2, 4]);
        assert_eq!(reconstructed.anonymous[0].weight, 1);
    }

    #[test]
    fn malformed_varint_and_trailing_bytes_fail_closed() {
        let mut position = 0;
        assert_eq!(decode_bytecode_integer(&[0], &mut position).unwrap(), 0);
        let mut position = 0;
        assert!(decode_bytecode_integer(&[0x80, 0], &mut position).is_err());

        let mut truncated_varint = GOLDEN.to_vec();
        *truncated_varint.last_mut().unwrap() = 0x80;
        assert!(RecordRegionalPayload::decode(&truncated_varint).is_err());

        let mut trailing = GOLDEN.to_vec();
        trailing.push(0);
        assert!(RecordRegionalPayload::decode(&trailing).is_err());
    }

    #[test]
    fn maximum_occurrence_total_is_accepted_and_next_value_rejected() {
        let half = MAX_DECODED_OCCURRENCES_PER_TILE / 2;
        let payload = RecordRegionalPayload {
            core_start: 100,
            core_end: 101,
            context: CONSTRUCTION_CONTEXT,
            reference_sample: "GRCh38".into(),
            reference_contig: "chr1".into(),
            reference_haplotype: 0,
            reference_fragment_start: 100,
            reference_query_offset: 0,
            reference_node_offset: 0,
            reference_position: (2, 0),
            nodes: BTreeMap::from([(1, b"A".to_vec())]),
            edges: BTreeSet::new(),
            records: vec![terminal_record(2, half), terminal_record(3, half)],
            total_occurrences: MAX_DECODED_OCCURRENCES_PER_TILE,
        };
        let encoded = payload.encode().unwrap();
        assert_eq!(
            RecordRegionalPayload::decode(&encoded)
                .unwrap()
                .total_occurrences,
            MAX_DECODED_OCCURRENCES_PER_TILE
        );

        let mut over = payload;
        over.total_occurrences += 1;
        assert!(over.encode().is_err());
    }

    #[test]
    fn fragmented_and_reverse_reference_anchors_reconstruct_exactly() {
        let mut fragmented = RecordRegionalPayload::decode(GOLDEN).unwrap();
        fragmented.reference_fragment_start = 99;
        fragmented.reference_query_offset = 1;
        fragmented.reference_position = (4, 0);
        let reconstructed = fragmented.reconstruct_traversals().unwrap();
        assert_eq!(reconstructed.reference_handles, vec![2, 4]);
        assert_eq!(
            (reconstructed.reference_start, reconstructed.reference_end),
            (99, 101)
        );

        let mut reverse = RecordRegionalPayload::decode(GOLDEN).unwrap();
        reverse.reference_position = (5, 0);
        let reconstructed = reverse.reconstruct_traversals().unwrap();
        assert_eq!(reconstructed.reference_handles, vec![5, 3]);
        assert_eq!(
            (reconstructed.reference_start, reconstructed.reference_end),
            (100, 102)
        );
    }

    #[test]
    fn cyclic_local_occurrence_graph_fails_closed() {
        let payload = RecordRegionalPayload {
            core_start: 100,
            core_end: 101,
            context: CONSTRUCTION_CONTEXT,
            reference_sample: "GRCh38".into(),
            reference_contig: "chr1".into(),
            reference_haplotype: 0,
            reference_fragment_start: 100,
            reference_query_offset: 0,
            reference_node_offset: 0,
            reference_position: (2, 0),
            nodes: BTreeMap::from([(1, b"A".to_vec())]),
            edges: BTreeSet::new(),
            records: vec![
                PackedGbwtRecord {
                    handle: 2,
                    occurrence_count: 1,
                    bytes: vec![1, 3, 0, 0],
                },
                PackedGbwtRecord {
                    handle: 3,
                    occurrence_count: 1,
                    bytes: vec![1, 2, 0, 0],
                },
            ],
            total_occurrences: 2,
        };
        assert!(payload.reconstruct_traversals().is_err());
    }
}
