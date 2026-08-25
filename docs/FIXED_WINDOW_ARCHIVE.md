# Fixed-window archive v3 (Candidate 1)

Status: research prototype. This is the format currently emitted by
`pangenome-range-build`; it is deliberately documented before the TypeScript
browser reader is implemented, but it is not yet a stable interchange format.

## Design goals

Archive v3 is a static-object layout for HTTP range access to regional
pangenome subgraphs. It has five properties needed before a multi-gigabyte
experiment:

- construction retains only metadata and one regional payload at a time;
- the root grows with reference paths, not with chunks or leaf pages;
- leaf-page offsets are computed directly from reference coordinates;
- oversized regional payloads split recursively until they satisfy a byte cap;
- regional payloads use packed numeric fields and local string dictionaries;
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
| `0..8` | `[u8; 8]` | Magic `PNGRNG03` |
| `8..12` | `u32` | Archive version `3` |
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

## Packed regional payload

After independent decompression, a v3 regional chunk begins:

```text
magic                  [u8; 8] = PNGRGN02
version                u32 = 2
flags                  u32 = 1
node_count             u64
edge_count             u64
path_count             u64
reference_visit_count  u64
sample_count           u32
contig_count           u32
```

The sections that follow are suitable for a typed-array-oriented TypeScript
decoder:

- sorted node IDs are delta-coded and followed by sequence bytes;
- an oriented node is packed as `(node_id * 2) + reverse_bit` in one `u64`;
- edges are pairs of packed oriented nodes;
- sample and contig strings are stored once in local dictionaries;
- paths use `u32` dictionary IDs and delta-code sorted visit indices;
- coordinate-bearing reference visits use fixed numeric fields and one packed
  oriented node.

Original graph path IDs and visit indices are retained. This lets adjacent
chunks merge without inventing chunk-local order and preserves path
multiplicity and orientation in canonical correctness checks.

Boundary edges are stored even if the neighboring node belongs to another
chunk. A reader activates an edge for context traversal only after both
endpoint nodes have been assembled.

## Streaming, bounded construction

The builder still uses the upstream `gbz` crate to deserialize the source GBZ;
v3 bounds the *additional encoder working set* rather than claiming that the
upstream source graph itself is streamed.

1. Scan each original path once into a temporary SQLite occurrence index keyed
   by node ID. Its page cache is fixed at 16 MiB. This replaces the v2 behavior
   that rescanned every complete path for every regional window.
2. For each reference window, ask the upstream selector for nodes in the window
   plus the 100-base construction halo.
3. Look up only occurrences of those nodes, materialize one regional graph,
   encode it, and discard its collections after handling the payload.
4. If the raw payload exceeds `max_uncompressed_chunk_bytes` (8 MiB in current
   presets), split its coordinate interval in half and retry. Splitting stops at
   `min_window_size` (1 KiB); a still-oversized payload fails explicitly.
5. Compress one accepted payload and append it immediately to a temporary
   spool. Retain only its hash, spool offset, and lengths.
6. For a BLAKE3 collision, read and decompress only the candidate spool payload
   to confirm exact equality. Raw payloads are never retained as a corpus.
7. Write the header, root, fixed pages, and stream-copy the payload spool into
   the final archive with `io::copy`.

Encoder memory is proportional to the already-loaded source graph, the bounded
SQLite cache, descriptor/hash metadata, and the largest single regional
raw/compressed payload. It is not proportional to total archive payload bytes.
Temporary disk is proportional to path occurrences plus final payload bytes.

## Reader and browser mapping

`FixedArchiveReader` keeps the file open, reads the bootstrap once, and has a
1 MiB byte-bounded least-recently-used leaf cache. The retained Rust benchmark
creates one reader per coalescing-gap run and reuses it across that workload.
`query_fixed_archive` remains a cold one-shot compatibility wrapper.

The TypeScript reader should mirror this state machine:

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

See the [retained v3 smoke report](../results/2026-08-25-mhc-v3-streaming-smoke-final/REPORT.md)
and its `summary.json` for query distributions and qualifications.

## Remaining limits before a multi-GB claim

- The source GBZ is still deserialized in memory by the upstream library.
- The external occurrence index was about 9.3x the compressed GBZ on MHC;
  temporary disk and index-build throughput need GB-scale measurement.
- Descriptor and hash metadata still grow with chunk count, though payload
  bytes do not remain in memory.
- The 102-entry fixed-page limit needs a dense-locus exercise; v3 fails closed
  if adaptive splitting overflows a bucket.
- The raw-byte cap bounds accepted payloads, but an oversized parent interval
  is materialized once before splitting. A future estimator or streaming
  serializer could make that transient bound stricter.
- There are no per-section checksums, authentication, corruption recovery, or
  forward-migration rules.
- The TypeScript HTTP-range reader and browser benchmark are next; no browser
  speed claim is made here.
