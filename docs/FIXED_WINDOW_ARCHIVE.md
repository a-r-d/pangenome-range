# Fixed-window archive v1 (Candidate 0)

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
│ flat regional directory      │
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

The v1 header is exactly 64 bytes.

| Byte range | Type | Meaning |
|---|---|---|
| `0..8` | `[u8; 8]` | Magic: `PNGRNG01` |
| `8..12` | `u32` | Archive version: `1` |
| `12..16` | `u32` | Header length: `64` |
| `16..24` | `u64` | Directory offset: `64` |
| `24..32` | `u64` | Directory length in bytes |
| `32..40` | `u64` | Directory-entry count |
| `40..48` | `u64` | First data byte: `64 + directory_length` |
| `48..64` | 16 bytes | Reserved, currently zero |

The current reader validates the magic, version, and header length. It reads
the directory directly after the header and does not yet validate the stored
directory or data offsets. Those fields make the intended layout explicit, but
they must not yet be treated as relocatable-section support.

## Regional directory

The directory begins with a `u64` entry count, followed by that many entries.
The count must match the count in the archive header. Entries are sorted by
`(sample, contig, start, end)`.

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
flat array and the reader scans it linearly; the sort order is not yet used for
a binary or hierarchical lookup.

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
separately encoded directory is sorted by its lookup key.

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

1. Read the first `min(16 KiB, object_length)` bytes. Decode the header and flat
   directory. If the directory extends past 16 KiB, fetch its remainder in an
   additional dependent read.
2. Select directory entries whose sample and contig match and whose interval
   overlaps the requested interval:
   `entry.start < query.end && entry.end > query.start`.
3. Remove bytes already present in the bootstrap, collapse identical physical
   chunk ranges, and coalesce nearby ranges using the configured gap.
4. Fetch the remaining data ranges as one logical parallel data round.
5. Independently decompress and decode each unique chunk, then merge nodes,
   edges, paths, and reference visits by their original identifiers.
6. Use overlapping reference visits as seeds and traverse graph sides up to the
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

## Limits and next experiment

These numbers are directional, not a general performance claim:

- the fixture contains only two roughly 13--14 kb loci;
- 256 KiB, 1 MiB, and 4 MiB windows therefore cover effectively the same data,
  so 256 KiB is not yet a proven optimum;
- the local timings do not control cold/warm page cache and are not browser or
  object-store timings;
- the tiny fixture cannot exercise 100 kb or 1 Mb ranges; the MHC run exercised
  both, but the planned 10,000-query load remains outstanding;
- a flat directory that fits in the 16 KiB bootstrap will not scale to a whole
  genome;
- path metadata and halo duplication may dominate at chromosome scale;
- chunks do not preserve compressed GBWT records;
- there are no per-section checksums, authentication, corruption recovery, or
  stable forward-migration rules yet.

The next high-information test is one HPRC chromosome, with a hierarchical
directory and a GBZ-record-preserving candidate tested beside this materialized
encoding. See the retained reports for the
[tiny-locus sweep](../results/2026-08-25-fixed-window-c0-final/REPORT.md) and the
[medium MHC sweep](../results/2026-08-25-mhc-fixed-window-c0-full/REPORT.md),
including workload qualifications and raw result-file locations.
