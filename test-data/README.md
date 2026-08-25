# Test data

The medium MHC fixture is intentionally committed for hermetic integration and
benchmark smoke tests. Other third-party binary fixtures remain fetched locally.

Fetch the Tier 1 fixture with:

```bash
./scripts/fetch-test-data.sh
```

Fetch the medium MHC integration fixture with:

```bash
./scripts/fetch-test-data.sh mhc
```

Use `./scripts/fetch-test-data.sh all` to fetch and verify both. The tiny
MICB/KIR3DL1 GBZ remains ignored by Git; `mhc-10.gbz` is tracked intentionally.

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

Source and provenance links are recorded in `docs/UPSTREAM.md`. This fetch model
keeps provenance explicit and lets us revisit redistribution/data-use terms
before publishing any derived fixture.

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
