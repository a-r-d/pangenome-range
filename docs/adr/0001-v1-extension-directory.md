# ADR 0001: reserve a versioned v1 extension directory

- Status: accepted for the v1 release candidate
- Date: 2026-08-26
- Decision owners: file-format specification and both reference decoders

## Context

The fixed archive already answers regional graph queries efficiently, but the
known reader/viewer roadmap needs archive title/description/provenance, named
loci and aliases, optional multiscale summaries, and possibly annotations. The
format is still pre-stable, so this is the last responsible point to decide
whether these additions always require v2.

The decision must preserve the 64-byte bootstrap, arithmetic fixed-page lookup,
one parallel regional-payload round, immutable static-object operation, and
current no-extension bytes. It must not turn extensions into a second server or
an unbounded generic object graph.

## Considered options

### Option A: no v1 extension mechanism

Advantages:

- no new bootstrap parser or extension corruption surface;
- every v1 byte has a single current meaning;
- new metadata cannot accidentally become a required query dependency.

Costs:

- the first embedded provenance, named-locus, summary, or annotation section
  forces file-format v2 even when the regional representation is unchanged;
- a sidecar adds an identity-binding contract, another object lifecycle, and
  usually another dependency round;
- a sidecar is secure only when it declares and the client verifies the exact
  archive byte length plus a cryptographic whole-object digest (or an
  equivalently strong immutable identity). A filename or mutable URL is not a
  binding;
- title/provenance and locus search would otherwise be split from the object
  whose identity they describe.

### Option B: use header bytes 48..63 as an extension-directory pointer

Advantages:

- an archive with no extensions keeps both fields zero; the extension pointer
  alone does not add bytes or rounds (the separate checksum ADR changes
  directory bytes);
- an extension directory placed immediately after the root is normally covered
  by the existing 16 KiB bootstrap and does not add a query dependency round;
- unknown optional types can be ignored while unknown required types fail
  closed;
- metadata payloads remain independently range-addressable and integrity-bound;
- the regional directory and payload format remain unchanged.

Costs:

- the header, root/page contiguity rule, extension parser, and malformed-input
  surface become larger;
- an unusually large root plus extension directory can require the existing
  metadata-remainder round;
- reading an out-of-bootstrap extension payload adds a round when that feature
  is requested;
- extension type/schema governance is required to prevent incompatible reuse.

## Decision

Choose Option B.

Header bytes 48..55 are `extension_directory_offset`; bytes 56..63 are
`extension_directory_length`. Both are zero when absent. When present, the
directory begins exactly at `64 + root_length`, is at most 1 MiB, and directory
pages begin immediately after it.

The extension directory uses magic `PNGEXT01`, directory version 1, a 32-byte
header, and sorted unique 64-byte entries. Each entry declares a 16-byte type
identifier, required bit, codec, absolute offset, encoded/decoded lengths, and
a BLAKE3-128 digest of the encoded bytes. Extension payloads begin at or after
`data_offset`. Unknown optional types are skipped. Unknown required types fail
archive open. The complete normative layout is in
[`FILE_FORMAT_V1.md`](../FILE_FORMAT_V1.md).

The fixed costs are:

| Case | Added object bytes | Added normal graph-query rounds |
| --- | ---: | ---: |
| No extensions | 0 | 0 |
| Present, zero-entry directory | 32 | 0 while bootstrap covers it |
| Each declared extension | 64 plus encoded payload | 0 for directory; payload only when consumed |

No extension type is implicitly supported. A type requires a registry entry,
decoded schema, resource limits, Rust and TypeScript tests, and conformance
fixtures before an encoder may mark it required.

## Product placement decisions

- Archive title, description, and source provenance belong in one optional
  metadata extension once its schema is registered. Stable v1 remains blocked
  until provenance is either emitted there or explicitly external and securely
  bound.
- Named loci and aliases belong in an optional searchable extension when their
  measured lookup structure fits the static-object/request budget. A small
  sidecar is still permitted, but must bind to archive length and cryptographic
  identity.
- Multiscale summaries and annotations remain optional extension types; they
  must never be required to decode a regional graph query.

## Compatibility and consequences

This is a pre-stable v1 semantic change, not a v2 decoder stack. Existing
research archives have zeroes in these fields and retain identical bytes and
meaning. Extension-enabled objects are intentionally rejected by older
pre-release readers. There is no historical compatibility decoder.

The reference implementations now parse the pointer and directory, skip
unknown optional entries, and reject unknown required entries. The shared
cross-language fixture includes a positive unknown-optional provenance example.
The release candidate still needs a registered provenance type/schema before
stable v1.
