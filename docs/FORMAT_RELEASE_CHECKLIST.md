# File-format v1 release checklist

Status: release-candidate implementation measured on 2026-08-26; stable v1 is
not yet frozen. This checklist maps
every normative group in [`FILE_FORMAT_V1.md`](FILE_FORMAT_V1.md) to evidence.
`PASS` means the Rust and TypeScript reference implementations and retained
tests agree. `POLICY` means an ADR supplies the decision. `OPEN` is a blocker to
declaring stable v1.

## Conventions, layout, and version dispatch

| ID | Normative requirement | Evidence | Status |
| --- | --- | --- | --- |
| V1-C01 | Integers are unsigned little-endian with exact `u8`/`u32`/`u64` widths. | Rust `binary`; TypeScript `BinaryReader`; golden pieces. | PASS |
| V1-C02 | Offsets are absolute; intervals are half-open. | Archive/directory readers and query boundary tests. | PASS |
| V1-C03 | Strings have `u64` byte lengths and valid UTF-8. | Shared invalid-UTF-8 corrupt fixture. | PASS |
| V1-C04 | Non-padded sections are consumed exactly. | Root/regional trailing-byte tests; directory zero-padding checks. | PASS |
| V1-C05 | All arithmetic is checked; offsets remain `bigint` in JavaScript. | Rust checked arithmetic; TypeScript `checkedAdd`/`checkedMultiply`; unsafe-coordinate tests. | PASS |
| V1-L01 | Layout is header, root, contiguous arithmetic pages, then payload ranges. | Shared manifest section offsets; structural validator. | PASS |
| V1-L02 | Directory pages are manifest/bucket ordered and end at `data_offset`. | Both root decoders. | PASS |
| V1-L03 | Exact physical payload reuse is allowed; partial byte overlap is forbidden. | Duplicate-physical-payload and partial-overlap validation tests. | PASS |
| V1-H01 | Header is exactly 64 bytes with `PNGRNG01`, version 1, length/root constants, counts, and `data_offset`. | Shared header fixture and manifest constants. | PASS |
| V1-H02 | Extension pointer fields are both zero or both valid and contiguous after the root. | Shared `corrupt-archive-header-reserved` half-pointer case. | PASS |
| V1-H03 | Unknown magic/version is unsupported and never dispatched to an old decoder. | Typed Rust/TypeScript version tests and corrupt fixtures. | PASS |
| V1-H04 | Any use of header bytes 48..63 is governed by the v1 extension ADR. | Extension-policy ADR. | POLICY |

## Root manifests and directory pages

| ID | Normative requirement | Evidence | Status |
| --- | --- | --- | --- |
| V1-R01 | Root begins with bounded `manifest_count`; decoded root is at most 16 MiB. | Count-before-allocation and root-limit tests. | PASS |
| V1-R02 | Manifest strings are non-empty; numeric fields and codec have exact order/types. | Golden root plus malformed UTF-8/codec fixtures. | PASS |
| V1-R03 | Codec codes are only 0, 1, 3, and 6; stored `bucket_span` is authoritative. | Both decoders and zstd matrix. | PASS |
| V1-R04 | `start < end`, `grid_start <= start`, nonzero spans, and nonzero arithmetic page count. | Both root decoders. | PASS |
| V1-R05 | `page_count` equals the ceiling formula using checked arithmetic. | Both root decoders; overflow tests. | PASS |
| V1-R06 | Page ranges are contiguous, checked, and end exactly at `data_offset`. | Both root decoders and truncated archive fixtures. | PASS |
| V1-R07 | Manifest entry totals equal the header entry count. | Both root decoders and structural validation. | PASS |
| V1-R08 | Root parser consumes exactly `root_length`; reserved bytes are zero. | Both decoders; trailing and reserved tests. | PASS |
| V1-R09 | Exact `(sample, contig, start, end)` intervals are unique; fragmented identities may repeat and all overlapping fragments participate. | Both decoders; whole-HPRC fragmented manifests; duplicate-interval test. | PASS |
| V1-D01 | A page is exactly 4096 bytes with a 16-byte header and no more than 72 56-byte entries. | Exact 72/73 current tests plus explicit obsolete 102/103 model coverage. | PASS |
| V1-D02 | Page offset and bucket coordinate use checked arithmetic. | Both decoders; near-`u64::MAX` tests. | PASS |
| V1-D03 | Entry core is nonempty and contained in its bucket/manifest; lengths are nonzero. | Both decoders and malformed page tests. | PASS |
| V1-D04 | Payload range starts at/after `data_offset`, ends in the object, and obeys configured byte limits. | Structural validator and TypeScript directory decode. | PASS |
| V1-D05 | Entries use nondecreasing tuple order; duplicate rules are explicit. | Rust and TypeScript ordered-page checks. | PASS |
| V1-D06 | All page padding after the declared entries is zero. | Both decoders. | PASS |
| V1-D07 | A 73rd adaptive entry must split or fail before serialization; obsolete 102/103 inputs also reject. | Rust directory-capacity tests. | PASS |

## Lookup, compression, and regional framing

| ID | Normative requirement | Evidence | Status |
| --- | --- | --- | --- |
| V1-Q01 | Lookup first intersects the query and never subtracts one from an empty interval. | Terminal-boundary source-oracle workload and query validation tests. | PASS |
| V1-Q02 | First/last buckets use the normative floor formulas and inclusive page range. | Rust/TypeScript query tests. | PASS |
| V1-Q03 | Selection uses strict half-open overlap. | Boundary and terminal query tests. | PASS |
| V1-Q04 | One physical payload is decoded once while logical coverage is retained. | Exact physical-range registration and payload-cache tests. | PASS |
| V1-Q05 | Bootstrap, arithmetic directory, and parallel payload are the three dependency stages; bootstrap bytes are reusable. | Reader trace/cache tests. | PASS |
| V1-Z01 | Stored payload lengths match exactly. | Both decompressors. | PASS |
| V1-Z02 | Compressed payload is exactly one standard zstd frame with declared content size. | Rust/TypeScript single-frame and trailing-byte tests. | PASS |
| V1-Z03 | Dictionaries, skippable/concatenated frames, reserved bits, truncation, and mismatched size reject. | Rust/TypeScript zstd adversarial matrix. | PASS |
| V1-Z04 | Optional zstd content checksum is validated. | Decoder-library behavior test. | OPEN |
| V1-P01 | Regional prefix is exactly 128 bytes with `PNGRGN01`, version 1, flags 1, semantics 2, and zero reserved bytes. | Golden raw payload plus version/flags/reserved tests. | PASS |
| V1-P02 | Counts, core, context 100, and reference anchor fields occupy the specified offsets. | Machine manifest and golden raw round trip. | PASS |
| V1-P03 | Required reference strings are nonempty UTF-8 and `fragment_start + query_offset == core_start`. | Rust/TypeScript regional decoders. | PASS |
| V1-P04 | Only `anonymous-distinct-weighted-tile-paths` is serialized. | Both decoders reject other semantics. | PASS |
| V1-P05 | Regional parser consumes exactly and bounds counts before allocation. | Truncation/huge-count fixtures and generated-byte tests. | PASS |

## Nodes, edges, and packed records

| ID | Normative requirement | Evidence | Status |
| --- | --- | --- | --- |
| V1-N01 | Nodes are nonzero, strictly increasing delta-coded IDs with nonempty opaque sequences. | Both decoders; duplicate/delta/empty tests. | PASS |
| V1-N02 | Handle packing is `node_id * 2 + reverse_bit`; zero/overflow reject. | Rust/TypeScript oriented-handle tests. | PASS |
| V1-E01 | Edges are handle pairs in canonical bidirected orientation. | Canonical-orientation tests and malformed payload matrix. | PASS |
| V1-E02 | Edge source is local; target may be nonlocal for boundary topology. | Boundary-edge activation test. | PASS |
| V1-E03 | Edge tuples are strictly ordered and duplicate-free; decoders do not normalize malformed input. | Rust/TypeScript edge-order tests. | OPEN |
| V1-G01 | `record_count == node_count * 2`; records are strictly increasing, local, nonzero-occurrence handles. | Both decoders and corrupt-count tests. | PASS |
| V1-G02 | Sum of record occurrences equals header total and does not exceed 16,777,216. | Exact maximum/next-value Rust test and huge-count corrupt fixture. | PASS |
| V1-G03 | Base-128 integers are unsigned, little-endian, terminating, and overflow-checked. | Malformed-varint fixture and fuzz target. | PASS |
| V1-G04 | `sigma` is nonzero; successor deltas/offsets and successor ordering validate. | Packed-record decoder tests/fuzz. | PASS |
| V1-G05 | Both RLE branches implement exact rank/run rules and ranks remain in alphabet. | Golden records and source-oracle tests. | PASS |
| V1-G06 | Decoded run lengths equal occurrence count; packed bytes are consumed exactly. | Rust/TypeScript malformed-record tests. | PASS |
| V1-G07 | Handle zero terminates; a nonlocal successor terminates local reconstruction. | Boundary/terminal traversal tests. | PASS |
| V1-G08 | Encoder emits minimal bytecode; release decoders reject non-minimal forms. | Shared generated non-minimal-varint fixture consumed by Rust and TypeScript. | PASS |

## Reference, anonymous evidence, merge, and hashes

| ID | Normative requirement | Evidence | Status |
| --- | --- | --- | --- |
| V1-T01 | Local predecessors are marked; traversal starts are predecessor-free occurrences. | Packed-record reconstruction unit tests. | PASS |
| V1-T02 | Invalid offsets and cycles fail with bounded work. | Cyclic local reconstruction test and fuzz target. | PASS |
| V1-T03 | Exactly one traversal contains the declared real-reference anchor. | Golden, missing-anchor, and conflicting-anchor tests. | PASS |
| V1-T04 | Anonymous paths use canonical orientation, lexical collapse, and exact `u64` weights. | MHC all-vs-distinct source oracle and golden payload. | PASS |
| V1-T05 | Exactly one matching real reference occurrence is subtracted; zero weights are omitted. | Golden payload and multiplicity tests. | PASS |
| V1-T06 | Fragment/query/node offsets reconstruct checked reference start/end and preserve anchored orientation. | Fragmented and reverse-anchor tests. | PASS |
| V1-M01 | Multi-tile nodes merge by ID with identical sequences; topology edges union canonically. | Rust/TypeScript merge conflict tests. | PASS |
| V1-M02 | Reference visits assemble by coordinate and conflicting visits reject. | Reader merge tests. | PASS |
| V1-M03 | Context selection uses the real reference interval and merged topology. | Rust/TypeScript canonical-hash conformance. | PASS |
| V1-M04 | Anonymous evidence remains per tile and is never stitched or named. | Public types, semantic label, viewer tests. | PASS |
| V1-M05 | Each physical tile is included once; halo evidence is not double-counted. | Physical-range deduplication and keyed payload-cache tests. | PASS |
| V1-M06 | Merge result is independent of request completion order. | Shuffled tile-arrival hash test. | OPEN |
| V1-X01 | Graph hash uses the exact v1 domain, integer/string encoding, sorting, graph/reference fields. | Shared graph hash in manifest consumed by Rust and TypeScript. | PASS |
| V1-X02 | Tile hash uses exact provenance, weights, orientation, and v1 ordering. | Shared tile-local hash in manifest consumed by Rust and TypeScript. | PASS |

## Corruption, resource limits, HTTP, and determinism

| ID | Normative requirement | Evidence | Status |
| --- | --- | --- | --- |
| V1-S01 | Unknown magic/version/flags/semantics/codec and nonzero reserved bytes reject. | Shared corrupt manifest plus both test suites. | PASS |
| V1-S02 | Truncation, invalid UTF-8, overflows, unsafe conversions, gaps, invalid ranges, counts, duplicates, and trailing bytes reject. | Shared/generated adversarial matrix and fuzz targets. | PASS |
| V1-S03 | Payload core must equal its directory core. | Structural validator and reader tests. | PASS |
| V1-S04 | Finite root, encoded/decoded chunk, cache, and full-response limits exist and return no partial result. | Rust/TypeScript limit tests and API defaults. | PASS |
| V1-S05 | Header, root, page, regional, and packed-record fuzz targets remain opt-in; normal CI stays bounded. | `fuzz/` plus CI configuration. | PASS |
| V1-HR1 | HTTP uses exact ranges, requires matching 206/content range/body length, and propagates abort. | `HttpRangeSource` contract tests. | PASS |
| V1-HR2 | Object byte length and strong identity stay stable across requests. | ETag mutation tests. | PASS |
| V1-HR3 | A 200 response is accepted only below an explicit full-response cap; large objects reject. | HTTP fallback tests. | PASS |
| V1-HR4 | Public origin exposes range/identity headers and prevents transformation. | Probe script and hosting documentation; live origin is not a format-freeze prerequisite. | PASS |
| V1-DET1 | Same bytes/options/locks/encoder yield byte-identical output across worker counts. | MHC t1/t4 exact SHA-256 and conformance export repeat. | PASS |
| V1-DET2 | Request/worker completion order cannot affect directory, payload, merge, or hash order. | Encoder t1/t4 plus shuffled reader test. | OPEN |
| V1-VER1 | Pre-stable incompatible changes replace v1 without compatibility decoding; old research archives regenerate. | Version-policy tests and release notes. | PASS |

## Release-policy and whole-source gates

| Gate | Evidence required before stable v1 | Status |
| --- | --- | --- |
| Extension policy | ADR 0001; optional/required behavior implemented in Rust, TypeScript, spec, and fixtures. | PASS |
| Payload integrity | ADR 0002; BLAKE3-128 directory field, full-HPRC occupancy/scan model, MHC encoder and TypeScript query measurements. | PASS |
| Provenance | Embedded and identity-bound, or explicitly external with a secure binding. | OPEN |
| Timing integrity | Schema-3 encoder report plus RC retained result; output SHA-256 is timed and worker CPU is separate. | PASS |
| Atomic validation | Standard gate checks ranges, BLAKE3, exact decompression, and structural decode before rename; full/source modes remain distinct. | PASS |
| Whole HPRC | Candidate validated 363,105 payloads and passed 9/9 graph plus 9/9 tile-set source-oracle queries; MHC t1/t4 proves worker determinism. | PASS |
| Source RAM | Full-load requirement is measured/preflighted and a focused upstream lazy-access issue is retained. | PASS |
| 1000GP responsibility | Two-chunk pilot remained at 8,776,080 KiB RSS; whole 1000GP is explicitly not authorized. | PASS |

Stable v1 is blocked while any `OPEN` row above is release-critical. A row may
move to `PASS` only with a checked-in test, retained result, or accepted ADR;
prose assertions alone are not evidence.
