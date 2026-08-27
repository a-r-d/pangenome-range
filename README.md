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

[Format specification](docs/FILE_FORMAT_V1.md) · [Live demo](https://a-r-d.github.io/pangenome-range/demo) · [Hosting requirements](docs/HOSTING.md)

## Encoder

```bash
pangenome-range encode input.gbz output.pngr
```

Add a searchable gene index with GFF3 annotations:

```bash
pangenome-range encode input.gbz output.pngr --annotations genes.gff3 --annotation-release v50 --annotation-assembly GRCh38.p14
```

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
- [Architecture decisions](docs/adr/) · [distribution](docs/DISTRIBUTION.md) · [hosting](docs/HOSTING.md)

## License

[MIT](LICENSE)
