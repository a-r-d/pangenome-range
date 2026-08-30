# ADR: production named source-path membership

Date: 2026-08-30

Status: accepted as an experimental identity mode; archives must be regenerated

## Decision

File-format v1 registers optional `path-members-v1-`. The normal graph payload and
its `anonymous-distinct-weighted-tile-paths` semantics do not change. Encoders emit
named identity only with `--path-membership`; readers that do not request it may skip
the extension.

The descriptor owns a paged complete source-path catalog and one compact manifest per
graph manifest. Fixed 4 KiB membership-directory pages align one-for-one with graph
directory pages. They address independently compressed membership pages in identical
tile order. This removes the experiment's root-wide tile list and its 65,536-tile
limit without adding a row-per-visit index.

The build crate parses embedded GBWT DA support and performs bounded tile-batched LF
locate. Persistent source-cache v2 stores authenticated DA support and the canonical
catalog. Neither the format nor browser package depends on GBWT or `gbwt-rs`.

The descriptor records the identity implementation, authenticated source SHA-256,
catalog/group/occurrence totals, the sum of per-group distinct-path counts, and codec
distribution. That sum is deliberately not called archive-global uniqueness. Each tile page carries
the aligned regional payload BLAKE3-128. Rust and TypeScript compute the same
domain-separated traversal digest over manifest identity, core bounds, payload
integrity, and canonical oriented nodes. Validation requires exact digest,
occurrence-weight, unique-count, multiplicity, and catalog-bound agreement.

The TypeScript reader provides lazy catalog lookup, bounded batched lookup, search, per-decoded-tile
membership reconciliation, and a combined query with separate graph, membership,
and catalog traces. It exposes only real catalog IDs and tile-local memberships. It
does not stitch anonymous traversals across tiles.

## Evidence and exclusions

Fresh final-layout anonymous/named controls passed for rice Xa7 (one tile) and HPRC TERT (four
tiles), each with one worker, a 64 MiB construction queue, a hard 4 GiB address-space
limit, and zero swap. Named encoder wall overhead was 3.49% for rice and 2.57% for
HPRC. Observed peaks were 155,344 KiB and 653,416 KiB. These bounded pilots do not
establish archive-wide storage overhead. The fixed mirrored directories alone would
add 249,856 bytes for rice chr06 and 47,345,664 bytes for whole HPRC before tile
membership pages, catalog, descriptor, compression, and metadata. Full Rust reconstruction and the
public TypeScript reader both required occurrence weight to equal membership
multiplicity. Exact controls, checksums, ranges, timings, and limitations are retained
in `results/named-membership/`.

Graph-only canonical hashes and two dependency rounds are unchanged. The additional
registered extension entry adds 64 fetched bytes. The current lazy identity path
needs two serial membership rounds followed by one catalog round, missing the
aspirational single-extra-round target. That measured boundary keeps the feature
experimental rather than stable.

Population, subpopulation, cultivar, country, and phenotype metadata are deliberately
absent from the core schema. This preview exposes the source sample key for a
caller-owned join but does not ship a cited metadata sidecar or population-grouping
UI and makes no enrichment claim.

The checked-in `path-membership-v1.pngr` golden fixture is encoded from the checked-in
MICB/KIR3DL1 GBZ and is decoded by both Rust and TypeScript, including a
membership-directory corruption rejection. Ephemeral and persistent-cache v2 encodes
of the synthetic fixture are byte-identical.

Rust validation and the TypeScript identity reader require archive provenance metadata
and reject a source SHA-256 mismatch across the two extensions. Membership decoding
has a separate 250,000-record per-group and per-tile browser-safety bound.

The user explicitly accepts the 1000G exclusion after the earlier memory failure. No
1000G named-membership overhead, time, or memory claim is inferred. Archive-wide HPRC
construction cost also remains an operational measurement, not a correctness blocker.

No change was merged upstream while proving the experiment; this ADR records the
local productionization decision and the required regeneration of older research
archives.

READY_BUT_EXPERIMENTAL_IDENTITY_MODE
