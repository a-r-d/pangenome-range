# Changelog

## Unreleased

### Changed

- Productionized optional named-path membership as registered `path-members-v1-`.
  `--path-membership` performs bounded tile-batched LF locate in the disk encoder;
  scalable fixed 4 KiB membership directories mirror graph directory pages instead
  of listing every tile in the root. Persistent source-cache v2 authenticates and
  reuses GBWT DA support and the canonical path catalog. Rust validation reconciles
  every group with the unchanged anonymous traversal payload, while the browser
  package exposes `capabilities().pathMembership`, `info().pathMembership`, and
  lazy `pathCatalogInfo()`, `pathById()`, batched `pathsByIds()`, `searchPaths()`,
  `tilePathMemberships()`, `pathMembership()`, and `queryWithPathMembership()` APIs.
  Combined queries report graph, membership, and catalog traces separately, and the
  tube-map inspector filters, copies, and highlights named source paths within loaded
  tiles. Membership hashes now bind manifest identity, core bounds, regional payload
  BLAKE3-128, and canonical oriented nodes. The descriptor records authenticated
  identity-source provenance, catalog/group/occurrence totals, the sum of per-group
  distinct-path counts, and codec
  distribution. A checked-in Rust/TypeScript golden archive covers exact path
  identity, multiplicity totals, range decoding, and directory corruption. Missing
  path metadata fails closed without fabricated biological labels, and one path may
  retain both orientations in a group while counting once toward `uniquePathCount`.
  Fresh
  final-layout rice Xa7 and four-tile HPRC TERT runs passed under a hard 4 GiB
  address-space cap at 155,344 KiB and 653,416 KiB peak RSS respectively, with zero
  swap. Named encoder wall overhead was 3.49% and 2.57%; bounded tile results do not
  establish archive-wide storage overhead. Graph-only hashes and
  dependency rounds were unchanged, with a measured 64-byte extension-directory
  increment. Exact evidence is retained under `results/named-membership/`.
  The 1000G pilot is an explicit user-accepted resource-safety exclusion after the
  earlier HPRC `.ri` OOM; no 1000G membership measurement is inferred.
  Source-cache v1 directories are intentionally unsupported and must be rebuilt.
  Cross-extension validation now requires the membership identity-source SHA-256 to
  equal archive provenance, and membership materialization is capped separately at
  250,000 records per group and tile. Encoder reports advance to schema 12 and record
  named/anonymous mode, identity
  source/checksum, membership totals, codec distribution, and directory-page counts.
- Added `named-loci-v1---`, `summary-pyr-v1--`, and `archive-meta-v1-`
  extensions. The encoder always emits summaries and deterministic provenance,
  and emits named loci only with explicit GFF3, sample, release, and assembly
  binding. The TypeScript reader exposes lazy `searchLoci()`, `summary()`, and
  `info()` APIs with separate encoded and decoded feature caches.
- Registered both binary schemas in the normative v1 specification. They use
  independently compressed, BLAKE3-128-protected pages and remain skippable so
  regional graph decoding never depends on viewer metadata.
- Advanced encoder reports with feature page/count/byte metrics and
  annotation input settings.

- Added repeatable exact `--annotation-feature-type` selection (default
  `gene`), GFF3 sequence-region validation, partial-archive containment, and
  early equal-key/page limit failures. Prefix search now uses bounded
  concurrency and stops after it knows the limit is truncated.
- Added a versioned persistent source cache with atomic construction,
  interprocess locking, source SHA-256 authentication, serialized sparse
  reference-index reuse, inspection, and explicit pruning. Fused ephemeral
  cache construction computes source SHA-256 in the same sequential pass.
- Advanced encoder reports to schema 7 for persistent-cache reuse, cache-open,
  manifest-validation, and path-index-deserialization evidence.
- Added explicit `--sample NAME --reference-haplotype N` anchoring for real
  named population haplotypes in GBWTs without reference tags. Schema-8 encoder
  reports record the selected haplotype; the mode uses the bounded ephemeral
  disk source and cannot reuse a persistent cache.
- Made graph results independent of payload completion order and documented
  `queryTiles()` progressive event order as intentionally unspecified. Summary
  pyramids now aggregate adjacent groups of four in linear emitted-bin work and
  select valid levels independently for fragmented manifests.
- Linux native launching no longer treats missing glibc metadata as proof of
  musl; an explicit override or the only installed exact-version package is
  required when runtime reporting is ambiguous.

- Made the project-owned disk-backed GBZ source the default production encoder
  path. Reference indexing, local context extraction, and exact packed-record
  handling no longer call `gbz-base`; the loaded adapter and `gbz-base` remain
  available as independent correctness/research baselines.
- Added `--source-access disk|loaded` and use `--scratch-dir` for the ephemeral
  indexed source cache. The cache has a fixed 64 MiB read budget, sharded for
  bounded parallel workers, and is removed when encoding exits.
- Extended schema-4 encoder reports with source-access mode, cache construction
  and size, source/path-index memory evidence, and coherent critical-path wall
  timing.
- Replaced per-batch construction and compression thread creation with one
  bounded persistent worker pool shared by a deterministic rolling construction
  window and ordered compression jobs.
- Advanced encoder reports to schema 7. Source SHA-256 and ephemeral disk-cache
  construction use one sequential source pass; persistent-cache authentication,
  named-locus stages, and extension byte totals are reported separately.
- Added an atomic persistent source cache with source/version/checksum binding,
  per-block integrity, a serialized sparse reference index, locking, inspection,
  and explicit pruning. Warm encodes skip raw-cache and reference-index rebuilds.
- Restricted the default GFF3 named-locus importer to `gene` features so gene
  labels and stable IDs are not multiplied across transcript, exon, CDS, codon,
  and UTR child records.

### Performance

- On the retained whole-HPRC source, reduced peak RSS from 8,775,928 KiB to
  608,060 KiB (93.07%) while producing the identical 8,828,788,418-byte archive
  and SHA-256. Whole wall increased from 438.72 to 499.34 seconds (13.82%), with
  11,921,858,427 bytes of ephemeral source-cache disk.
- After enabling both viewer indexes, the same source/options/host produced an
  8,829,030,376-byte archive in 503.36 seconds at 642,220 KiB peak RSS. Relative
  to the preceding disk-backed run this is +241,958 bytes (+0.00274%), +4.02
  seconds (+0.81%), and +34,160 KiB RSS; all nine source-oracle queries passed.
- With unified rolling workers and source checksum/cache overlap, that exact
  archive completed in 409.94 seconds (-18.53%) at 640,556 KiB peak RSS. The
  payload pipeline fell from 328.72 to 267.23 seconds, and archive/index bytes
  plus SHA-256 remained identical.
- The canonical whole-HPRC GENCODE v50 annotation run indexed all 78,733 CHR
  genes as 157,466 name/stable-ID records in 612 range-addressable pages. It
  added 3,719,573 bytes (+0.0421%) and 23.12 seconds of writer finalization;
  whole wall was 420.85 seconds and peak RSS was 695,816 KiB.
- The release-hardening warm-cache rerun produced an 8,832,750,626-byte
  populated archive at 621,808 KiB peak RSS in 384.08 seconds. Cold and warm
  source modes produced byte-identical output; warm reuse reduced the directly
  comparable prebuild phase from 118.944 to 21.685 seconds. The 677-byte change
  from the prior populated archive is deterministic archive provenance.

### Compatibility

- Newly encoded `.pngr` bytes change because summary and provenance extension
  descriptors are present by default, while an unpopulated named-locus
  extension is omitted. The archive magic and v1 regional
  payload remain unchanged; pre-release research archives without these
  optional entries still decode, and should be regenerated for the new viewer
  capabilities.
