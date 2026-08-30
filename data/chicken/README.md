# Chicken pangenome demo data

Large source and generated objects stay outside this repository. The checked-in
files describe two distinct results from the Rice et al. 30-assembly chicken
pangenome ([paper](https://doi.org/10.1186/s12915-023-01758-0),
[dataset](https://doi.org/10.5281/zenodo.10018222)).

## Historical chromosome 15 pilot

`scripts/chicken/build-chicken-demo.sh` extracts the closed chromosome 15 GFA
component before running vg. It produced the 15.9 MB research object retained in
`results/named-membership/chicken/`. This pilot proved named-membership behavior
at IGLL1. It is not the configured production demo and must not be described as
the whole chicken corpus.

## Whole-genome named-membership archive

`scripts/chicken/build-chicken-whole.sh` converts the complete 12.63 GB
uncompressed GFA and encodes every one of the 207 `bGalGal1b` reference paths.
The measured objects are:

| Object | Bytes | SHA-256 |
| --- | ---: | --- |
| source GFA gzip | 2,358,369,558 | `609230aa36071690bfde02445a7d0693f75e512a79276f60a3b5d04361c2600d` |
| generated GBZ | 1,359,062,880 | `96c04d263e8af7cf0863cfda8a22bb5cc9b9c3aea387cdd59a05db7a7ab1ea7f` |
| mapped gene GFF3 | 4,424,252 | `0b9b65ba6db7cac55636d392638895faf0758dc2d0be0944347c3501c1d6ee27` |
| whole `.pngr` | 1,498,984,132 | `93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e` |

The archive/source ratio is 1.102954x. It contains 64,371 regional payloads,
56,390 searchable locus records, 12,237 GBWT source-path catalog records,
717,130 canonical tile-local traversal groups, and 1,850,732 membership records.
Catalog records are paths, not chickens or genomes.

## Reproduce the toolchain and build

The whole conversion used vg commit
`32cadf3d3ee45d04c532767158c7dee6243f5713` with its pinned GBWTGraph submodule
and the one-line rvalue-move repair under `patches/vg/`. Build and verify it from
a clean checkout with:

```bash
export CHICKEN_DATA_DIR=/external/chicken
scripts/chicken/build-pinned-vg.sh "$CHICKEN_DATA_DIR/tools/pinned-vg" --jobs 8
export VG_BIN="$CHICKEN_DATA_DIR/tools/pinned-vg/vg-source/bin/vg"
scripts/chicken/fetch-chicken.sh
scripts/chicken/build-chicken-whole.sh
```

The script requires the measured vg SHA-256 and preserves the original one-job,
20-million-node buffer, 42 GB hard memory limit, and zero-swap conversion guard.
The `.pngr` stages are capped at 2 GB. `data/chicken/sources.json` records every
input, derived checksum, conversion command, path-selection rule, upload object,
and origin result. `data/chicken/vg-tool-manifest.json` records the exact source,
submodule, patch, compiler, build mode, and binary identity.

## Paper-backed IGLL1 preset

The graph VCF is independently checksum-pinned. Regenerate the UCD312 deletion
preset with no more than a 256 MB Node heap:

```bash
node --max-old-space-size=256 scripts/chicken/build-demo-presets.mjs \
  "$CHICKEN_DATA_DIR/chicken-whole-named.pngr" \
  "$CHICKEN_DATA_DIR/pangenome.vcf.gz" \
  data/chicken/demo-presets.json
```

The generator requires the exact chr15:7,944,313 VCF deletion edge, verifies
UCD312 genotype `12|1` and allele count one, and maps the edge to two adjacent
tile-local groups that both resolve exclusively to one UCD312 source path. It
fails on checksum mismatch, ambiguous group mapping, or catalog disagreement.
See `docs/CHICKEN_DEMO.md` for the scientific/product boundary.

The Zenodo graph file is distributed under CC BY 4.0. The derived archive keeps
the Rice et al. citation and license; see `data/chicken/LICENSE_REVIEW.md`.
