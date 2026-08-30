# Direct encoder path-membership proof

Status: bounded local proof; not a format or publication decision.

The disk-backed encoder now parses GBWT's embedded document-array samples, performs
bounded batched LF locate, builds the complete paged path catalog from GBWT metadata,
and writes one membership page per tile. The implementation is internal to
`pangenome-range-build`; the decoder has no GBWT dependency and the local `gbwt-rs`
fork remains only an independent oracle.

## Correctness

- The synthetic fixture recovered all ten traversal starts exactly. Maximum locate
  distance was seven LF steps. Its archive is byte-identical to the prepared
  C++/local-fork fixture.
- A unit test exhaustively walks every sequence in the checked-in tiny GBZ and
  requires the direct DA locate result for every GBWT position to equal brute-force
  enumeration.
- The rice Xa7 tile is byte-identical to its prepared fixture and preserves 51 groups
  and 60 membership records.
- The four-tile HPRC TERT archive is byte-identical to the existing prepared fixture,
  preserving 1,686 groups and 4,257 membership records over 8,522 located starts.
- Full archive validation reconstructs anonymous traversals and requires exact
  digest/weight agreement, so every membership multiplicity sum equals its existing
  occurrence weight.

## Bounded measurements

| Corpus | Tiles | Starts | Max LF | Locate ms | Peak RSS KiB | Swap | Archive bytes | Exact oracle SHA |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| Synthetic | 1 | 10 | 7 | 0.011 | 6,856 | 0 | 5,572 | yes |
| Rice Xa7 | 1 | 122 | 1,017 | 16.431 | 177,096 | 0 | 348,394 | yes |
| HPRC TERT | 4 | 8,522 | 1,023 | 1,486.906 | 660,332 | 0 | 987,840 | yes |

Each process ran with one encoder worker and an address-space cap (1 GiB synthetic,
4 GiB rice/HPRC). HPRC source-cache construction took 49.08 seconds and reference
indexing 42.63 seconds; archive construction itself took 2.28 seconds. No vg or `.ri`
construction ran.

## Remaining boundary

Persistent source-cache v1 does not serialize DA support, so direct membership cannot
yet reuse it and must stream the source GBZ into a fresh ephemeral disk cache. The
research extension is still unregistered, bounded to 65,536 pages, and skipped by the
public TypeScript API. No archive-wide HPRC or 1000G run was attempted, and no
normative format change was merged.

Exact compact measurements are in `summary.json`; generated GBZ caches and `.pngr`
objects remain outside the repository.
