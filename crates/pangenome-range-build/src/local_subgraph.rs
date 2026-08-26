use crate::source::{PangenomeSource, SourcePathIndex, SourceReferencePosition};
use gbz::bwt::Record;
use gbz::support::{self, NodeSide};
use gbz::{ENDMARKER, FullPathName, Orientation, Pos};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::io;

/// The exact source records and node sequences needed for one regional tile.
///
/// This is deliberately owned by the encoder. It retains only the active
/// subgraph and preserves packed GBWT record bytes without translating them
/// through `gbz-base`.
#[derive(Clone, Debug, Default)]
pub struct LocalSubgraph {
    nodes: BTreeMap<usize, Vec<u8>>,
    records: BTreeMap<usize, Vec<u8>>,
}

impl LocalSubgraph {
    /// Selects a reference interval and its bidirectional graph context.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid interval, missing source data, or GBWT
    /// traversal inconsistencies.
    pub fn around_reference_interval(
        source: &dyn PangenomeSource,
        path_index: &SourcePathIndex,
        reference: &FullPathName,
        start: u64,
        end: u64,
        context: u64,
    ) -> io::Result<(Self, SourceReferencePosition)> {
        if start >= end {
            return Err(invalid_data("reference interval must be nonempty"));
        }
        let mut query = reference.clone();
        query.fragment = usize::try_from(start)
            .map_err(|_| invalid_data("reference start does not fit in usize"))?;
        let reference_position = path_index.reference_position(source, &query)?;
        let mut result = Self::default();
        result.insert_interval(
            source,
            reference_position.position,
            reference_position.node_offset,
            usize::try_from(end - start)
                .map_err(|_| invalid_data("reference interval length does not fit in usize"))?,
            usize::try_from(context)
                .map_err(|_| invalid_data("context length does not fit in usize"))?,
        )?;
        Ok((result, reference_position))
    }

    pub fn node_iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.nodes.keys().copied()
    }

    pub fn handle_iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.records.keys().copied()
    }

    #[must_use]
    pub fn sequence(&self, node_id: usize) -> Option<&[u8]> {
        self.nodes.get(&node_id).map(Vec::as_slice)
    }

    #[must_use]
    pub fn sequence_len(&self, node_id: usize) -> Option<usize> {
        self.nodes.get(&node_id).map(Vec::len)
    }

    #[must_use]
    pub fn packed_record(&self, handle: usize) -> Option<&[u8]> {
        self.records.get(&handle).map(Vec::as_slice)
    }

    pub fn record(&self, handle: usize) -> io::Result<Record<'_>> {
        let bytes = self.packed_record(handle).ok_or_else(|| {
            invalid_data(format!("missing local GBWT record for handle {handle}"))
        })?;
        Record::new(0, bytes)
            .ok_or_else(|| invalid_data(format!("empty local GBWT record for handle {handle}")))
    }

    pub fn supergraph_successors(
        &self,
        node_id: usize,
        orientation: Orientation,
    ) -> io::Result<Vec<(usize, Orientation)>> {
        let handle = support::encode_node(node_id, orientation);
        let record = self.record(handle)?;
        Ok((0..record.outdegree())
            .filter_map(|rank| {
                let successor = record.successor(rank);
                (successor != ENDMARKER).then(|| support::decode_node(successor))
            })
            .collect())
    }

    fn insert_interval(
        &mut self,
        source: &dyn PangenomeSource,
        mut position: Pos,
        mut node_offset: usize,
        mut remaining: usize,
        context: usize,
    ) -> io::Result<()> {
        if remaining == 0 {
            return Err(invalid_data("reference interval must be nonempty"));
        }
        let mut active = BinaryHeap::<Reverse<(usize, (usize, NodeSide))>>::new();
        loop {
            let node_id = support::node_id(position.node);
            let orientation = support::node_orientation(position.node);
            self.add_node(source, node_id)?;
            let sequence_len = self.sequence_len(node_id).ok_or_else(|| {
                invalid_data(format!("missing local sequence for node {node_id}"))
            })?;
            if sequence_len == 0 || node_offset >= sequence_len {
                return Err(invalid_data(format!(
                    "offset {node_offset} in node {node_id} of length {sequence_len}"
                )));
            }

            active.push(Reverse((
                node_offset,
                (node_id, support::entry_side(orientation)),
            )));
            let distance_to_next = sequence_len - node_offset;
            if remaining <= distance_to_next {
                let end_distance = if remaining == distance_to_next {
                    0
                } else {
                    distance_to_next - remaining - 1
                };
                active.push(Reverse((
                    end_distance,
                    (node_id, support::exit_side(orientation)),
                )));
                break;
            }
            active.push(Reverse((0, (node_id, support::exit_side(orientation)))));

            position = self
                .record(position.node)?
                .lf(position.offset)
                .ok_or_else(|| {
                    invalid_data(format!(
                        "no successor for GBWT position ({}, {})",
                        position.node, position.offset
                    ))
                })?;
            node_offset = 0;
            remaining -= distance_to_next;
        }
        self.insert_context(source, active, context)
    }

    fn insert_context(
        &mut self,
        source: &dyn PangenomeSource,
        mut active: BinaryHeap<Reverse<(usize, (usize, NodeSide))>>,
        context: usize,
    ) -> io::Result<()> {
        let mut visited = BTreeSet::<(usize, NodeSide)>::new();
        while let Some(Reverse((distance, node_side))) = active.pop() {
            if !visited.insert(node_side) {
                continue;
            }
            self.add_node(source, node_side.0)?;

            let other_side = (node_side.0, node_side.1.flip());
            if !visited.contains(&other_side) {
                let sequence_len = self.sequence_len(node_side.0).ok_or_else(|| {
                    invalid_data(format!("missing local sequence for node {}", node_side.0))
                })?;
                if sequence_len == 0 {
                    return Err(invalid_data("GBZ node sequence must be nonempty"));
                }
                let next_distance = distance
                    .checked_add(sequence_len - 1)
                    .ok_or_else(|| invalid_data("context distance overflow"))?;
                if next_distance <= context {
                    active.push(Reverse((next_distance, other_side)));
                }
            }

            let handle = support::encode_node(node_side.0, support::exit_orientation(node_side.1));
            let next_distance = distance
                .checked_add(1)
                .ok_or_else(|| invalid_data("context distance overflow"))?;
            if next_distance <= context {
                let record = self.record(handle)?;
                for rank in 0..record.outdegree() {
                    let successor = record.successor(rank);
                    if successor == ENDMARKER {
                        continue;
                    }
                    let next = (
                        support::node_id(successor),
                        support::entry_side(support::node_orientation(successor)),
                    );
                    if !visited.contains(&next) {
                        active.push(Reverse((next_distance, next)));
                    }
                }
            }
        }
        Ok(())
    }

    fn add_node(&mut self, source: &dyn PangenomeSource, node_id: usize) -> io::Result<()> {
        if self.nodes.contains_key(&node_id) {
            return Ok(());
        }
        let sequence = source
            .sequence(node_id)?
            .ok_or_else(|| invalid_data(format!("missing sequence for node {node_id}")))?;
        if sequence.is_empty() {
            return Err(invalid_data(format!(
                "node {node_id} has an empty sequence"
            )));
        }
        for orientation in [Orientation::Forward, Orientation::Reverse] {
            let handle = support::encode_node(node_id, orientation);
            let bytes = source
                .packed_record(handle)?
                .ok_or_else(|| invalid_data(format!("missing GBWT record for handle {handle}")))?;
            Record::new(0, &bytes)
                .ok_or_else(|| invalid_data(format!("empty GBWT record for handle {handle}")))?;
            self.records.insert(handle, bytes);
        }
        self.nodes.insert(node_id, sequence);
        Ok(())
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
