# pangenome-range

[![CI](https://img.shields.io/github/actions/workflow/status/a-r-d/pangenome-range/ci.yml?branch=main&style=for-the-badge&logo=githubactions&logoColor=white&label=CI)](https://github.com/a-r-d/pangenome-range/actions/workflows/ci.yml)
[![MIT](https://img.shields.io/badge/license-MIT-blue?style=for-the-badge)](LICENSE)

## [Open the live demo →](https://a-r-d.github.io/pangenome-range/demo)

`pangenome-range` is four things:

1. `.pngr`, a range-readable pangenome file format.
2. A reference Rust encoder from GBZ to `.pngr`.
3. A reference JavaScript decoder.
4. A [browser demo](https://a-r-d.github.io/pangenome-range/demo) using that decoder.

It is inspired by tiled GeoTIFF systems: convert a huge GBZ pangenome into one tiled, static `.pngr` object, then query small regions in a browser with HTTP Range requests instead of downloading the whole graph.

[Format specification](docs/FILE_FORMAT_V1.md) · [Live demo](https://a-r-d.github.io/pangenome-range/demo) · [Viewer requirements](docs/VIEWER_PRODUCT_REQUIREMENTS.md) · [Hosting requirements](docs/HOSTING.md)

### Curated demo views

The HPRC archive includes GENCODE v50 named-locus search:

- [HLA-B — dense MHC variation](https://a-r-d.github.io/pangenome-range/demo?archive=hprc&locus=HLA-B&zoom=0.32&center=0.5&vscale=1.3)
- [MICB — MHC class I-related variation](https://a-r-d.github.io/pangenome-range/demo?archive=hprc&locus=MICB&zoom=0.38&center=0.5&vscale=1.2)
- [KIR3DL1 — structurally complex immune locus](https://a-r-d.github.io/pangenome-range/demo?archive=hprc&locus=KIR3DL1&zoom=0.3&center=0.5&vscale=1.25)
- [CHAD — compact alternate paths and node labels](https://a-r-d.github.io/pangenome-range/demo?archive=hprc&locus=CHAD&zoom=0.45&center=0.5&vscale=1.2)
- [CRISP1 — dense graph and display-budget controls](https://a-r-d.github.io/pangenome-range/demo?archive=hprc&locus=CRISP1&zoom=0.25&center=0.5&vscale=1)

The second archive exposes real NA19239 haplotype-0 population-path coordinates, not GRCh38 coordinates, and does not contain named-gene annotations:

- [1000 Genomes NA19239 haplotype 0 — chromosome 1 opening window](https://a-r-d.github.io/pangenome-range/demo?archive=1000g&sample=NA19239&contig=1&start=0&end=32768&zoom=0.35&center=0.5&vscale=1)

The rice demo explores chromosome 6 from the PPanG rice pangenome, using NATELBORO as the reference. The paths show local patterns of variation across the graph, not individual named rice accessions:

- [Rice Xa7 — bacterial-blight-resistance locus](https://a-r-d.github.io/pangenome-range/demo?archive=rice&locus=Xa7&zoom=2.856&center=0.600576&vscale=1.45)

Demo URLs can select `archive=hprc`, `archive=1000g`, `archive=rice`, or `archive=fixture`; select a named `locus` or an exact `sample`/`contig`/`start`/`end` interval; and restore horizontal `zoom`, normalized `center`, and vertical `vscale`. Local file selections cannot be restored from a link because browsers do not grant URLs access to local files.

## Encoder

```bash
pangenome-range encode input.gbz output.pngr
```

Add a searchable gene index with GFF3 annotations:

```bash
pangenome-range encode input.gbz output.pngr --annotations genes.gff3 --annotation-release v50 --annotation-assembly GRCh38.p14
```

Add named GBWT source-path identity with `--path-membership`. The disk-backed
encoder parses the document-array samples embedded in GBWT, performs bounded
tile-batched LF locate, and writes a paged catalog plus one membership page for
each graph tile. `--path-locate-max-lf-steps` is a hard per-position guard. Source
cache v2 persists the authenticated locate support and catalog, so repeated
encodes do not restream the GBZ. Named membership requires complete GBWT path,
sample, and contig metadata and fails closed instead of inventing labels. No local
`gbwt-rs` fork is a dependency.

The browser API reads the optional feature independently from graph queries:

```ts
const membership = await archive.pathMembership({
  sample: "GRCh38",
  contig: "chr6",
  start: 31_498_145,
  end: 31_511_124,
});
console.log(membership.paths, membership.tiles[0]?.groups);

const region = await archive.query({
  sample: "GRCh38",
  contig: "chr6",
  start: 31_498_145,
  end: 31_511_124,
});
const groups = await archive.tilePathMemberships(region.tiles[0]);
const sourcePath = await archive.pathById(groups[0].memberships[0].pathId);

const combined = await archive.queryWithPathMembership({
  sample: "GRCh38",
  contig: "chr6",
  start: 31_498_145,
  end: 31_511_124,
});
console.log(combined.trace.graph, combined.trace.membership, combined.trace.catalog);
```

Each returned path has a real canonical GBWT path ID, structured metadata, and a
`canonicalName` deterministically reconstructed from that metadata. GBWT does not
retain the original input spelling of every path name.

Anonymous weighted tile paths remain the default graph semantics. Named memberships
are tile-local evidence; the reader does not stitch them across tiles.

## JavaScript decoder

```ts
import { openPangenome } from "pangenome-range";

const archive = await openPangenome("https://example.org/graph.pngr");
const region = await archive.query({
  sample: "GRCh38",
  contig: "chr6",
  start: 31_498_145,
  end: 31_511_124,
});

console.log(region.graph.nodes.ids.length, region.tiles.length);
await archive.close();
```

## Encoder performance

Largest measured encode: HPRC v2.1 GRCh38 with GENCODE v50 genes, eight workers, on the same NVMe host.

| Measurement | Result |
| --- | ---: |
| Source GBZ | 5,492,627,216 bytes (5.115 GiB) |
| GFF3 annotations | 4,763,975,927 bytes |
| Output `.pngr` | 8,832,750,626 bytes (1.608× source) |
| Cold encode | 612.87 s, 685,992 KiB peak RSS |
| Warm reusable-cache encode | 384.08 s, 621,808 KiB peak RSS |
| Reusable source cache | 12,055,087,949 bytes |
| Prebuild, cold → warm | 118.94 s → 21.69 s |
| Correctness | 363,105/363,105 payloads; 9/9 graph and 58/58 haplotype checks |

Cold and warm modes produced byte-identical output. Only the prebuild reduction is directly attributable to cache reuse; whole-wall timing also contains normal I/O variance. [Full report](results/2026-08-26-release-hardening-v1/REPORT.md).

## Demo performance

Five fresh Chromium sessions against the [deployed demo](https://a-r-d.github.io/pangenome-range/demo) and its 8.8 GB remote archive:

| Measurement | Result |
| --- | ---: |
| HTTP Range requests | 5 |
| Archive bytes read | 73,972 bytes |
| Median page-to-region-ready | 520 ms |
| Observed range | 502–1,456 ms |

Measured August 27, 2026 over the public network; latency will vary by location and cache state.

## Encoder arguments

| Argument | Default | Meaning |
| --- | --- | --- |
| `input.gbz` | required | Source GBZ file. |
| `output.pngr` | required | New archive path; existing files are not overwritten. |
| `--sample NAME` | all | Encode one reference sample. |
| `--reference-haplotype N` | none | Explicitly use haplotype `N` of `--sample` as the real coordinate anchor when the GBWT does not tag a reference; disk source only, without persistent-cache reuse. |
| `--contig NAME` | all | Encode one reference contig. |
| `--start BP` | contig start | Start coordinate; requires `--contig`. |
| `--end BP` | contig end | End coordinate; requires `--contig`. |
| `--window-size BP` | `16384` | Base tile width. |
| `--codec NAME` | `zstd-3` | `none`, `zstd-1`, `zstd-3`, or `zstd-6`. |
| `--haplotypes MODE` | `anonymous-distinct-weighted-tile-paths` | Haplotype semantics; `distinct` is an alias. |
| `--max-uncompressed-chunk-bytes N` | `8388608` | Split tiles above this decoded-size limit. |
| `--min-window-size BP` | `1024` | Smallest adaptive tile; must not exceed `--window-size`. |
| `--threads N` | up to 8 cores | Bounded tile and compression workers. |
| `--max-queued-bytes N` | `268435456` | Raw plus compressed worker-queue memory limit. |
| `--source-access MODE` | `disk` | `disk` for bounded RAM or `loaded` for the in-memory oracle. |
| `--scratch-dir PATH` | output directory | Ephemeral disk-cache location. |
| `--source-cache PATH` | none | Reuse an authenticated persistent cache; disk mode only. |
| `--annotations PATH` | none | GFF3 file used to build named-locus search. |
| `--annotation-sample NAME` | inferred if unique | Reference sample for GFF3 coordinates; required when multiple samples are encoded. |
| `--annotation-feature-type TYPE` | `gene` | Exact GFF3 feature type; repeat to include more types. |
| `--annotation-release ID` | none | Required with `--annotations`. |
| `--annotation-assembly ID` | none | Required with `--annotations`. |
| `--reference-assembly ID` | none | Reference assembly stored in archive metadata. |
| `--dataset-title TEXT` | none | Deterministic archive title. |
| `--dataset-description TEXT` | none | Deterministic archive description. |
| `--source-uri URI` | none | Canonical source URI; local paths and `file:` URIs are rejected. |
| `--experimental-path-membership-summary PATH` | none | Prepared bounded tile-membership JSON; research only and requires the matching catalog. |
| `--experimental-path-catalog PATH` | none | Complete contiguous path-catalog NDJSON; research only and requires the matching summary. |
| `--path-membership` | off | Preserve named source-path membership from embedded GBWT DA samples; disk mode only. |
| `--path-locate-max-lf-steps N` | `1000000` | Hard LF-step limit for each located traversal start. |
| `--keep-partial` | off | Keep the temporary sibling archive after failure. |
| `--progress MODE` | terminal: plain; redirected: off | `auto`, `plain`, `json`, or `off`. |
| `--progress-interval-seconds N` | `5` | Progress update interval. |
| `--max-chunks N` | none | Research guard limiting one selected reference path. |
| `--report PATH` | none | Write a JSON build report. |
| `--help`, `-h` | — | Show CLI help. |

## Reference

- [File-format v1 specification](docs/FILE_FORMAT_V1.md)
- [Architecture](docs/ARCHITECTURE.md) · [fixed-window archive](docs/FIXED_WINDOW_ARCHIVE.md) · [haplotype semantics](docs/HAPLOTYPE_SEMANTICS.md)
- [Benchmarks](docs/BENCHMARKS.md) · [optimization log](docs/OPTIMIZATION_LOG.md) · [release checklist](docs/FORMAT_RELEASE_CHECKLIST.md)
- [Viewer performance](docs/VIEWER_PERFORMANCE.md) · [actual viewer format gaps](docs/VIEWER_FORMAT_GAPS.md)
- [Architecture decisions](docs/adr/) · [distribution](docs/DISTRIBUTION.md) · [hosting](docs/HOSTING.md)

## License

[MIT](LICENSE)

## Thank you and data attribution

This project is possible because research groups made large pangenome resources openly available. Thank you to:

- The [Human Pangenome Reference Consortium](https://github.com/human-pangenomics/hpp_pangenome_resources) for the HPRC Release 2 Minigraph-Cactus v2.1 graph used by the main human demo. The HPRC Data Use Protocol applies to that source data.
- The [GENCODE project](https://www.gencodegenes.org/) for the Human Release 50 gene annotations that power named-locus search in the HPRC demo ([exact GFF3 source](https://ftp.ebi.ac.uk/pub/databases/gencode/Gencode_human/release_50/gencode.v50.annotation.gff3.gz)).
- The [1000 Genomes Project](https://www.internationalgenome.org/) and the [UCSC Computational Genomics Lab](https://cgl.gi.ucsc.edu/data/giraffe/mapping/graphs/for-NA19239/1000gplons/hs38d1/) for the 1000GPlons `hs38d1` graph used by the NA19239 demo.
- The [PPanG team at SJTU-CGM](https://cgm.sjtu.edu.cn/PPanG/) for the rice Minigraph-Cactus graphs and Xa7 locus context ([source repository](https://github.com/SJTU-CGM/PPanG), [paper](https://doi.org/10.1186/s12864-024-10302-5)).

The demo archives are derived, range-addressable representations. Please credit and follow the terms of the original data providers when using them; the repository's MIT license covers the `pangenome-range` software, not a replacement license for every upstream dataset.
