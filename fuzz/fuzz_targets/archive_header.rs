#![no_main]

use libfuzzer_sys::fuzz_target;
use pangenome_range_format::decode_header;

fuzz_target!(|data: &[u8]| {
    let _ = decode_header(data);
});
