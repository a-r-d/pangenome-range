#![no_main]

use libfuzzer_sys::fuzz_target;
use pangenome_range_format::RecordRegionalPayload;

fuzz_target!(|data: &[u8]| {
    let _ = RecordRegionalPayload::decode(data);
});
