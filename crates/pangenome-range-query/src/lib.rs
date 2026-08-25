//! Storage-independent query and correctness utilities.
//!
//! Candidate readers should consume [`pangenome_range_format::RangeSource`] and emit a
//! [`CanonicalSubgraph`]. The same canonical representation can be produced by
//! a GBZ or GBZ-base oracle, keeping graph semantics separate from storage.

use std::collections::{BTreeMap, BTreeSet};

pub use pangenome_range_format::RangeSource;

/// A node visit with orientation preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrientedNode {
    pub id: u64,
    pub reverse: bool,
}

/// A directed edge between oriented node visits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: OrientedNode,
    pub to: OrientedNode,
}

/// Reference-coordinate context attached to a query result.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReferenceInterval {
    pub sample: String,
    pub contig: String,
    pub start: u64,
    pub end: u64,
}

/// A named path or haplotype traversal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CanonicalPath {
    pub sample: String,
    pub contig: String,
    pub haplotype: u64,
    pub fragment: u64,
    pub is_reference: bool,
    pub traversal: Vec<OrientedNode>,
}

/// Canonical graph semantics for an extracted region.
///
/// Maps and sets make node and edge ordering irrelevant. Path order is
/// normalized during comparison and hashing, while traversal order remains
/// significant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CanonicalSubgraph {
    pub nodes: BTreeMap<u64, Vec<u8>>,
    pub edges: BTreeSet<Edge>,
    pub paths: Vec<CanonicalPath>,
    pub reference_intervals: BTreeSet<ReferenceInterval>,
}

impl CanonicalSubgraph {
    /// Returns a normalized copy suitable for direct semantic comparison.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized.paths.sort();
        normalized
    }

    /// Returns whether two results contain the same normalized semantics.
    #[must_use]
    pub fn equivalent_to(&self, other: &Self) -> bool {
        self.normalized() == other.normalized()
    }

    /// Returns a stable BLAKE3 digest of the normalized semantics.
    #[must_use]
    pub fn canonical_hash(&self) -> blake3::Hash {
        let normalized = self.normalized();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"pangenome-range canonical subgraph v1\0");

        put_u64(&mut hasher, normalized.nodes.len() as u64);
        for (id, sequence) in &normalized.nodes {
            put_u64(&mut hasher, *id);
            put_bytes(&mut hasher, sequence);
        }

        put_u64(&mut hasher, normalized.edges.len() as u64);
        for edge in &normalized.edges {
            put_oriented_node(&mut hasher, edge.from);
            put_oriented_node(&mut hasher, edge.to);
        }

        put_u64(&mut hasher, normalized.paths.len() as u64);
        for path in &normalized.paths {
            put_bytes(&mut hasher, path.sample.as_bytes());
            put_bytes(&mut hasher, path.contig.as_bytes());
            put_u64(&mut hasher, path.haplotype);
            put_u64(&mut hasher, path.fragment);
            hasher.update(&[u8::from(path.is_reference)]);
            put_u64(&mut hasher, path.traversal.len() as u64);
            for node in &path.traversal {
                put_oriented_node(&mut hasher, *node);
            }
        }

        put_u64(&mut hasher, normalized.reference_intervals.len() as u64);
        for interval in &normalized.reference_intervals {
            put_bytes(&mut hasher, interval.sample.as_bytes());
            put_bytes(&mut hasher, interval.contig.as_bytes());
            put_u64(&mut hasher, interval.start);
            put_u64(&mut hasher, interval.end);
        }

        hasher.finalize()
    }
}

fn put_u64(hasher: &mut blake3::Hasher, value: u64) {
    hasher.update(&value.to_le_bytes());
}

fn put_bytes(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    put_u64(hasher, bytes.len() as u64);
    hasher.update(bytes);
}

fn put_oriented_node(hasher: &mut blake3::Hasher, node: OrientedNode) {
    put_u64(hasher, node.id);
    hasher.update(&[u8::from(node.reverse)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(sample: &str) -> CanonicalPath {
        CanonicalPath {
            sample: sample.into(),
            contig: "chr6".into(),
            haplotype: 1,
            fragment: 0,
            is_reference: false,
            traversal: vec![OrientedNode {
                id: 1,
                reverse: false,
            }],
        }
    }

    #[test]
    fn path_order_does_not_change_equivalence_or_hash() {
        let mut left = CanonicalSubgraph::default();
        left.nodes.insert(1, b"AC".to_vec());
        left.paths = vec![path("z"), path("a")];

        let mut right = left.clone();
        right.paths.reverse();

        assert!(left.equivalent_to(&right));
        assert_eq!(left.canonical_hash(), right.canonical_hash());
    }

    #[test]
    fn traversal_orientation_changes_hash() {
        let mut left = CanonicalSubgraph::default();
        left.paths.push(path("sample"));
        let mut right = left.clone();
        right.paths[0].traversal[0].reverse = true;

        assert!(!left.equivalent_to(&right));
        assert_ne!(left.canonical_hash(), right.canonical_hash());
    }

    #[test]
    fn duplicate_path_multiplicity_is_semantically_significant() {
        let mut left = CanonicalSubgraph::default();
        left.paths.push(path("sample"));
        let mut right = left.clone();
        right.paths.push(path("sample"));

        assert!(!left.equivalent_to(&right));
        assert_ne!(left.canonical_hash(), right.canonical_hash());
    }
}
