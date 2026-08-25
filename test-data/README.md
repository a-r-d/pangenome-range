# Test data

Third-party binary fixtures are not committed to this repository.

Fetch the Tier 1 fixture with:

```bash
./scripts/fetch-test-data.sh
```

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

