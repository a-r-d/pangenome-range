# Default viewer indexes v1 bounded pilot

## Verdict

Accepted as a bounded release-candidate feature pilot. Every newly encoded
archive now contains `named-loci-v1---` and `summary-pyr-v1--`. Both are
optional extension entries so readers may skip them without losing regional
graph queries, but the production encoder emits both by default. Summaries are
always populated. Named loci are populated only from explicit GFF3 input and
otherwise use a valid empty descriptor.

## Layout and API

Each extension-directory entry addresses a small descriptor. The descriptor
contains sorted key fences or arithmetic summary-series metadata and exact
offset/length/codec/BLAKE3-128 information for independently compressed child
pages. A feature query therefore needs a descriptor and one parallel page
round; it does not read thousands of graph tiles.

The TypeScript reader exposes:

- `capabilities()`;
- `searchLoci({ name, mode, sample?, contig?, limit? })`;
- `summary({ sample, contig, start, end, maxBins? })`.

The encoder accepts `--annotations <GFF3>` and
`--annotation-sample <real-reference-sample>`. The exact annotation SHA-256 and
filename are embedded. It performs no network lookup and invents no assembly
or sample identity.

## MHC size and determinism

The retained pre-feature MHC archive was 4,806,677 bytes. With the same source,
reference filter, 16,384 bp windows, zstd-3, and disk-backed source mode, the
new archive is 4,807,552 bytes: 875 bytes (0.0182%) larger. The fixed bootstrap
index cost is 160 bytes for the two extension-directory entries; all child
pages and descriptors total 715 encoded bytes. The empty locus descriptor and
three summary series contain eight total bins.

One- and four-worker encodes were byte-identical at SHA-256
`bcdf56f5b63124af54e28580e9b679d193b44c9f0e659a8ce0296a1c2e854503`.
Construction measured 601.871 ms and 309.011 ms respectively, with 23,712 KiB
and 28,452 KiB process peak RSS. These are bounded correctness/size runs, not a
claim of performance improvement over the earlier run.

The normal pre-rename structural gates measured 87.562 ms (one worker) and
31.659 ms (four workers). Four-requested-worker full reconstruction used one
effective worker under its memory model and took 274.426 ms. Structural
validation included both descriptors and every known child page.

## Cross-language and source correctness

The annotated golden fixture grew from 7,354 to 8,045 bytes. Its GFF3 produces
four searchable records (`ID`, `Name`, and two aliases) in one leaf plus one
summary bin. TypeScript prefix search for `rang` returns the exact `RANGE1`
feature and CHM13/chr6 interval, and the summary decoder returns the exact
1,024 covered core bases and one accepted tile.

An independent GBZ oracle query over MHC `[100000,200000)` matched the graph and
all seven tile-local weighted-haplotype sets. The focused browser package gate
passed 25/25 tests in two files. `pnpm check:rust`, `pnpm check`, `pnpm build`,
and the 36-measurement Chromium/Firefox/WebKit browser smoke all passed.

## Whole-HPRC evidence and remaining limits

The follow-up [whole-source run](../2026-08-26-default-viewer-indexes-whole-hprc/REPORT.md)
closed the remaining scale gate. On the exact retained source/options/host, the
archive grew by 241,958 bytes (0.00274%), whole wall moved from 499.34 to
503.36 seconds (+0.81%), and peak encoder RSS was 642,220 KiB. All 9/9 graph
oracle queries and 58/58 checked tile-local haplotype sets passed.

Summary memory is compact and scales with reference span. GFF3 searchable
records are still sorted in memory; a disk-backed external sort is the next
required scale improvement before multi-million-record annotation files can be
described as bounded-memory inputs. The independent source oracle also remains
a high-memory research path; its full-source process peaked at 12,973,152 KiB,
separate from the bounded encoder measurement.
