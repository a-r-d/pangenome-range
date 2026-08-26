# Fixed-window archive v1

Status: current pre-release implementation.

The normative byte contract is [Pangenome Range File Format v1](FILE_FORMAT_V1.md).
This document describes the design and its measured research status.

## Layout

A v1 object is one immutable static `.pngr` file:

```text
64-byte PNGRNG01 header
variable root manifest
versioned extension directory with default locus and summary descriptors
contiguous fixed 4 KiB arithmetic directory pages
independently compressed PNGRGN01 regional payloads
independently compressed named-locus and summary pages
```

Each reference manifest records its real sample and contig, coordinate span,
base window size, arithmetic bucket span, directory-page range, entry count,
and one payload codec. A query computes the necessary page numbers directly
from its coordinates. Adaptive subchunks remain inside their parent bucket, so
one page can name up to 72 payloads without another index level. Each entry
contains BLAKE3-128 over the exact encoded payload for verification before
decompression.

The root and some first directory pages normally fit in the 16 KiB bootstrap.
After directory lookup, selected compressed payloads are fetched in one parallel
round. All file offsets and lengths are unsigned 64-bit values; TypeScript keeps
them as `bigint`.

## Default viewer indexes

The normal encoder emits two optional-in-the-decoder-sense extensions. The
named-locus descriptor uses sorted key fences so exact or prefix search reads a
small descriptor and only matching leaves. `--annotations <GFF3>` populates it;
without external annotations it is a valid empty index. The annotation SHA-256
and filename are embedded, and coordinates are explicitly bound to a real
reference sample.

The summary pyramid starts at `window_size * 64` bases and increases by four
per level until one bin covers a reference fragment. Each requested scale is
one independently compressed/checksummed series page. Counters are exact sums
of accepted core tiles: covered bases, tile and byte counts, and tile-local
node, edge, packed-record, and occurrence totals. They are not sample
frequencies or globally unique graph counts.

## Regional representation

The only v1 payload is the record-preserving `PNGRGN01` representation. It
stores:

- sorted node identifiers with forward sequence bytes;
- canonical oriented topology edges, including boundary edges;
- the real reference sample, contig, haplotype, fragment, coordinate anchor, and
  oriented occurrence;
- both packed local GBWT records for every node.

The writer does not enumerate, materialize, or sort every anonymous path.
Readers reconstruct local walks from the selected packed records, keep the real
reference path separately, collapse identical anonymous oriented traversals,
and expose exact integer weights under
`anonymous-distinct-weighted-tile-paths`.

Anonymous traversal evidence remains owned by its source tile. Multi-tile
queries merge graph topology and the real reference walk, but never stitch
anonymous traversals into synthetic individuals.

## Direct, bounded construction

The encoder writes a temporary sibling of the destination and atomically
renames it only after validation. It writes a provisional header/root, reserves
the exact directory span, appends accepted payloads in deterministic
reference/coordinate order, and backfills metadata. There is no global
row-per-visit index, payload spool, second full-object copy, or global pending
entry sort.

Tile selection/materialization and compression use bounded work controlled by
`--threads` and `--max-queued-bytes`. Worker completion order cannot change
archive ordering. Exact borrowed-record lengths allow an oversized parent to be
split before copying a second raw payload corpus. Progress reports time to
first payload, coordinate-based percentage, throughput, ETA, queue bounds,
scratch bytes, and output size.

## Read and correctness behavior

Rust and TypeScript both:

1. validate the v1 header, root, and registered extension descriptors;
2. compute fixed directory pages arithmetically;
3. reject invalid counts, offsets, codecs, padding, or object ranges;
4. verify encoded payload BLAKE3-128, then decompress to the exact declared length;
5. decode local records and reconstruct weighted traversal evidence;
6. assemble selected topology and the real reference traversal;
7. produce the same domain-separated v1 canonical result hash;
8. validate every known extension child range and checksum before atomic rename.

The Rust verifier independently extracts the same source interval from GBZ and
compares topology/reference semantics. It also re-extracts every selected tile
to compare its complete anonymous weighted traversal multiset and provenance.
A performance result is invalid if either gate fails.

## Retained measurements

Older experiment names and report paths preserve the identifiers used when the
measurements were captured. They are historical evidence, not supported format
versions. The most relevant reports are:

- [arithmetic directory and streaming writer smoke](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-25-mhc-v3-streaming-smoke-final/REPORT.md);
- [tile-local haplotype semantics smoke](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-25-mhc-v4-local-haplotypes-smoke-final/REPORT.md);
- [record-preserving encoder and full-source run](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-25-record-preserving-v4/REPORT.md);
- [TypeScript/Rust reader conformance](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-25-typescript-reader-conformance/REPORT.md);
- [v1 release-candidate audit and whole-HPRC rerun](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-26-format-v1-release-candidate/REPORT.md).

The format reset changes magic/version bytes and canonical hash domain strings,
not the recorded performance conclusions. Old research archives are
intentionally unsupported and must be regenerated.

## Current limitations

- GFF3 named-locus ingestion currently sorts the selected searchable records in
  memory; a disk-backed external sort is required before very large annotation
  corpora should be treated as bounded-memory inputs.
- The 72-entry dense-bucket limit has exact 72/73 tests; broader
  pathological-locus and retained fuzz campaigns remain release gates.
- The current regional occurrence safety limit is 16,777,216 per tile; adaptive
  splitting must keep tiles below it.
- Anonymous haplotypes have no cross-tile sample identity.
- Whole-genome validation remains an explicit opt-in measurement, never a CI
  task.
