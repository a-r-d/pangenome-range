# Test data

The tiny MICB/KIR3DL1 and medium MHC fixtures are intentionally committed for
hermetic integration, conformance, and benchmark smoke tests. Ordinary tests do
not fetch data or require network access.

Fetch the Tier 1 fixture with:

```bash
./scripts/fetch-test-data.sh
```

Fetch the medium MHC integration fixture with:

```bash
./scripts/fetch-test-data.sh mhc
```

Use `./scripts/fetch-test-data.sh all` to restore and verify both fixtures if a
local copy is missing. Both GBZ files are tracked intentionally.

The script downloads `micb-kir3dl1.gbz` from `jltsiren/gbz-base` commit
`a5ed1ff3ddc402e230d1187afa438e05c8b3654e` and requires this SHA-256:

```text
1d574ede7533150eb87f6837a7763d4eac120aa03f34877392ecdd53b0410788
```

The upstream repository is MIT licensed. Its README identifies the graph as a
subset of the HPRC Minigraph-Cactus v1.1 graph and the vg haplotype-sampling test
case, covering:

- `GRCh38#chr6:31498145-31511124` (MICB)
- `GRCh38#chr19:54816468-54830778` (KIR3DL1)

Source and provenance links are recorded in `docs/UPSTREAM.md`. The exact
73,920-byte file is retained for test-only use under the upstream MIT license.
Rust tests verify its checksum before opening it. The fetch script remains a
checksum-verified restoration path.

## Medium MHC integration fixture

`mhc-10.gbz` is fetched from `vgteam/vg_snakemake` commit
`d938c2035fa5ce16acd69147743762b513292173`. It is 4,511,832 bytes and requires
this SHA-256:

```text
a0b44236852d5659202a6855308020df05efd7c2be90645d341d94fb775df685
```

The upstream README says the graph was built with Minigraph-Cactus from 10 MHC
haplotypes selected from Heng Li's MHC-61 dataset. Local inspection with this
repository's pinned Rust reader reports:

- 157,913 nodes;
- 10 paths representing 10 haplotypes from 6 samples;
- one `MHC-GRCh38#0#MHC@0` reference path;
- node-to-segment translation metadata.

This is a useful medium fixture rather than the final Tier 2 chromosome target.
At 61 times the compressed size of MICB/KIR3DL1, it is large enough to exercise
100 kb and 1 Mb intervals and to expose index/chunk scaling behavior while still
being quick to fetch and inspect on a laptop.

The upstream workflow repository is GPL-3.0 licensed, and the input sequences
are traced to Zenodo record `10.5281/zenodo.6617246`, which identifies them as
HPRC year-1 material. The repository tracks this fixture under the explicit
small-fixture decision recorded here; re-check the source record's license and
the applicable HPRC Data Use Protocol before redistributing the GBZ or derived
data outside this research repository.

## Current v1 golden fixtures

`golden/record-region-v1.hex` is the normative uncompressed `PNGRGN01`
regional payload. `record-region-v1.zstd3.hex` is its zstd level-3 stored
representation, and `record-region-v1.expected.json` records the decoded
reference, topology, sequences, weighted traversal, and BLAKE3 digests. Rust and
TypeScript decode the same bytes.

`golden/record-archive-v1.pngr` is a 7,354-byte deterministic archive containing
one 1,024 bp CHM13 chr6 tile derived from the tiny MICB/KIR3DL1 source. Its
source checksum, archive checksum, exact command, interval, codec, and format
identifiers are retained in `record-archive-v1.json`. The Rust test suite
rebuilds it and checks the decoded query against the source oracle.

## Cross-language conformance fixture

`conformance/manifest.json` is generated deterministically by:

```bash
cargo run --release -p pangenome-range-cli -- fixtures export test-data/conformance
```

It describes exactly one synthetic two-node fixture:

| Fixture | Archive | Regional payload | Semantics |
|---|---:|---:|---|
| `format-v1` | `PNGRNG01` / 1 | `PNGRGN01` / 1 | reconstructed distinct weighted anonymous tile paths |

The fixture includes a complete `.pngr`, isolated header/root/directory/raw
payload bytes, Rust zstd frames at levels 1/3/6, SHA-256 checksums, expected
references/tile data, and the Rust canonical hash. Rust exports and rereads it;
TypeScript decodes every artifact and matches the same expectations.

`conformance/micb-kir3dl1-reader-v1.pngr` is the 164,259-byte Node integration
archive derived from the pinned Tier 1 source. Its sidecar records source and
archive checksums, the deterministic encode command, format identifiers, query
coordinates, canonical hashes, and exact expected HTTP ranges. It contains
eight record-preserving v1 chunks and is retained only as compact integration
evidence; the source GBZ remains fetched with explicit provenance.

Older research fixtures are intentionally absent. They are not compatibility
test cases and their archives must be regenerated.
