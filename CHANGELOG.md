# Changelog

## Unreleased

### Changed

- Added default `named-loci-v1---` and `summary-pyr-v1--` archive extensions.
  The encoder always emits both descriptors, populates summaries from exact
  core-tile counters, and accepts `--annotations <GFF3>` plus an explicit
  reference-sample binding for names and aliases. The TypeScript reader exposes
  lazy `searchLoci()` and `summary()` range APIs with a separate bounded cache.
- Registered both binary schemas in the normative v1 specification. They use
  independently compressed, BLAKE3-128-protected pages and remain skippable so
  regional graph decoding never depends on viewer metadata.
- Advanced encoder reports to schema 5 with feature page/count/byte metrics and
  annotation input settings.

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
- Advanced encoder reports to schema 6. Source SHA-256 and disk-cache
  construction now overlap, while their worker-wall times and combined
  critical-path wall remain separately labeled.

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

### Compatibility

- Newly encoded `.pngr` bytes change because the two standard extension
  descriptors are now present by default. The archive magic and v1 regional
  payload remain unchanged; pre-release research archives without these
  optional entries still decode, and should be regenerated for the new viewer
  capabilities.
