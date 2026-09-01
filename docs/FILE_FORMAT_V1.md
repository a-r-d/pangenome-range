# Pangenome Range File Format v1

Status: normative for the current unreleased, pre-v1 implementation.

This document is the complete contract for a `.pngr` object. The Rust encoder
and reader and the TypeScript reader must implement this document. Other design
documents explain motivation and measurements but do not override it.

## 1. Conventions

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** in this document are normative requirements. A conforming decoder
MUST reject an object that violates a **MUST** or **MUST NOT** rule. A conforming
encoder MUST emit objects satisfying every applicable requirement. **SHOULD**
and **SHOULD NOT** identify interoperability or operational requirements that
may be departed from only for a documented reason.

- All integers are unsigned and little-endian.
- `u8`, `u32`, and `u64` occupy 1, 4, and 8 bytes respectively.
- Every offset is an absolute byte offset from the start of the `.pngr` object.
- Every interval is half-open: `[start, end)`.
- Every length-prefixed byte string is `u64 byte_length` followed by exactly
  that many bytes. Text strings use the same representation and must be valid
  UTF-8.
- Unless a section explicitly permits padding, a parser MUST consume the
  section exactly. Trailing bytes are corruption.
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
optional versioned extension directory
one or more contiguous 4096-byte directory pages per reference manifest
compressed or uncompressed regional payload byte ranges
```

The root starts at offset 64. When present, the extension directory starts
immediately after the root; otherwise the directory pages do. Directory pages
are ordered by manifest and then bucket and end at `data_offset`. Regional and
extension payloads begin at or after `data_offset`. Two directory entries may
reference the exact same payload range when deterministic payload deduplication
is enabled.

Section ranges MUST NOT partially overlap. Exact reuse of a complete payload
range is the only permitted overlap.

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
| 48 | 8 | `u64` | extension directory offset | zero when absent; otherwise exactly `64 + root_length` |
| 56 | 8 | `u64` | extension directory length | zero when absent; otherwise 32 through 1,048,576 bytes |

An archive with any other magic or version is unsupported. Readers must not
probe for or reinterpret older research formats.

Both extension fields MUST be zero or both MUST be nonzero. A reader MUST distinguish an unsupported
magic/version from corruption in an otherwise supported version where its API
exposes typed errors; it MUST NOT attempt another historical decoder.

### 3.1 Versioned extension directory

The extension directory is optional. Its absence is encoded by two zero header
fields, preserving the 64-byte bootstrap and current no-extension archive
bytes. When present it occupies exactly the header-declared range immediately
after the root. Directory pages begin immediately after it.

The directory header is 32 bytes:

| Offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 8 | bytes | magic, ASCII `PNGEXT01` |
| 8 | 4 | `u32` | extension-directory version, exactly `1` |
| 12 | 4 | `u32` | entry size, exactly `64` |
| 16 | 8 | `u64` | entry count |
| 24 | 8 | bytes | reserved, all zero |

Each fixed 64-byte entry is:

| Relative offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 16 | bytes | nonzero type identifier |
| 16 | 4 | `u32` | flags; bit 0 means required, all other bits zero |
| 20 | 1 | `u8` | codec, using the archive codec table |
| 21 | 3 | bytes | reserved, all zero |
| 24 | 8 | `u64` | absolute encoded-payload offset |
| 32 | 8 | `u64` | encoded length, nonzero |
| 40 | 8 | `u64` | decoded length, nonzero |
| 48 | 16 | bytes | first 128 bits of BLAKE3 over the encoded bytes |

Entries MUST be strictly ordered by type identifier and types MUST be unique.
Extension payloads start at or after `data_offset`, stay within the object, and
obey the same stored/zstd exact-frame rules as regional payloads. Their encoded
digest MUST validate before decompression. Unknown optional extensions are
skipped. An unknown required extension MUST fail archive open. Implementations
MUST NOT interpret an unknown type by inspecting its payload. Extension types
and decoded schemas require a normative registry entry before they can be
emitted as required.

### 3.2 Registered extensions

All registered v1 entries have the required flag clear. The reference encoder
always emits the summary and deterministic archive metadata entries. It emits
the named-locus entry only when explicit annotation input is supplied and the
path-membership entry only when `--path-membership` is requested. Readers
that do not implement these features MUST be able to skip them and continue
serving regional graph queries.

| Type identifier (exactly 16 bytes) | Purpose |
| --- | --- |
| `named-loci-v1---` | binary-searchable names, aliases, and genomic intervals |
| `summary-pyr-v1--` | arithmetic multiscale overview bins |
| `archive-meta-v1-` | deterministic source, encoder, reference, and annotation provenance |
| `path-members-v1-` | tile-local named GBWT source-path membership and path catalog |

Each extension-directory entry addresses a small descriptor. A known
descriptor MAY own child page ranges elsewhere at or after `data_offset`.
Every child range declares its own absolute offset, encoded length, decoded
length, codec, and BLAKE3-128 value using the 56-byte sequence below:

| Order | Type | Field |
| ---: | --- | --- |
| 1 | `u64` | absolute child-page offset |
| 2 | `u64` | encoded length |
| 3 | `u64` | decoded length, at most 64 MiB |
| 4 | `u8` | archive codec |
| 5 | 7 bytes | reserved, all zero |
| 6 | 16 bytes | first 128 bits of BLAKE3 over encoded page bytes |

Known-extension child pages MUST stay within the object, MUST NOT overlap any
regional payload, extension descriptor, or other child page, and MUST pass
their digest before decompression. A parser MUST consume every descriptor and
page exactly. Registered schemas may also define fixed raw directory pages that
address these child-page sequences. This is not a generic recursive extension graph.
Both encoded and decoded compressed child-page lengths are at most 64 MiB.

#### 3.2.1 Named loci

The descriptor begins with:

| Order | Type | Field |
| ---: | --- | --- |
| 1 | 8 bytes | magic `PNGLOC01` |
| 2 | `u32` | version, exactly `1` |
| 3 | `u32` | leaf-page count, at most 65,536 |
| 4 | `u64` | total indexed-record count |
| 5 | 32 bytes | SHA-256 of the exact annotation input, or all zero for an empty index |
| 6 | byte string | annotation filename, or empty for an empty index |

Each leaf descriptor then contains `first_key` and `last_key` byte strings,
`u64 record_count`, and the 56-byte child-page sequence above. Leaf key ranges
MUST be nonempty, strictly disjoint, and strictly increasing. Equal keys MUST
NOT be split across leaves. Leaf counts MUST sum to the descriptor total. Zero
records requires zero leaves; otherwise both counts are nonzero.

A decoded leaf starts with magic `PNGLPG01`, `u32 version = 1`, and a nonzero
`u32 record_count` no greater than 65,536. Each record contains, in order:

1. normalized search key, matched input name, display name, stable identifier,
   feature type, reference sample, and reference contig as seven byte strings;
2. zero-based half-open `u64 start` and `u64 end`;
3. strand byte (`0` unknown, `1` forward, `2` reverse) and seven zero bytes.

Records are ordered by `(normalized_key, sample, contig, start, end,
stable_id)`. The normalized key is formed by removing ASCII whitespace at both
ends and mapping only ASCII `A` through `Z` to lowercase. No locale-sensitive
or Unicode case folding is allowed. Exact search compares the full normalized
key. Prefix search begins at the first leaf whose last key is not less than the
normalized prefix and proceeds only while the leaf range can contain that
prefix. Results MAY be capped by a caller limit, but truncation MUST be exposed.

GFF3 input coordinates are converted from one-based inclusive to zero-based
half-open. The input is explicitly bound to a real archive reference sample;
an encoder MUST NOT infer a sample when more than one reference sample is
present. The reference contig must match exactly. `Name`, `Alias`, `ID`,
`gene_name`, `gene_id`, and `gene_synonym` values are searchable when present.
The reference encoder selects only records whose GFF3 feature type is exactly
`gene`; it does not duplicate gene-level search names onto transcript, exon,
CDS, codon, or UTR child records. This selection is an encoder policy rather
than a restriction on the feature-type field of independently produced v1
archives.
The reference encoder's default accepted feature type is exactly `gene`.
Repeatable explicit feature-type options replace that default and compare the
GFF3 type field byte-for-byte. Duplicate aliases and duplicate expanded records
are removed only after the complete record tuple is formed; features sharing a
normalized key or stable identifier remain distinct and deterministically
ordered. Percent escapes MUST decode to valid UTF-8. A malformed recognized
`##sequence-region` directive, conflicting bounds for one contig, or an
accepted feature outside declared sequence bounds is an error.

The reference encoder requires user-supplied annotation release and assembly
identifiers whenever annotations are supplied. It never guesses `chr1` versus
`1`, downloads annotations, infers parent/child biological identity, or invents
assembly, sample, or locus identity. For a partial or fragmented archive it
indexes only a feature completely contained within one encoded manifest
interval. It MUST NOT return a locus interval partly absent from the archive.
An equal-key group MUST remain in one leaf and therefore MUST fail before page
construction if it exceeds 65,536 records or the 64 MiB encoded/decoded page
limit; the error SHOULD identify the key and limit.

An explicitly empty descriptor remains valid for independent producers and
conformance. The reference encoder omits `named-loci-v1---` when no annotation
input is supplied, so feature capability presence means a usable index was
requested. Readers SHOULD distinguish absent, present-empty, and
present-populated states in archive-information APIs.

#### 3.2.2 Multiscale summaries

The summary descriptor is a 32-byte header followed by fixed 88-byte series
descriptors:

| Offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 8 | bytes | magic `PNGSUM01` |
| 8 | 4 | `u32` | version, exactly `1` |
| 12 | 4 | `u32` | series count, 1 through 65,536 |
| 16 | 8 | `u64` | base bin span |
| 24 | 8 | bytes | reserved, all zero |

Each series descriptor contains `u32 manifest_index`, `u32 level`, `u64
bin_span`, `u64 first_bin_start`, `u64 bin_count`, then the 56-byte child-page
sequence. Series are strictly ordered by `(manifest_index, level)`. Every
manifest has contiguous levels starting at zero. The canonical encoder uses:

```text
base_bin_span = regional_window_size * 64
bin_span(level) = base_bin_span * 4^level
first_bin_start = floor(manifest.start / bin_span) * bin_span
bin_count = floor((manifest.end - 1) / bin_span)
            - floor(manifest.start / bin_span) + 1
```

Levels continue until the series has one bin. A summary page has a 48-byte
header: magic `PNGSMP01`, `u32 version = 1`, `u32 record_size = 64`, its
manifest index and level as two `u32` values, its bin span, first bin start, and
bin count as three `u64` values. It is followed by exactly `bin_count`
fixed-width records containing eight `u64` values:

1. covered core reference bases;
2. accepted regional tile count;
3. encoded regional bytes;
4. decoded regional bytes;
5. tile-local node records;
6. tile-local edge records;
7. packed local GBWT records;
8. packed-record occurrence count.

Every accepted tile contributes exactly once, according to its core interval;
halo context never creates another contribution. Higher levels are exact sums
of base-bin values. Node, edge, GBWT-record, and occurrence fields are
tile-record totals, not globally unique graph elements, individuals, allele
frequencies, or globally stitchable haplotypes. Readers and viewers MUST label
them accordingly.

A query may clip the coordinate bounds of its first and last returned bin, but
the eight counters still describe the complete underlying stored bin. Reader
APIs therefore expose both the clipped query bounds and the full bin bounds,
plus the covered fraction. Consumers MAY coverage-prorate whole-bin counters
for a conservative display or LOD estimate, but MUST label that result as an
estimate. Exact payload count and transfer bytes come from directory planning,
not from prorating summary counters.

#### 3.2.3 Archive metadata

`archive-meta-v1-` is stored without compression by the reference encoder and
is limited to 1 MiB. Its fixed 112-byte prefix is:

| Offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 8 | bytes | magic `PNGMET01` |
| 8 | 4 | `u32` | schema version, exactly `1` |
| 12 | 4 | `u32` | fixed prefix bytes, exactly `112` |
| 16 | 8 | `u64` | source GBZ byte length, nonzero |
| 24 | 32 | bytes | SHA-256 of the exact source GBZ, nonzero |
| 56 | 8 | `u64` | regional window size, nonzero |
| 64 | 8 | `u64` | construction context, exactly `100` |
| 72 | 1 | `u8` | regional payload codec |
| 73 | 1 | `u8` | haplotype semantics, exactly `2` |
| 74 | 1 | `u8` | annotation checksum present, `0` or `1` |
| 75 | 5 | bytes | reserved, all zero |
| 80 | 32 | bytes | annotation SHA-256, nonzero iff the presence byte is `1` |

The prefix is followed by exactly ten byte strings in this order:

1. encoder package version, nonempty;
2. format implementation identifier, nonempty;
3. reference sample;
4. reference assembly;
5. dataset title;
6. dataset description;
7. canonical source URI;
8. annotation filename;
9. annotation release;
10. annotation assembly.

Strings 3 through 10 use an empty string to represent absence. Annotation
filename and checksum MUST be present together. Annotation release/assembly
MUST be absent when annotation provenance is absent. Encoders MUST NOT insert
an absolute local path, `file:` URI, current clock time, worker count,
source-access mode, scratch path, or other operational setting. They MUST NOT
invent absent identity. The extension-directory BLAKE3-128 binds these exact
metadata bytes to the archive; the source and annotation SHA-256 values bind
the named inputs. This is integrity and identity evidence, not a keyed
authenticity signature. Published whole-object SHA-256 or a strong immutable
HTTP identity remains necessary to bind the complete object externally.

#### 3.2.4 Named source-path membership

The optional descriptor begins with a 112-byte header:

| Offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 8 | bytes | magic `PNGPMD01` |
| 8 | 4 | `u32` | version, exactly `1` |
| 12 | 4 | `u32` | records per catalog page, 1 through 65,536 |
| 16 | 8 | `u64` | total source-path count, nonzero |
| 24 | 4 | `u32` | catalog-page count, nonzero |
| 28 | 4 | `u32` | membership-manifest count, nonzero |
| 32 | 1 | `u8` | identity source: `1` embedded GBWT DA bounded LF v1, `2` prepared authenticated oracle v1 |
| 33 | 7 | bytes | reserved, all zero |
| 40 | 32 | bytes | SHA-256 of the authenticated source GBZ |
| 72 | 8 | `u64` | membership group count |
| 80 | 8 | `u64` | membership occurrence total |
| 88 | 8 | `u64` | `group_unique_path_count_sum`: sum of each group's distinct path-ID count |
| 96 | 8 | `u64` | groups using delta codec `0` |
| 104 | 8 | `u64` | groups using run codec `1` |

The codec counts MUST sum to the group count, the source checksum MUST be nonzero,
and `group_unique_path_count_sum` MUST NOT exceed the occurrence total. This field
is not an archive-global distinct-path cardinality; a path may contribute once in
many groups or tiles. When this extension is present, `archive-meta-v1-` is REQUIRED
and its source GBZ SHA-256 MUST exactly equal this identity-source SHA-256. The complete
descriptor is at most 16 MiB. Its exact byte length is the 112-byte
header plus 64 bytes per catalog descriptor and 32 bytes per membership manifest.

Each catalog descriptor is `u64 first_path_id`, `u64 record_count`, then the
56-byte child-page sequence. Path IDs start at zero and pages cover a contiguous
range ending at `path_count`. A catalog page begins with magic `PNGPCP01`, `u32
version = 1`, `u32 record_count`, and `u64 first_path_id`. Each record contains
front-coded UTF-8 canonical name, sample, and contig strings; unsigned LEB128
haplotype and fragment values; and a sense byte (`0` unknown, `1` generic, `2`
reference, `3` haplotype). The canonical name is a deterministic rendering of
the structured GBWT metadata; the GBWT serialization does not retain an original
input spelling. A front-coded string is unsigned LEB128 prefix-byte length,
unsigned LEB128 suffix-byte length, then suffix bytes. Prefixes MUST end on a
UTF-8 boundary. Reconstructed strings in one page total at most 64 MiB. All LEB128
values MUST use their minimal representation. The production encoder emits named
membership only when every source path has complete path, sample, and contig
metadata; it never fabricates missing biological labels.

Each membership manifest is exactly 32 bytes: `u32 manifest_index`, four zero
reserved bytes, `u64 first_page_offset`, `u64 page_count`, and `u64 entry_count`.
There is exactly one membership manifest for each root reference manifest in the
same order. Page and entry counts MUST equal the corresponding graph manifest.
Membership-directory ranges are contiguous across manifests.

Membership directories are raw fixed 4 KiB pages aligned one-for-one with graph
directory pages. Their 32-byte header is magic `PNGPMI01`, `u32 version = 1`,
`u32 entry_count`, then BLAKE3-128 over bytes 32 through 4095. Up to 72 entries
follow in graph-directory order. Each entry is `u64 group_count` plus the 56-byte
child-page sequence. Remaining bytes MUST be zero. Empty pages are valid only when
the aligned graph directory page is empty.

A decoded tile page begins with magic `PNGPMT01`, `u32 version = 1`, `u32
group_count`, zero-based half-open `u64 core_start` and `u64 core_end`, and the
16-byte BLAKE3-128 integrity value of the aligned regional payload.
Zero groups is valid for a tile containing no anonymous traversal evidence.
Groups are strictly ordered by their 16-byte traversal digest. Each group contains
that digest, `u64 occurrence_weight`, `u64 unique_path_count`, `u64 membership_bytes`,
then exactly that many membership-codec bytes. The digest is BLAKE3-128 over the
domain `pangenome-range/path-membership/traversal/v1\0`; length-prefixed UTF-8
manifest sample and contig; little-endian `u64` core start and end; the aligned
regional payload BLAKE3-128; the traversal length as little-endian `u64`; and each
oriented handle as little-endian `u64`. This binds a group to the manifest identity,
physical tile interval, exact regional payload, and canonical oriented traversal.

Membership codec `0` stores a count followed by path-ID deltas, multiplicities,
and one orientation byte per membership. Codec `1` additionally run-encodes
consecutive path IDs with multiplicity one and equal orientation. Counts and integer
fields use minimal unsigned LEB128. `(path_id, orientation)` pairs MUST be unique
and strictly increasing; path IDs alone may repeat once with each orientation and
must be less than `path_count`. Multiplicities are nonzero and orientation is `0`
or `1`. For every group, membership multiplicities MUST sum to `occurrence_weight`
and `unique_path_count` MUST equal the number of distinct path IDs. Total expanded
occurrences per tile are at most 16,777,216. A decoder MUST also reject more than
250,000 materialized membership records in one group or one tile; multiplicity is
stored as a scalar and does not consume one record per occurrence.

Membership identity is tile-local. It associates each reconstructed local traversal
occurrence with a real GBWT source path, but it does not authorize a reader to stitch
anonymous graph-query traversals across tiles or infer biological continuity not
present in the returned membership records.

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

The `(sample, contig, start, end)` tuple of every manifest MUST be unique.
Multiple fragmented manifests MAY share `(sample, contig)` and a query MUST use
every fragment whose interval overlaps it. Manifest order is semantically
irrelevant to queries, but a conforming encoder MUST choose it deterministically
for the same source and options. A decoder MUST NOT silently replace an exact
duplicate interval.

The final manifest page range must end exactly at `data_offset`. Manifest entry
counts must sum exactly to the archive header entry count. The root decoder must
consume exactly `root_length` bytes; trailing root bytes are corruption.

## 5. Fixed directory pages

Every directory page is exactly 4096 bytes. It represents one arithmetic
coordinate bucket and can contain at most 72 entries.

### Page header

| Offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 4 | `u32` | entry count, 0 through 72 |
| 4 | 4 | `u32` | entry size, exactly `56` |
| 8 | 8 | `u64` | bucket start |

For bucket index `i`, the page offset and expected coordinate are:

```text
page_offset  = first_page_offset + i * 4096
bucket_start = grid_start + i * bucket_span
bucket_end   = bucket_start + bucket_span
```

### Directory entry

Entries begin at page offset 16. Every entry is 56 bytes.

| Relative offset | Size | Type | Field |
| ---: | ---: | --- | --- |
| 0 | 8 | `u64` | core start |
| 8 | 8 | `u64` | core end |
| 16 | 8 | `u64` | absolute payload offset |
| 24 | 8 | `u64` | compressed length |
| 32 | 8 | `u64` | uncompressed length |
| 40 | 16 | bytes | first 128 bits of BLAKE3 over the encoded payload bytes |

The core interval must be non-empty, begin within the bucket, and end no later
than `min(bucket_end, manifest.end)`. Both lengths are nonzero. The payload range
must start at or after `data_offset` and end within the object. Bytes after the
last entry through the end of the 4096-byte page are zero padding.

Entries in one page MUST be in nondecreasing lexicographic order by
`(core_start, core_end, payload_offset, compressed_length,
uncompressed_length, integrity)`. Exact duplicate entries are permitted and count as
separate logical directory coverage, but their identical physical payload MUST
be decoded at most once per validation/query operation. Partially overlapping
payload byte ranges are corruption.

Adaptive splitting may produce multiple entries inside one arithmetic bucket.
The page capacity is therefore a hard format limit: an encoder must split or
fail before emitting a 73rd entry. The obsolete pre-checksum research layout
fit 102 40-byte entries and has no compatibility decoder.

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

An empty intersection with the manifest produces no bucket read. Computing
`min(q_end, end) - 1` MUST occur only after proving the intersected interval is
non-empty. A query whose requested `(sample, contig)` has no overlapping
manifest MUST fail rather than choose a similar name or alias.

A normal remote query has three dependency stages: bootstrap header/root,
arithmetic directory lookup, then one parallel round for the selected payload
ranges. A 16 KiB bootstrap read may already contain some required directory
bytes and should be reused. The [range-read walkthrough](how-range-reads-work.md)
animates those rounds.

## 7. Payload compression

The manifest codec applies to every payload referenced by that manifest.
Codecs 1, 3, and 6 MUST contain exactly one standard zstd frame whose declared content size
and decoded byte length equal the directory entry's uncompressed length. Codec
0 stores the regional payload directly and therefore requires compressed and
uncompressed lengths to match.

The zstd frame MUST include its content-size field. Concatenated frames,
skippable frames, dictionaries, trailing bytes after the frame, reserved frame
or block bits, and frames whose encoded extent differs from the directory range
are corruption. The zstd frame content-checksum flag MUST be clear. V1 rejects
checksum-bearing zstd frames because every regional and extension payload is
already protected by BLAKE3-128 over the exact encoded bytes; this avoids
decoder-dependent checksum behavior.

Readers reject unknown codecs, truncated frames, decompression failure, output
larger than configured safety bounds, and decompressed-length mismatch.

Before decompression, a reader MUST compute BLAKE3 over the exact encoded
payload range and compare its first 16 bytes with the directory entry integrity
field. A mismatch is corruption. This digest detects accidental/corrupt range
content; it is not a keyed authenticity mechanism and does not replace the
strong immutable object identity or published whole-object SHA-256.

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

The decoder MUST consume the raw regional payload exactly. Counts MUST be
validated against both the remaining bytes and the configured resource limits
before reserving or allocating storage.

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

Node sequence bytes are opaque biological sequence bytes in v1; encoders SHOULD
emit their source representation without case conversion or alphabet
normalization. A repeated node identifier is corruption even when the sequence
bytes are identical.

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

Edges MUST be serialized in strictly increasing `(from_handle, to_handle)`
order. A decoder MUST reject a noncanonical orientation rather than silently
flip it.

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

The packed-record byte string MUST be consumed exactly. Non-minimal base-128
encodings SHOULD be rejected by release-candidate implementations and MUST NOT
be emitted by the reference encoder. A record may not cause any successor
offset, decoded occurrence total, or allocation length to exceed `u64`, the
remaining section bytes, or the v1 tile occurrence limit.

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

Canonical orientation for an anonymous traversal is determined from the first
and last packed handles: when their reverse bits match, the forward-ended form
is canonical; otherwise the same canonical bidirected comparison used for
edges is applied to the endpoints. The reference traversal is never
orientation-normalized or collapsed into anonymous evidence: its anchored
orientation is authoritative.

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

Tiles MUST be merged in a deterministic coordinate order independent of payload
arrival order. Conflicting sequence bytes, reference visits, or reference
coordinates are corruption. Boundary edges whose target is absent MAY remain
as tile topology but MUST enter the merged query graph only when both endpoint
nodes are selected.

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
- directory counts over 72, record/node count disagreement, occurrence-total
  disagreement, duplicate nodes/edges/records, or unsorted delta-coded data;
- a payload whose core interval differs from its directory entry;
- decompressed-length mismatch or trailing regional/root bytes;
- invalid local handles, edge orientation, GBWT ranks/runs/offsets, cycles, or
  reference anchors.

Counts must be checked against remaining bytes before allocation. Implementations
also apply explicit byte limits to roots, compressed chunks, decompressed chunks,
caches, and full-response fallbacks. A declared count never justifies an
unbounded allocation.

A conforming implementation MUST provide finite configurable limits for root
bytes, compressed payload bytes, decoded payload bytes, directory cache bytes,
payload cache bytes, and any accepted full HTTP response. Failure due to a
configured resource limit MUST be reported as a rejection, never as a partial
successful decode. Parsers MUST make forward progress or fail; cycles in local
occurrence reconstruction MUST be detected without unbounded work.

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

The first successful response MUST establish the object byte length and a
strong identity. Every later response MUST agree. A weak ETag is not a strong
identity; when no strong ETag is available the caller MUST supply an equivalent
immutable identity or the reader MUST reject multi-request operation. A range
retry MAY repeat a request only against the same established identity.

Public origins MUST permit CORS and expose `Accept-Ranges`, `Content-Range`,
`Content-Length`, and `ETag`. Immutable content-addressed objects should use
`Cache-Control: public, max-age=31536000, immutable, no-transform` and must not
be transparently compressed or transformed in transit.

## 16. Normative algorithms

The pseudocode in this section is implementation-independent. `reject` means
the operation fails closed without returning a partial archive, tile, graph, or
hash.

### 16.1 Archive open

```text
open(source, limits):
  object_size, identity = source.establish_identity()
  require object_size >= 64
  header_bytes = source.read_exact(0, 64, identity)
  header = decode_header_exact(header_bytes)
  require header.magic == "PNGRNG01" and header.version == 1
  require header.root_offset == 64 and header.root_length <= limits.root_bytes
  root_end = checked_add(64, header.root_length)
  if header.extension_directory_length == 0:
    require header.extension_directory_offset == 0
    directory_start = root_end
    extensions = []
  else:
    require header.extension_directory_offset == root_end
    require header.extension_directory_length <= limits.extension_directory_bytes
    directory_start = checked_add(root_end, header.extension_directory_length)
    extension_bytes = reuse_bootstrap_or_read_exact(root_end,
                                                    header.extension_directory_length,
                                                    identity)
    extensions = decode_extension_directory_exact(extension_bytes)
    reject any unknown required extension
  require root_end <= directory_start <= header.data_offset <= object_size
  root_bytes = reuse_bootstrap_or_read_exact(64, header.root_length, identity)
  manifests = decode_root_exact(root_bytes, header)
  require unique(manifest.sample, manifest.contig, manifest.start, manifest.end)
  require contiguous_manifest_pages(manifests, directory_start, header.data_offset)
  return Archive(source, object_size, identity, header, manifests, extensions,
                 bounded_caches)
```

### 16.2 Manifest selection

```text
select_manifest(archive, sample, contig):
  matches = manifests where identity == (sample, contig)
  require length(matches) >= 1
  return all matches in deterministic interval order
```

Aliases and named loci are not implicit manifest matching rules. They are
resolved only through the registered named-locus extension and return an exact
manifest sample/contig/interval for a subsequent coordinate query.

### 16.3 Coordinate-to-directory lookup

```text
lookup(manifest, query):
  lo = max(query.start, manifest.start)
  hi = min(query.end, manifest.end)
  if lo >= hi: return []
  first = (lo - manifest.grid_start) / manifest.bucket_span
  last  = ((hi - 1) - manifest.grid_start) / manifest.bucket_span
  pages = read_exact arithmetic pages first..last, reusing bootstrap/cache bytes
  entries = decode each page exactly and validate order, padding, ranges, limits
  selected = entries where entry.start < query.end and entry.end > query.start
  reject partial physical-range overlaps
  return stable_unique_physical_ranges(selected), preserving logical coverage
```

### 16.4 Regional decoding

```text
decode_region(entry, encoded, limits):
  require length(encoded) == entry.compressed_length
  if codec == stored:
    require compressed_length == uncompressed_length
    raw = encoded
  else:
    require exactly_one_standard_zstd_frame(encoded)
    require frame.content_size == entry.uncompressed_length <= limits.decoded_chunk
    raw = decompress_exact(encoded, entry.uncompressed_length)
  payload = parse_regional_prefix_strings_nodes_edges_records_exact(raw)
  require payload.core == entry.core
  require all counts, ordering, uniqueness, canonical orientation, and limits
  return payload
```

### 16.5 Local GBWT traversal reconstruction

```text
reconstruct(payload):
  decode every record's successor alphabet and runs exactly
  mark each local successor occurrence as having a predecessor
  starts = all occurrences without a local predecessor, in handle/offset order
  for start in starts:
    follow local successors, stopping at end marker or nonlocal handle
    reject invalid offset or visits greater than total local occurrences
    retain the walk and whether it contains the declared reference anchor
  require exactly one anchored reference walk
  orient anonymous walks canonically; sort; run-collapse equal walks to u64 weights
  subtract one occurrence of the anchored reference walk from anonymous evidence
  return anchored_reference_walk, weighted_tile_local_walks
```

### 16.6 Reference identification

```text
identify_reference(payload, anchored_walk, anchor_index):
  require anchor handle and occurrence exist in anchored_walk
  prefix = payload.reference_node_offset
  require prefix < sequence_length(anchor_node)
  prefix += checked_sum(sequence lengths before anchor_index)
  relative_start = checked_sub(payload.reference_query_offset, prefix)
  start = checked_add(payload.reference_fragment_start, relative_start)
  end = checked_add(start, checked_sum(all anchored-walk sequence lengths))
  return real identity, haplotype, fragment, orientation, [start, end), anchored_walk
```

### 16.7 Multi-tile graph merge

```text
merge(tiles, query):
  stable_sort tiles by (reference identity, core_start, core_end, physical range)
  include each physical tile once
  union nodes by id, requiring identical sequence bytes
  union canonical edges; activate an edge only when selected endpoint nodes exist
  assemble reference visits by coordinate, rejecting conflicts and gaps in used span
  select context through merged topology from overlap with query interval
  retain each tile's weighted anonymous walks under that tile's provenance
  return graph plus ordered tile-local evidence
```

### 16.8 Canonical hashing

```text
graph_hash(graph):
  BLAKE3("pangenome-range canonical query graph v1\\0" ||
         u64_count + sorted nodes(id, length-prefixed sequence) ||
         u64_count + sorted canonical edges(oriented endpoints) ||
         u64_count + sorted real paths(identity, haplotype, fragment,
                                       reference bit, ordered visits) ||
         u64_count + sorted reference intervals(identity, start, end))

tile_hash(tile):
  BLAKE3("pangenome-range canonical haplotype tile v1\\0" ||
         length-prefixed reference sample and contig || core_start || core_end ||
         u64_count + weighted traversals sorted by (weight, oriented visits),
         each encoded as weight || visit_count || oriented visits)
```

### 16.9 Named-locus search

```text
search_loci(archive, input, mode, filters, limit):
  entry = archive.extension("named-loci-v1---") or report unsupported
  descriptor_bytes = read_verify_decompress_exact(entry)
  descriptor = decode_named_loci_descriptor_exact(descriptor_bytes)
  key = ascii_trim_and_lowercase_A_through_Z(input)
  require key is nonempty and 1 <= limit <= implementation_limit
  leaves = binary-search descriptor fences for key or prefix range
  records = parallel read_verify_decompress_decode_exact(leaves)
  require every leaf count and first/last key agrees with its descriptor
  matches = stable records satisfying exact-or-prefix key and exact filters
  return first limit matches plus whether additional matches existed
```

### 16.10 Multiscale summary selection

```text
summary(archive, sample, contig, start, end, max_bins):
  require a nonempty safe interval and bounded positive max_bins
  manifests = exact overlapping manifest selection
  entry = archive.extension("summary-pyr-v1--") or report unsupported
  descriptor_bytes = read_verify_decompress_exact(entry)
  descriptor = decode_summary_descriptor_exact(descriptor_bytes)
  validate contiguous canonical levels for every manifest
  choose the finest common level whose overlapping bin total <= max_bins,
    or the coarsest available level if none qualifies
  pages = parallel read_verify_decompress_decode_exact(selected series)
  return arithmetic bins overlapping [start, end), retaining tile-total labels
```

Every integer above is encoded as little-endian `u64`; every byte string is
preceded by its `u64` byte length; an oriented visit is `u64 node_id` followed
by one byte `0` (forward) or `1` (reverse). Hash order is part of v1 and MUST NOT
depend on map iteration, worker count, request completion order, or host locale.

## 17. Worked tiny example and fixtures

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

## 18. Pre-release version policy

This project is unreleased and pre-v1. The current file format is named v1 and
uses only `PNGRNG01` and `PNGRGN01`.

Until the project deliberately declares a stable public format, an incompatible
format change replaces this v1 specification, implementation, and fixtures in
place. It does not create a v2 compatibility stack. Old research archives are
intentionally unsupported and must be regenerated with the current encoder.

The npm/Cargo package semantic version is independent of the on-disk format
version. Package releases may advance while the current pre-release file format
continues to identify itself as v1.

A conforming encoder MUST be deterministic: identical input bytes, options,
dependency lockfiles, and encoder version produce byte-identical output across
supported worker counts. Any intentional exception MUST be a documented format
change with replacement conformance fixtures. Old research archives MUST be
regenerated after an incompatible pre-stable v1 change; implementations MUST NOT
add a compatibility decoder unless a later stable-version policy explicitly
requires it.
