# Changelog

## Unreleased

### Changed

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

### Performance

- On the retained whole-HPRC source, reduced peak RSS from 8,775,928 KiB to
  608,060 KiB (93.07%) while producing the identical 8,828,788,418-byte archive
  and SHA-256. Whole wall increased from 438.72 to 499.34 seconds (13.82%), with
  11,921,858,427 bytes of ephemeral source-cache disk.

### Compatibility

- No `.pngr` format bytes changed. Rust/TypeScript conformance fixtures and
  deterministic archives remain unchanged.
