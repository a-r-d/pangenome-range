use sha2::{Digest, Sha256};
use std::path::PathBuf;

const TINY_FIXTURE_SHA256: &str =
    "1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788";

pub(crate) fn tiny_gbz_fixture() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-data/micb-kir3dl1.gbz");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "tracked MICB/KIR3DL1 fixture is unavailable at {}: {error}",
            path.display()
        )
    });
    assert_eq!(
        bytes.len(),
        73_920,
        "tracked MICB/KIR3DL1 fixture byte length changed"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        TINY_FIXTURE_SHA256,
        "tracked MICB/KIR3DL1 fixture checksum changed"
    );
    path
}
