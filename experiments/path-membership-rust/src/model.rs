use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PathMembership {
    pub path_id: u64,
    pub multiplicity: u64,
    pub reversed_relative_to_group: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedTraversalGroup {
    pub traversal: Vec<u64>,
    pub occurrence_weight: u64,
    pub unique_path_count: u64,
    pub memberships: Vec<PathMembership>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocatedPosition {
    pub node: u64,
    pub offset: u64,
    pub sequence_id: u64,
    pub path_id: u64,
    pub reversed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedTile {
    pub core_start: u64,
    pub core_end: u64,
    pub payload_path: String,
    pub encoded_graph_bytes: u64,
    pub decoded_graph_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedManifest {
    pub schema_version: u32,
    pub archive: String,
    pub archive_bytes: u64,
    pub sample: String,
    pub contig: String,
    pub start: u64,
    pub end: u64,
    pub graph_query_bytes: u64,
    pub graph_query_ranges: u64,
    pub tiles: Vec<PreparedTile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCodecSizes {
    pub delta_varint: u64,
    pub interval_run: u64,
    pub dense_bitset: u64,
    pub roaring_blocks: u64,
    pub complement: Option<u64>,
    pub adaptive_codec: String,
    pub adaptive_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileAnalysis {
    pub core_start: u64,
    pub core_end: u64,
    pub distinct_groups: u64,
    pub total_occurrence_weight: u64,
    pub unique_paths: u64,
    pub paths_in_multiple_groups: u64,
    pub paths_with_multiplicity_gt_one: u64,
    pub disjoint_partition: bool,
    pub dominant_group_share: f64,
    pub groups: Vec<NamedTraversalGroup>,
    pub codecs: Vec<GroupCodecSizes>,
}
