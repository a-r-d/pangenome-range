//! Normative file-format and byte-range primitives for pangenome-range.

mod archive;
mod binary;
mod cost;
mod extensions;
mod integrity;
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
pub use integrity::{IntegrityEvaluation, IntegrityPlacementEstimate, evaluate_integrity_options};
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
