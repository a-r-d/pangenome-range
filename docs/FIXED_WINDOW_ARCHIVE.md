# Fixed-window archive v4 (Candidate 1)

Status: research prototype. This is the format currently emitted by
`pangenome-range-build`. Rust reads the complete archive and Rust/TypeScript
share the current regional decoder and golden fixtures, but the browser archive
transport is not yet a stable interchange implementation.

## Design goals

Archive v4 is a static-object layout for HTTP range access to regional
pangenome subgraphs. It has five properties needed before a multi-gigabyte
experiment:

- construction retains compact metadata plus a bounded number of active tiles;
- the root grows with reference paths, not with chunks or leaf pages;
- leaf-page offsets are computed directly from reference coordinates;
- oversized regional payloads split recursively until they satisfy a byte cap;
- regional payloads preserve exact compressed local GBWT records plus topology;
- an open reader retains the bootstrap and a byte-bounded leaf-page cache.

```text
byte 0
┌────────────────────────────────────┐
│ 64-byte archive header             │
├────────────────────────────────────┤
│ arithmetic reference manifests     │  O(reference paths)
├────────────────────────────────────┤
│ fixed 4096-byte directory pages    │  offsets computed from coordinates
├────────────────────────────────────┤
│ independently compressed payloads  │
└────────────────────────────────────┘
```

All offsets and intervals are half-open. Integers are unsigned little-endian.
A byte string is a `u64` length followed by that many bytes; strings use the
same representation and must be UTF-8.

## Archive header

The header is exactly 64 bytes.

| Byte range | Type | Meaning |
|---|---|---|
| `0..8` | `[u8; 8]` | Magic `PNGRNG04` |
| `8..12` | `u32` | Archive version `4` |
| `12..16` | `u32` | Header length `64` |
| `16..24` | `u64` | Root offset `64` |
| `24..32` | `u64` | Root length |
| `32..40` | `u64` | Total directory-entry count |
| `40..48` | `u64` | First regional-payload byte |
| `48..64` | 16 bytes | Reserved, currently zero |

## Arithmetic reference manifest

The root begins with a `u64` manifest count. There is one manifest for each
reference path fragment:

| Field | Type | Meaning |
|---|---|---|
| `sample` | string | Reference sample |
| `contig` | string | Reference contig |
| `start`, `end` | `u64`, `u64` | Real reference extent |
| `grid_start` | `u64` | Window-aligned coordinate at or before `start` |
| `window_size` | `u64` | Base coordinate window |
| `bucket_span` | `u64` | Coordinate span represented by one leaf page |
| `first_page_offset` | `u64` | Absolute offset of bucket zero |
| `page_count` | `u64` | Number of fixed leaf pages |
| `entry_count` | `u64` | Adaptive chunk descriptors across all pages |
| `codec` | `u8` | `0` none, `1` zstd-1, `3` zstd-3, `6` zstd-6 |
| reserved | 7 bytes | Zero |

The implemented bucket span is 32 base windows. For coordinate `x`, a reader
computes:

```text
bucket = floor((x - grid_start) / bucket_span)
page_offset = first_page_offset + bucket * 4096
```

No page list is stored or scanned. A query crossing buckets computes its first
and last bucket and requests that contiguous page range. The builder and reader
validate that manifests are contiguous in archive space and terminate exactly
at the header's payload offset.

## Fixed leaf page

Every directory page is exactly 4096 bytes:

```text
entry_count      u32
entry_size       u32 = 40
bucket_start     u64
entries          entry_count * 40 bytes
zero padding     to 4096 bytes
```

Each entry is five `u64` values:

```text
reference_start
reference_end
archive_offset
compressed_length
uncompressed_length
```

Sample, contig, codec, and bucket bounds are inherited from the manifest rather
than repeated per entry. A page holds 102 descriptors. With 32 base windows per
page this leaves room for more than three adaptive pieces per base window on
average. The builder fails with a specific capacity error instead of silently
creating a non-arithmetic overflow chain.

Multiple entries may point to the same physical payload when exact
deduplication is enabled.

## Regional payload version dispatch

Archive v4 keeps each independently compressed regional payload versioned. A
reader must dispatch on the eight-byte regional magic after decompression; it
must never reinterpret an older payload as the current representation.

| Magic | Regional version | Semantics | Status |
|---|---:|---|---|
| `PNGRGN02` | 2 | globally named paths | Rust read compatibility only |
| `PNGRGN03` | 3 | materialized anonymous weighted tile paths | Rust read compatibility only |
| `PNGRGN04` | 4 | exact local GBWT records reconstructed as anonymous weighted tile paths | emitted format; Rust and TypeScript decode |

The TypeScript decoder explicitly rejects versions 2 and 3. Archive v3
(`PNGRNG03`) is likewise rejected because its named-path semantics are not the
current browser contract.

## Record-preserving regional payload v4

After independent decompression, the currently emitted regional chunk begins:

```text
magic                         [u8; 8] = PNGRGN04
version                       u32 = 4
flags                         u32 = 1
haplotype_semantics           u8 = 2 (distinct weighted anonymous)
reserved                      [u8; 7] = zero
node_count                    u64
topology_edge_count           u64
gbwt_record_count             u64
total_local_occurrences       u64
core_start, core_end          u64, u64
construction_context          u64 (currently 100)
reference_haplotype           u64
reference_fragment_start      u64
reference_query_offset        u64
reference_node_offset         u64
reference_gbwt_handle         u64
reference_occurrence_offset   u64
reference_sample              string
reference_contig              string
```

Sections follow in deterministic order:

1. strictly increasing node IDs, delta-coded as `u64`, followed by
   length-prefixed forward sequences;
2. canonical graph-topology edges as pairs of packed oriented `u64` handles;
3. strictly increasing oriented GBWT handles, each followed by its declared
   occurrence count and one length-prefixed exact compressed GBWT record.

The record bytes are the upstream GBWT adjacency bytecode followed by its
run-length encoded BWT. They are copied exactly; the encoder does not enumerate,
sort, or materialize local haplotype paths. Full graph topology remains a
separate section because GBWT transitions are evidence of indexed traversals,
not a complete substitute for graph edges.

The reference anchor contains a real GBWT handle/occurrence and enough
fragment/query/node offset information to recover the reference traversal and
coordinates. A reader safely decodes local records, marks occurrences with
local predecessors, walks local starts, identifies the anchored reference,
keeps canonical traversal orientation, groups identical traversals, and
subtracts the one reference copy. The result is
`anonymous-distinct-weighted-tile-paths`: exact tile-local multiplicity with
no invented sample or continuation identity.

Counts, byte ranges, varints, run ranks, successor offsets, record ordering,
reference anchors, and trailing bytes are checked before use. Both decoders
limit a tile to 16,777,216 expanded local occurrences before allocation. Edges
whose destination lies outside the tile remain present; they activate only
after both endpoint nodes are assembled.

## Retained compatibility payloads

`PNGRGN03` version 3 stores already-materialized reference and anonymous
weighted traversals. Its nodes, topology edges, packed handles, integer weights,
provenance, and semantics remain readable in Rust, but the normal encoder no
longer emits it.

`PNGRGN02` version 2 is the older named-path representation with local string
dictionaries, global source path IDs, visit indices, and coordinate-bearing
reference visits. It remains a research compatibility decoder only. Neither
compatibility format is silently exposed as the current TypeScript tile model.
## Streaming, bounded construction

The upstream `gbz` crate still deserializes the source GBZ in full. Archive
construction bounds its *additional* state and does not claim lazy source
access.

1. Build compact reference metadata and the upstream `PathIndex`; do not scan
   every haplotype visit or build an occurrence table.
2. Form coordinate-ordered base-window tasks. At most `--threads` tasks select
   local topology with `HaplotypeOutput::None` concurrently.
3. For each selected tile, compute the real reference GBWT anchor, measure the
   exact payload size from borrowed compressed records, and split before copying
   if the byte or occurrence safety cap would be exceeded.
4. Copy forward sequences, canonical topology edges, and exact compressed GBWT
   records into one record payload. No anonymous path is enumerated during
   construction.
5. Consume worker results in original coordinate order, compress bounded
   batches, and append directly to the temporary final archive. Parallel
   completion order cannot affect archive bytes or offsets.
6. Backfill fixed directory pages and the final header, fsync, structurally
   validate every physical payload, and atomically rename. Query-time decoding
   reconstructs weighted paths only for selected tiles.

Adaptive children are inserted before later completed tasks, preserving exact
reference/coordinate order. Raw and compressed queues are bounded by
`--max-queued-bytes`; the default is 256 MiB. The CLI defaults to the available
parallelism capped at eight workers and reports the actual value. Active source
state is one bounded local `Subgraph` per worker plus bounded payload and
compression buffers.

There is no payload spool, second full-file copy, global pending-entry sort, or
source-global occurrence index. Failure cleanup is the default;
`--keep-partial` is explicit.
## Reader and browser mapping

`FixedArchiveReader` keeps the file open, reads the bootstrap once, and has a
1 MiB byte-bounded least-recently-used leaf cache. The retained Rust benchmark
creates one reader per coalescing-gap run and reuses it across that workload.
`query_fixed_archive` remains a cold one-shot compatibility wrapper.

The TypeScript package now shares the bounded `PNGRGN04` decoder and golden
fixture with Rust. It returns typed arrays for node IDs/sequences, topology
edges, the reference traversal, traversal offsets/nodes, and weights. The
archive-open and HTTP directory state machine remains to be implemented:

1. fetch and retain the 16 KiB bootstrap;
2. decode the small manifest root;
3. compute fixed leaf offsets with integer arithmetic;
4. use an explicit byte-bounded leaf cache in addition to the browser HTTP
   cache;
5. issue payload ranges in one parallel dependency round;
6. decode packed sections into structures needed by rendering instead of
   reconstructing Rust `BTreeMap`/`BTreeSet` shapes.

Cold and warm browser phases must be reported separately. Rust local positioned
reads and simulated network profiles are layout evidence, not browser
performance claims.

## MHC v3 smoke result

The retained v3 smoke used `test-data/mhc-10.gbz` (4,511,832 bytes), 16 KiB
windows, zstd-3, an 8 MiB raw cap, and a 1 KiB minimum adaptive interval. All
candidate rows matched the source oracle.

| Metric | v2 two-level | v3 arithmetic/streaming |
|---|---:|---:|
| Archive bytes | 5,776,885 | 4,020,762 |
| Archive / GBZ | 1.280x | 0.891x |
| Header + root | 486 B | 173 B |
| Complete index | 23,942 B | 41,133 B |
| Fixed leaf pages | 6 | 10 |
| Physical chunks | 304 | 304 |
| Construction wall time | 27,964.7 ms | 2,762.4 ms |
| Whole-process peak RSS | 792,904 KiB | 352,516 KiB |

The v3 index spends 17,191 extra bytes on ten fixed pages to make addressing
arithmetic. Packed regional encoding more than offsets that cost, reducing the
archive by 1,756,123 bytes. The temporary path-occurrence index was 41,914,368
bytes, the payload spool was 3,979,629 bytes, and the largest raw/compressed
per-chunk buffers were 1,126,810 / 76,741 bytes. This fixture required no
adaptive splits, so a dense cohort input must still exercise that path.

Phase timings were 843.1 ms for the temporary occurrence index, 366.9 ms for
regional selection, 1,390.6 ms for regional materialization, 34.8 ms for packed
encoding, and 63.4 ms for compression.

See the [retained v3 smoke report](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-25-mhc-v3-streaming-smoke-final/REPORT.md)
and its `summary.json` for query distributions and qualifications.

## MHC v4 local-haplotype smoke result

The retained v4 smoke built a 3,194,336-byte archive (0.708x the 4,511,832-byte
GBZ) in 1,660.006 ms. It wrote its first payload after 11.282 ms, produced 304
chunks, used 3,153,203 bytes of bounded payload-spool scratch, and used zero
occurrence-index bytes. Whole-process peak RSS was 916,932 KiB; that includes
source loading, GBZ-base baseline construction, encoding, and all correctness
queries and is not an encoder-only peak.

On the first 16 KiB tile, `All` emitted 9 anonymous traversals and 504 node
visits in 47,969 raw JSON bytes. `Distinct` emitted 6 traversals with total
weight 9 and the same 504 weighted visits in 40,047 bytes. Exact oriented-walk
aggregation matched. All 40 deterministic 1 kb, 10 kb, 100 kb, and 1 Mb
queries passed graph correctness and fresh exact per-tile weighted comparison;
1 Mb queries selected multiple chunks.

See the [retained v4 smoke report](https://github.com/a-r-d/pangenome-range/blob/main/results/2026-08-25-mhc-v4-local-haplotypes-smoke-final/REPORT.md).

## Remaining limits before a multi-GB claim

- The source GBZ is still deserialized in memory by the upstream library.
- Descriptor and hash metadata still grow with chunk count, though payload
  bytes do not remain in memory.
- The 102-entry fixed-page limit needs a dense-locus exercise; v4 fails closed
  if adaptive splitting overflows a bucket.
- The raw-byte cap bounds accepted payloads, but an oversized parent interval
  is materialized once before splitting. A future estimator or streaming
  serializer could make that transient bound stricter.
- There are no per-section checksums, authentication, corruption recovery, or
  forward-migration rules.
- The TypeScript HTTP-range reader and browser benchmark are next; no browser
  speed claim is made here.
