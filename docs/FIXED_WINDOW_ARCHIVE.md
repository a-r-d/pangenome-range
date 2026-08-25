# Fixed-window archive v2 (Candidate 1)

Status: research prototype. This document describes the format implemented by
`pangenome-range-build`, not a stable or production-ready interchange format.

## Purpose

The fixed-window archive turns a pangenome graph into independently decodable
regional objects. A reader first fetches a small directory, uses reference
coordinates to find the relevant objects, and then fetches only those objects.

The design is deliberately shaped for static object storage and HTTP range
requests:

- the object starts with all information needed to locate regional data;
- regional chunks are compressed independently;
- a query normally needs one bootstrap read followed by one logical data round;
- exact duplicate regions can share one physical chunk without introducing a
  dependency between different chunks.

The implemented object layout is:

```text
byte 0
┌──────────────────────────────┐
│ 64-byte archive header       │
├──────────────────────────────┤
│ compact root directory       │
├──────────────────────────────┤
│ regional directory pages     │
├──────────────────────────────┤
│ independently compressed     │
│ regional chunk 0             │
├──────────────────────────────┤
│ regional chunk 1             │
├──────────────────────────────┤
│ ...                          │
└──────────────────────────────┘
```

All offsets and intervals are half-open. All integers are unsigned and
little-endian. Variable-length byte strings use a `u64` byte length followed by
that many bytes; strings use the same representation and must be UTF-8.

## Archive header

The v2 header is exactly 64 bytes.

| Byte range | Type | Meaning |
|---|---|---|
| `0..8` | `[u8; 8]` | Magic: `PNGRNG02` |
| `8..12` | `u32` | Archive version: `2` |
| `12..16` | `u32` | Header length: `64` |
| `16..24` | `u64` | Root-directory offset: `64` |
| `24..32` | `u64` | Root-directory length in bytes |
| `32..40` | `u64` | Total leaf-directory entry count |
| `40..48` | `u64` | First regional-payload byte, after all directory pages |
| `48..64` | 16 bytes | Reserved, currently zero |

The current reader validates the magic, version, and header length. It reads
the root directly after the header and validates that every referenced leaf page
is ordered, nonempty, and contained before the first regional payload.

## Root directory

The root begins with a `u64` page count. Each page descriptor contains:

| Field | Type | Meaning |
|---|---|---|
| `sample` | UTF-8 string | Reference sample name |
| `contig` | UTF-8 string | Reference contig name |
| `start` | `u64` | First coordinate covered by the page |
| `end` | `u64` | Exclusive last coordinate covered by the page |
| `page_offset` | `u64` | Absolute archive offset of the leaf page |
| `page_length` | `u64` | Encoded leaf-page length |
| `entry_count` | `u64` | Directory entries stored in the page |

The builder targets at most 4 KiB per leaf page (except an unavoidable single
oversized entry) and never mixes reference sample/contig pairs in one page. The
root is scanned in memory after the initial 16 KiB bootstrap; only overlapping
leaf pages are decoded.

## Regional directory page

Each leaf page begins with a `u64` entry count, followed by that many entries.
The count must match its root descriptor, and all leaf counts must sum to the
total in the archive header. Entries are globally sorted by
`(sample, contig, start, end)` before paging.

Each entry is encoded as:

| Field | Type | Meaning |
|---|---|---|
| `sample` | UTF-8 string | Reference sample name |
| `contig` | UTF-8 string | Reference contig name |
| `start` | `u64` | Inclusive reference-coordinate start |
| `end` | `u64` | Exclusive reference-coordinate end |
| `archive_offset` | `u64` | Absolute offset of the compressed chunk |
| `compressed_length` | `u64` | Stored payload length |
| `uncompressed_length` | `u64` | Expected decoded payload length |
| `codec` | `u8` | `0`: none, `1`: zstd-1, `3`: zstd-3, `6`: zstd-6 |
| reserved | 7 bytes | Currently zero |

More than one directory entry may point to the same physical payload. This is
how exact chunk deduplication is represented. The directory is currently a
paged sorted array; root and selected-page scans are still linear within their
small bounded collections.

## Regional chunk

After decompression, each regional chunk contains an independently decodable
regional graph materialization:

```text
magic                 [u8; 8] = PNGRGN01
version               u32 = 1
flags                 u32 = 0
node_count            u64
edge_count            u64
path_count            u64
reference_visit_count u64
nodes[node_count]
edges[edge_count]
paths[path_count]
reference_visits[reference_visit_count]
```

### Node

```text
node_id   u64
sequence  byte string
```

### Oriented node

```text
node_id   u64
reverse   u8       # 0 = forward, nonzero = reverse
```

### Edge

```text
from      oriented node
to        oriented node
```

Only canonical edge orientations are stored. The reader derives the reverse
counterpart when it constructs adjacency. An edge may reference one endpoint
whose node record is stored in a neighboring chunk; the construction and query
sections describe how those boundary edges are handled.

### Path

```text
original_path_id   u64
haplotype          u64
original_fragment  u64
is_reference       u8
sample             UTF-8 string
contig             UTF-8 string
visit_count        u64
visits[visit_count]
```

Each path visit is:

```text
original_visit_index  u64
node                  oriented node
```

Keeping original path and visit identifiers lets chunks be merged without
inventing chunk-local path order. Consecutive selected visits are reconstructed
as path segments; gaps remain separate segments. Path multiplicity is retained.

### Reference visit

```text
path_id      u64
visit_index  u64
start        u64
end          u64
node         oriented node
```

Reference visits attach absolute half-open coordinates to oriented node visits.
They seed exact interval selection after one or more regional chunks have been
decoded.

## Construction

For every reference path, the builder:

1. aligns fixed windows to multiples of the configured window size and clips
   them to the real path extent;
2. asks the upstream GBZ-base subgraph selector for the window plus a 100-base
   graph-context halo;
3. materializes selected node sequences, canonical oriented edges incident to
   those nodes, every original path visit touching those nodes, and
   coordinate-bearing reference visits;
4. serializes that regional graph and compresses it independently;
5. creates a coordinate directory entry pointing at the stored payload.

The halo deliberately duplicates graph material around window boundaries so a
query with up to 100 bases of context can be reconstructed without fetching an
unbounded neighboring region. It is a construction/query contract: the current
reader rejects a requested context larger than 100.

An edge crossing a regional boundary is retained even when its other endpoint
is not present in that chunk. After chunks are merged, the reader activates the
edge for context traversal only if both endpoint nodes are present. This keeps a
small query from following a dangling edge while preserving topology when a
larger query fetches both endpoint regions.

Physical chunks are written in source reference-path and coordinate order. The
root and leaf directories precede those chunks; leaf pages follow their sorted
coordinate order.

## Exact chunk deduplication

Before compression, the builder computes a BLAKE3 hash of each serialized
regional graph. A hash match is confirmed by comparing the complete byte
payload. If deduplication is enabled, an exact match reuses the earlier physical
chunk; only another directory entry is written.

This is content sharing, not delta compression. Each unique physical chunk
remains independently decodable, so fetching one region never requires fetching
another region as a dictionary or base object.

On the current fixture, two repeated entries were eliminated. That reduced the
archive from 200,492 to 100,430 bytes, saving 100,062 bytes (49.9%) without
changing bytes fetched per query. This unusually large saving may be specific
to the small fixture and its identical regional payloads.

## Query algorithm

The reader performs these steps:

1. Read the first `min(16 KiB, object_length)` bytes and decode the header plus
   compact root directory. If the root itself extends past 16 KiB, fetch only
   its remainder.
2. Select root descriptors overlapping the requested sample, contig, and
   interval. Decode a selected leaf directly from the bootstrap when it is
   already resident there; otherwise fetch only the missing part of that leaf
   in one logical directory round.
3. Select leaf entries whose interval overlaps the requested interval:
   `entry.start < query.end && entry.end > query.start`.
4. Remove bytes already present in the bootstrap, collapse identical physical
   chunk ranges, and coalesce nearby ranges using the configured gap.
5. Fetch the remaining data ranges as one logical parallel data round.
6. Independently decompress and decode each unique chunk. Adopt the first
   decoded regional graph directly; only later chunks pay the ordered-map/set
   merge cost for nodes, edges, paths, and reference visits.
7. Use overlapping reference visits as seeds and traverse graph sides up to the
   requested context distance. Emit only the exact requested canonical
   subgraph, including node sequences, oriented edges, path traversals and
   multiplicity, and the reference interval.

The prototype uses local positioned file reads. Its dependency-round model
represents how an HTTP range reader could schedule the requests, but an actual
HTTP `RangeSource` and browser benchmark have not yet been implemented.

## Measured result versus GBZ-base

The retained experiment ran 202 deterministic queries over the tiny MICB and
KIR3DL1 fixture. Every candidate result matched the source oracle for nodes,
sequences, edges, path multiplicity/orientation, and reference coordinates.

| Local query metric | GBZ-base | Fixed-window archive | Speedup |
|---|---:|---:|---:|
| Median (p50) | 14.40 ms | 10.66 ms | 1.35x |
| Mean | 16.81 ms | 8.64 ms | 1.95x |
| Slow tail (p95) | 52.36 ms | 14.66 ms | 3.57x |
| Very slow tail (p99) | 60.80 ms | 15.14 ms | 4.02x |

In simple terms: on this fixture the archive was about twice as fast on average,
and roughly three-and-a-half times as fast when comparing the slowest 5% of
queries. The median improvement was more modest at about 35%.

The more important architectural difference is the access shape. A typical
archive query used two positioned reads and fetched at most 76,605 bytes at p95:
one bootstrap read and one data read. A representative read-only GBZ-base MICB
query made 1,349 `pread64` calls, including many repeated SQLite header checks
and non-sequential accesses. That does **not** mean the archive is 1,349 times
faster, and local system calls cannot be converted directly into HTTP round
trips. It shows why a database page-access pattern is awkward for a static
remote object while the archive naturally maps to a small number of range
requests.

The selected deduplicated archive was also 100,430 bytes versus 172,032 bytes
for GBZ-base on this fixture. It was still 1.359x the original 73,920-byte GBZ.

### Medium MHC validation

The 4,511,832-byte MHC fixture makes the size dependence clearer. A full sweep
built 20 window/compression layouts and measured 40 deterministic queries at
all six coalescing gaps: 4,800 candidate rows in total, all canonically exact.
The latency-first point was 256 KiB with zstd-3. Its 4,273,073-byte archive was
0.947x the source GBZ, and its p95 request shape was two reads and 1,700,198
bytes (86.34 ms in the simulated 20 ms / 300 Mbps profile).

Local p95 performance crossed over with query size:

| Query size | Fixed-window versus GBZ-base p95 |
|---:|---:|
| 1 kb | 74.35x slower |
| 10 kb | 6.92x slower |
| 100 kb | 1.72x faster |
| 1 Mb | 3.66x faster |

In simple terms, this materialized prototype has substantial fixed decode and
reconstruction overhead for tiny ranges, but pulls ahead once GBZ-base must
assemble larger subgraphs. Unlike the tiny fixture, none of the 20 MHC layouts
contained exact duplicate regional payloads, so the sweep did not add an
unjustified deduplication follow-up.

### Candidate 1 small-query smoke

The v2 two-level directory was then exercised with the 16 KiB/zstd-3 layout on
the same 40 deterministic MHC queries and all six coalescing gaps. All 240
candidate rows remained canonically exact. The 5,776,885-byte archive contains
304 regional chunks, a 486-byte root, and six leaf-directory pages; the complete
header/root/leaves occupy 23,942 bytes.

At 64 KiB coalescing, 1 kb p95 local time was 1.00 ms versus 0.46 ms for
GBZ-base (2.16x slower), while 10 kb p95 was 6.30 ms versus 15.38 ms (2.44x
faster). Compared with the earlier flat-directory 16 KiB run, the 1 kb p95 fell
from 1.47 ms and p95 bytes fell from 45,750 to 39,112; the 10 kb p95 fell from
7.55 ms to 6.30 ms. Cache state remains uncontrolled, so these cross-run timing
differences are directional rather than a release-grade microbenchmark.

The 1 kb query is still exact; 16 KiB describes storage granularity, not the
returned interval. The root selects a 16 KiB regional payload, and canonical
reconstruction trims the result back to the requested coordinates and context.

## Limits and next experiment

These numbers are directional, not a general performance claim:

- the fixture contains only two roughly 13--14 kb loci;
- 256 KiB, 1 MiB, and 4 MiB windows therefore cover effectively the same data,
  so 256 KiB is not yet a proven optimum;
- the local timings do not control cold/warm page cache and are not browser or
  object-store timings;
- the tiny fixture cannot exercise 100 kb or 1 Mb ranges; the MHC run exercised
  both, but the planned 10,000-query load remains outstanding;
- the two-level root is compact on MHC, but will itself eventually require a
  higher-level or searchable representation at whole-genome scale;
- path metadata and halo duplication may dominate at chromosome scale;
- chunks do not preserve compressed GBWT records;
- there are no per-section checksums, authentication, corruption recovery, or
  stable forward-migration rules yet.

The next high-information test is one HPRC chromosome, with a hierarchical
directory and a GBZ-record-preserving candidate tested beside this materialized
encoding. See the retained reports for the
[tiny-locus sweep](../results/2026-08-25-fixed-window-c0-final/REPORT.md) and the
[medium MHC sweep](../results/2026-08-25-mhc-fixed-window-c0-full/REPORT.md),
plus the [Candidate 1 small-query smoke](../results/2026-08-25-mhc-two-level-w16k-smoke-c1/REPORT.md),
including workload qualifications and raw result-file locations.
