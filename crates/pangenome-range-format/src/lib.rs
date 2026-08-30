//! Normative file-format and byte-range primitives for pangenome-range.

mod archive;
mod binary;
mod cost;
mod extensions;
mod features;
mod integrity;
mod metadata;
mod path_membership;
mod regional;
mod source;
mod validation;

pub use archive::{
    ARCHIVE_MAGIC, ARCHIVE_VERSION, ArchiveEntry, ArchiveHeader, BOOTSTRAP_LEN, Bootstrap,
    ChunkCodec, DIRECTORY_BUCKET_WINDOWS, DIRECTORY_ENTRIES_PER_PAGE, DIRECTORY_ENTRY_BYTES,
    DIRECTORY_PAGE_BYTES, DIRECTORY_PAGE_HEADER_BYTES, HEADER_LEN, MAX_ROOT_BYTES,
    ReferenceManifest, RootIndex, bootstrap, compress, decode_directory_page, decode_header,
    decode_root_index, decompress, directory_page_offset, encode_directory_page, encode_header,
    encode_header_with_extensions, encode_root_index,
};
pub use cost::{NetworkCost, NetworkProfile};
pub use extensions::{
    EXTENSION_DIRECTORY_HEADER_BYTES, EXTENSION_DIRECTORY_VERSION, EXTENSION_ENTRY_BYTES,
    EXTENSION_MAGIC, ExtensionEntry, MAX_EXTENSION_DIRECTORY_BYTES, decode_extension_directory,
    encode_extension_directory, validate_extension_payload,
};
pub use features::{
    ExtensionPage, FEATURE_EXTENSION_VERSION, LocusPageDescriptor, LocusRecord,
    MAX_FEATURE_DESCRIPTOR_BYTES, MAX_FEATURE_PAGE_BYTES, MAX_LOCUS_PAGES,
    MAX_LOCUS_RECORDS_PER_PAGE, MAX_SUMMARY_BINS_PER_PAGE, MAX_SUMMARY_SERIES, NAMED_LOCI_MAGIC,
    NAMED_LOCI_PAGE_MAGIC, NAMED_LOCI_TYPE_ID, NamedLociDescriptor, SUMMARY_BIN_BYTES,
    SUMMARY_PAGE_MAGIC, SUMMARY_PYRAMID_MAGIC, SUMMARY_PYRAMID_TYPE_ID, SummaryBin,
    SummaryPyramidDescriptor, SummarySeriesDescriptor, decode_locus_page,
    decode_named_loci_descriptor, decode_summary_descriptor, decode_summary_page,
    encode_locus_page, encode_named_loci_descriptor, encode_summary_descriptor,
    encode_summary_page, normalize_locus_key, validate_extension_page,
};
pub use integrity::{IntegrityEvaluation, IntegrityPlacementEstimate, evaluate_integrity_options};
pub use metadata::{
    ARCHIVE_METADATA_HEADER_BYTES, ARCHIVE_METADATA_MAGIC, ARCHIVE_METADATA_TYPE_ID,
    ARCHIVE_METADATA_VERSION, ArchiveMetadata, MAX_ARCHIVE_METADATA_BYTES, decode_archive_metadata,
    encode_archive_metadata,
};
pub use path_membership::{
    MAX_PATH_CATALOG_PAGES, MAX_PATH_MEMBERSHIP_GROUPS_PER_TILE, MAX_PATH_MEMBERSHIP_MANIFESTS,
    PATH_CATALOG_PAGE_MAGIC, PATH_CATALOG_RECORDS_PER_PAGE, PATH_MEMBERSHIP_DESCRIPTOR_MAGIC,
    PATH_MEMBERSHIP_DIRECTORY_ENTRIES_PER_PAGE, PATH_MEMBERSHIP_DIRECTORY_ENTRY_BYTES,
    PATH_MEMBERSHIP_DIRECTORY_HEADER_BYTES, PATH_MEMBERSHIP_DIRECTORY_MAGIC,
    PATH_MEMBERSHIP_DIRECTORY_PAGE_BYTES, PATH_MEMBERSHIP_TYPE_ID, PATH_MEMBERSHIP_VERSION,
    PathCatalogPageDescriptor, PathCatalogRecord, PathMembership, PathMembershipDescriptor,
    PathMembershipDirectoryEntry, PathMembershipManifest, TILE_MEMBERSHIP_PAGE_MAGIC,
    TraversalMembershipGroup, decode_path_catalog_page, decode_path_membership_descriptor,
    decode_path_membership_directory_page, decode_tile_membership_page, encode_path_catalog_page,
    encode_path_membership_descriptor, encode_path_membership_directory_page,
    encode_tile_membership_page, traversal_membership_digest,
};
pub use regional::{
    CONSTRUCTION_CONTEXT, MAX_DECODED_OCCURRENCES_PER_TILE, PackedEdge, PackedGbwtRecord,
    REGION_MAGIC, REGION_VERSION, ReconstructedTraversals, RecordRegionalPayload,
    RegionalWeightedTraversal,
};
pub use source::{FileRangeSource, RangeRead, RangeSource, TraceSummary, TracingRangeSource};
pub use validation::{
    ArchiveValidationProgress, ArchiveValidationSummary, ValidationMode, ValidationOptions,
    validate_archive, validate_archive_with_options, validate_archive_with_progress,
};
