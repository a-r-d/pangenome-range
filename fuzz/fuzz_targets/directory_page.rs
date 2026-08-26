#![no_main]

use libfuzzer_sys::fuzz_target;
use pangenome_range_format::{
    ChunkCodec, DIRECTORY_PAGE_BYTES, ReferenceManifest, decode_directory_page,
};

fuzz_target!(|data: &[u8]| {
    if data.len() != DIRECTORY_PAGE_BYTES {
        return;
    }
    let manifest = ReferenceManifest {
        sample: "reference".into(),
        contig: "contig".into(),
        start: 0,
        end: u64::MAX / 2,
        grid_start: 0,
        window_size: 16_384,
        bucket_span: 524_288,
        first_page_offset: 64,
        page_count: 1,
        entry_count: 0,
        codec: ChunkCodec::Zstd3,
    };
    let _ = decode_directory_page(data, &manifest, 0);
});
