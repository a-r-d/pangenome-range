# Pangenome Range File Format v1

Status: normative for the current unreleased, pre-v1 implementation.

This document is the complete contract for a `.pngr` object. The Rust encoder
and reader and the TypeScript reader must implement this document. Other design
documents explain motivation and measurements but do not override it.

## 1. Conventions

- All integers are unsigned and little-endian.
- `u8`, `u32`, and `u64` occupy 1, 4, and 8 bytes respectively.
- Every offset is an absolute byte offset from the start of the `.pngr` object.
- Every interval is half-open: `[start, end)`.
- Every length-prefixed byte string is `u64 byte_length` followed by exactly
  that many bytes. Text strings use the same representation and must be valid
  UTF-8.
- All additions and multiplications described below are checked operations.
  Overflow is corruption, not wrapping arithmetic.
- Node identifiers, coordinates, counts, offsets, and lengths are logically
  `u64`. JavaScript readers keep raw object offsets as `bigint` and convert a
  value to `number` only after proving it is a safe integer and the receiving
  API requires a number.

## 2. Object layout

The object is the following concatenation:

```text
64-byte archive header
variable-length root index
one or more contiguous 4096-byte directory pages per reference manifest
compressed or uncompressed regional payload byte ranges
```

The root starts at offset 64. Directory pages start immediately after the root,
are ordered by manifest and then bucket, and end at `data_offset`. Payloads begin
at or after `data_offset`. Two directory entries may reference the exact same
payload range when deterministic payload deduplication is enabled.

## 3. Archive header

The header is exactly 64 bytes.

| Offset | Size | Type | Field | Required value or meaning |
| ---: | ---: | --- | --- | --- |
| 0 | 8 | bytes | magic | ASCII `PNGRNG01` |
| 8 | 4 | `u32` | version | `1` |
| 12 | 4 | `u32` | header length | `64` |
| 16 | 8 | `u64` | root offset | `64` |
| 24 | 8 | `u64` | root length | byte length of the root index |
| 32 | 8 | `u64` | entry count | total directory entries across all manifests |
| 40 | 8 | `u64` | data offset | first byte after all directory pages |
| 48 | 16 | bytes | reserved | all zero |

An archive with any other magic or version is unsupported. Readers must not
probe for or reinterpret older research formats.

## 4. Root index and reference manifests

The root index begins with `u64 manifest_count`. Its current hard limit is
16 MiB. It is followed immediately by
that many variable-length manifests:

| Order | Type | Field |
| ---: | --- | --- |
| 1 | byte string | reference sample; non-empty UTF-8 |
| 2 | byte string | reference contig; non-empty UTF-8 |
| 3 | `u64` | reference start |
| 4 | `u64` | reference end |
| 5 | `u64` | grid start |
| 6 | `u64` | base window size |
| 7 | `u64` | directory bucket span |
| 8 | `u64` | absolute offset of the first directory page |
| 9 | `u64` | directory page count |
| 10 | `u64` | directory entry count for this manifest |
| 11 | `u8` | payload codec |
| 12 | 7 bytes | reserved; all zero |

Valid codec bytes are:

| Code | Meaning |
| ---: | --- |
| 0 | stored bytes, no compression |
| 1 | zstd level 1 |
| 3 | zstd level 3 |
| 6 | zstd level 6 |

For the current encoder, `bucket_span = window_size * 32`. A reader uses the
stored value and does not silently substitute that default.

For every manifest:

- `start < end`, `grid_start <= start`, and both span values are nonzero;
- `page_count = ceil((end - grid_start) / bucket_span)` and is nonzero;
- its first page begins where the preceding root or manifest page range ended;
- its page range is `page_count * 4096` bytes and ends no later than
  `data_offset`.

The final manifest page range must end exactly at `data_offset`. Manifest entry
counts must sum exactly to the archive header entry count. The root decoder must
consume exactly `root_length` bytes; trailing root bytes are corruption.

## 5. Fixed directory pages

Every directory page is exactly 4096 bytes. It represents one arithmetic
coordinate bucket and can contain at most 102 entries.

### Page header

| Offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 4 | `u32` | entry count, 0 through 102 |
| 4 | 4 | `u32` | entry size, exactly `40` |
| 8 | 8 | `u64` | bucket start |

For bucket index `i`, the page offset and expected coordinate are:

```text
page_offset  = first_page_offset + i * 4096
bucket_start = grid_start + i * bucket_span
bucket_end   = bucket_start + bucket_span
```

### Directory entry

Entries begin at page offset 16. Every entry is 40 bytes.

| Relative offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 8 | `u64` | core start |
| 8 | 8 | `u64` | core end |
| 16 | 8 | `u64` | absolute payload offset |
| 24 | 8 | `u64` | compressed length |
| 32 | 8 | `u64` | uncompressed length |

The core interval must be non-empty, begin within the bucket, and end no later
than `min(bucket_end, manifest.end)`. Both lengths are nonzero. The payload range
must start at or after `data_offset` and end within the object. Bytes after the
last entry through the end of the 4096-byte page are zero padding.

Adaptive splitting may produce multiple entries inside one arithmetic bucket.
The page capacity is therefore a hard format limit: an encoder must split or
fail before emitting a 103rd entry.

## 6. Directory lookup

For a query interval `[q_start, q_end)` against one manifest, intersect it with
the manifest interval and compute:

```text
first_bucket = floor((max(q_start, start) - grid_start) / bucket_span)
last_bucket  = floor((min(q_end, end) - 1 - grid_start) / bucket_span)
```

Fetch each page in that inclusive bucket range. Select every entry satisfying
`entry.start < q_end && entry.end > q_start`. Duplicate entries that reference
the same physical payload range are decoded once, while their directory
coverage remains independently meaningful.

A normal remote query has three dependency stages: bootstrap header/root,
arithmetic directory lookup, then one parallel round for the selected payload
ranges. A 16 KiB bootstrap read may already contain some required directory
bytes and should be reused.

## 7. Payload compression

The manifest codec applies to every payload referenced by that manifest.
Codecs 1, 3, and 6 contain one standard zstd frame whose declared content size
and decoded byte length equal the directory entry's uncompressed length. Codec
0 stores the regional payload directly and therefore requires compressed and
uncompressed lengths to match.

Readers reject unknown codecs, truncated frames, decompression failure, output
larger than configured safety bounds, and decompressed-length mismatch.

## 8. Regional payload header

After decompression, every regional payload begins with the following fixed
128-byte prefix:

| Offset | Size | Type | Field | Required value or meaning |
| ---: | ---: | --- | --- | --- |
| 0 | 8 | bytes | magic | ASCII `PNGRGN01` |
| 8 | 4 | `u32` | version | `1` |
| 12 | 4 | `u32` | flags | `1` |
| 16 | 1 | `u8` | haplotype semantics | `2` |
| 17 | 7 | bytes | reserved | all zero |
| 24 | 8 | `u64` | node count | local nodes |
| 32 | 8 | `u64` | edge count | canonical local/boundary edges |
| 40 | 8 | `u64` | record count | exactly `node_count * 2` |
| 48 | 8 | `u64` | total occurrences | sum over record occurrence counts |
| 56 | 8 | `u64` | core start | tile provenance |
| 64 | 8 | `u64` | core end | tile provenance |
| 72 | 8 | `u64` | construction context | currently exactly `100` |
| 80 | 8 | `u64` | reference haplotype | real reference path metadata |
| 88 | 8 | `u64` | reference fragment start | real reference path metadata |
| 96 | 8 | `u64` | reference query offset | offset from fragment start to core start |
| 104 | 8 | `u64` | reference node offset | base offset within the anchored node |
| 112 | 8 | `u64` | reference handle | anchored packed oriented handle |
| 120 | 8 | `u64` | reference occurrence | anchored offset in that handle's record |

The fixed prefix is followed by two byte strings: reference sample and reference
contig. Both are non-empty UTF-8. The core interval is non-empty and
`reference_fragment_start + reference_query_offset = core_start`.

The only on-disk semantics label in v1 is
`anonymous-distinct-weighted-tile-paths`, encoded as byte `2`. Byte `1` is not a
supported v1 payload mode; “all traversals” remains an internal encoder oracle,
not an archive representation.

## 9. Nodes and oriented handles

Nodes follow the reference strings. They are sorted by strictly increasing,
nonzero node identifier. Each node is:

```text
u64 node_id_delta
u64 sequence_length
sequence_length raw sequence bytes
```

`node_id_delta` is relative to the preceding identifier, initially zero. Empty
node sequences are invalid.

An oriented node handle is packed as:

```text
handle = node_id * 2 + reverse_bit
node_id = handle / 2
reverse = (handle & 1) != 0
```

Handle zero and packing overflow are invalid.

## 10. Edges

The node table is followed by `edge_count` pairs:

```text
u64 from_handle
u64 to_handle
```

Edges use a canonical bidirected orientation so that an edge and its reverse
complement are stored only once. Let `a` be the source node, `b` the target
node, and let `ar` and `br` be their reverse bits. The stored orientation is
canonical exactly when:

```text
ar ? (b > a || (b == a && !br)) : (b >= a)
```

The source node must be local. The target may be outside the payload so boundary
topology survives until adjacent tiles are merged. Duplicate edge pairs are
invalid.

## 11. Packed local GBWT records

The edge table is followed by `record_count` records sorted by strictly
increasing handle:

```text
u64 handle
u64 occurrence_count
u64 record_byte_length
record_byte_length packed record bytes
```

There are exactly two records for every local node, one per orientation. Every
handle refers to a local node, every occurrence count is nonzero, and the sum of
all occurrence counts equals `total_occurrences`. V1 limits a tile to at most
16,777,216 decoded occurrences.

Packed integers inside a record use unsigned base-128 little-endian bytecode.
The low seven bits are payload; bit 7 indicates another byte. Encodings that
overflow `u64` or end with the continuation bit set are invalid.

The record byte stream contains:

1. bytecode `sigma`, the nonzero successor alphabet size;
2. `sigma` pairs of bytecode values: successor-handle delta and initial
   successor offset; reconstructed successor handles are strictly increasing;
3. run-length encoded successor ranks until the record byte string ends.

When `sigma >= 255`, each run is bytecode `rank`, then bytecode
`run_length_minus_one`. Otherwise each run starts with one byte:

```text
rank = byte % sigma
run_length = floor(byte / sigma) + 1
threshold = floor(256 / sigma)
```

If `run_length == threshold`, add one following bytecode integer to the run
length. Every rank is less than `sigma`. Successor offsets advance by one for
each occurrence in a run. The decoded run lengths must sum exactly to the
record's declared occurrence count.

Successor handle zero is an end marker. A nonzero successor whose handle is not
local is a tile-boundary continuation and ends local reconstruction.

## 12. Reference and anonymous traversal reconstruction

Readers reconstruct local traversal evidence from the packed records rather
than reading a materialized path table:

1. Mark each local successor occurrence as having a predecessor.
2. Start a traversal at every occurrence without a local predecessor.
3. Follow successor positions while their handles remain local; reject invalid
   offsets and cycles.
4. Identify the traversal containing `(reference_handle,
   reference_occurrence)` as the real reference traversal and retain its real
   sample, contig, haplotype, fragment, orientation, and coordinates.
5. Keep other traversals only in their canonical orientation, sort them
   lexicographically by packed handles, collapse identical traversals, and store
   their exact multiplicity as `u64` weight.
6. If the reference traversal occurs in the same equivalence group, subtract
   exactly one from that group's anonymous weight. Omit zero-weight groups.

Reference coordinates are reconstructed as:

```text
prefix_length = reference_node_offset
              + sum(sequence lengths before the anchor in the reference walk)
reference_start = reference_fragment_start
                + (reference_query_offset - prefix_length)
reference_end = reference_start + sum(reference traversal sequence lengths)
```

Underflow, overflow, an absent anchor, a reference node offset outside its
sequence, or a reference traversal inconsistent with node lengths is
corruption.

## 13. Tile-local haplotype semantics and multi-tile queries

Anonymous weighted traversals are evidence owned by exactly one tile. They do
not have sample names, global path identifiers, or continuation identities.
They must never be stitched across tiles or described as named individuals.

For a query spanning multiple chunks:

- merge nodes by identifier and require identical sequence bytes;
- merge canonical topology edges globally;
- assemble the real reference walk by coordinate and reject conflicting visits;
- select graph context from the real reference interval through merged topology;
- return anonymous weighted traversals grouped under their source tile;
- include each selected physical tile once, so overlapping construction halos
  are not counted again as a second tile.

The canonical query graph/hash covers selected topology, the assembled real
reference path, and the requested reference interval. Tile-local weighted
traversals have their own provenance-sensitive canonical hash and remain
separate from the globally merged graph hash.

## 14. Reader corruption and allocation rules

A conforming reader fails closed for at least the following:

- unknown archive or regional magic, version, flags, semantics, or codec;
- truncated fixed fields, strings, sections, zstd frames, or bytecode integers;
- nonzero reserved bytes or directory padding;
- invalid UTF-8 or empty required identities;
- arithmetic overflow, unsafe JavaScript integer conversion, and ranges outside
  the object;
- roots or directory pages that overlap, have gaps, or disagree with header
  counts and `data_offset`;
- zero or impossible intervals and lengths;
- directory counts over 102, record/node count disagreement, occurrence-total
  disagreement, duplicate nodes/edges/records, or unsorted delta-coded data;
- a payload whose core interval differs from its directory entry;
- decompressed-length mismatch or trailing regional/root bytes;
- invalid local handles, edge orientation, GBWT ranks/runs/offsets, cycles, or
  reference anchors.

Counts must be checked against remaining bytes before allocation. Implementations
also apply explicit byte limits to roots, compressed chunks, decompressed chunks,
caches, and full-response fallbacks. A declared count never justifies an
unbounded allocation.

## 15. HTTP range-source requirements

A remote `.pngr` is an immutable static object. A range source must:

- issue exact `Range: bytes=start-end` requests;
- require `206 Partial Content`, a matching `Content-Range`, and the exact body
  length;
- keep a stable strong `ETag` or equivalent object identity across requests;
- propagate `AbortSignal` through open and query reads;
- reject an origin that ignores Range and returns `200`, unless the entire
  object is below an explicit caller-supplied full-response byte cap;
- never silently download a multi-gigabyte object.

Public origins must permit CORS and expose `Accept-Ranges`, `Content-Range`,
`Content-Length`, and `ETag`. Immutable content-addressed objects should use
`Cache-Control: public, max-age=31536000, immutable, no-transform` and must not
be transparently compressed or transformed in transit.

## 16. Worked tiny example and fixtures

`test-data/conformance/format-v1.pngr` is the authoritative synthetic fixture.
It contains nodes 1 (`A`) and 2 (`C`), edge `(2, 4)`, reference
`GRCh38#chr1:[100,102)`, and one anonymous traversal `[2,4]` of weight 1.

Its principal offsets are:

| Section | Offset | Length |
| --- | ---: | ---: |
| archive header | 0 | 64 |
| root index | 64 | 106 |
| directory page | 170 | 4096 |
| zstd-3 payload | 4266 | 122 |
| decompressed regional payload | n/a | 316 |

The header declares one entry and `data_offset = 4266`. The directory entry is
`[100,102)`, payload offset 4266, compressed length 122, and uncompressed length
316. The regional prefix declares two nodes, one edge, four oriented records,
and eight total occurrences. Exact bytes, SHA-256 checksums, decoded JSON, and
the shared canonical hash are retained in
`test-data/conformance/manifest.json`.

The same directory also contains all independent pieces needed for another
implementation: header, root, directory page, raw payload, and zstd levels 1,
3, and 6. `micb-kir3dl1-reader-v1.pngr` is the small GBZ-derived integration
fixture used for real local HTTP `206` range tests.

## 17. Pre-release version policy

This project is unreleased and pre-v1. The current file format is named v1 and
uses only `PNGRNG01` and `PNGRGN01`.

Until the project deliberately declares a stable public format, an incompatible
format change replaces this v1 specification, implementation, and fixtures in
place. It does not create a v2 compatibility stack. Old research archives are
intentionally unsupported and must be regenerated with the current encoder.

The npm/Cargo package semantic version is independent of the on-disk format
version. Package releases may advance while the current pre-release file format
continues to identify itself as v1.
