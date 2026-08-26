#![no_main]

use libfuzzer_sys::fuzz_target;
use pangenome_range_format::{ArchiveHeader, ARCHIVE_VERSION, decode_root_index};

fuzz_target!(|data: &[u8]| {
    let header = ArchiveHeader {
        version: ARCHIVE_VERSION,
        root_len: data.len() as u64,
        entry_count: 0,
        data_offset: 64_u64.saturating_add(data.len() as u64),
        extension_directory_offset: 0,
        extension_directory_len: 0,
    };
    let _ = decode_root_index(data, header);
});
