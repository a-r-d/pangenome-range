#![no_main]

use libfuzzer_sys::fuzz_target;
use pangenome_range_format::PackedGbwtRecord;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }
    let handle = u64::from_le_bytes(data[..8].try_into().expect("fixed prefix"));
    let occurrence_count = u64::from_le_bytes(data[8..16].try_into().expect("fixed prefix"));
    let record = PackedGbwtRecord {
        handle,
        occurrence_count,
        bytes: data[16..].to_vec(),
    };
    let _ = record.validate();
});
