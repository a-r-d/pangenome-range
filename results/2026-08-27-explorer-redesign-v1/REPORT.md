# Pangenome Explorer redesign

## Verdict

Accepted as an application and reader tranche. The explorer now follows the
provided overview, search, loading, and detail mockups while preserving the
existing reader/viewer boundary and v1 semantics. No file-format layout changed.

The primary workflow is now orientation first: a summary-capable archive opens
to a composite regional overview and exact directory plan without fetching
graph payloads. The user explicitly opens a recommended bounded detail window.

## Product changes

- Replaced the dense three-column developer surface with a 64 px command bar,
  56 px tool rail, single dominant workspace, overlay inspectors, and bottom
  evidence drawer.
- Added archive-native command-palette search with aliases, coordinates, recent
  entries, and keyboard navigation.
- Added staged archive/summary/graph/integrity loading UI. Archive replacement
  keeps the previous visualization mounted until the replacement is ready.
- Added exact `planRegion()` directory planning before payload reads.
- Added composite topology, traversal-evidence, and exact transfer-cost tracks.
- Added full-bin bounds and coverage fractions so clipped summary estimates are
  honest.
- Added deterministic topology bundling for dense regional graph views.
- Preserved local anonymous traversal semantics; no sample/path identities are
  invented or stitched across tiles.

## Browser evidence

The built Pages test passed Chromium, Firefox, and WebKit. It exercised 12
strict loopback `206` responses, coordinate and archive-index search, aliases,
history, node selection, local files, custom URLs, cancellation, bad range
responses, dark mode, and responsive layouts. It also connected successfully
to both configured public multi-gigabyte archives.

One public HLA-B orientation-first observation planned 2 payload ranges and
139.3 KiB without fetching them; the UI reported 108.1 ms total summary/plan
wall. This is a single public observation, not a percentile. The deterministic
4.3 KiB fixture reported 5.8 ms open, 0.6 ms first summary paint, 1.2 ms first
graph tile, 1.9 ms query completion, 0.0 ms layout, and 0.3 ms paint in the
retained Chromium run.

The document had no horizontal or vertical page overflow at 1600x1000,
1366x768, 1024x768, or 820x1180. Explicit popovers, inspectors, and the evidence
drawer retain their own bounded scrolling.

## Summary-granularity decision

The existing whole-HPRC evidence records 591 series, 8,017 bins, 241,798
encoded extension-body bytes, 588,832 decoded bytes, and 188.823 ms feature
finalization for the current 1,048,576 bp base span. The requested coarser
alternatives have the following gene-scale locality for the 17,873 bp HLA-B
overview:

| Base-span option | Span | HLA-B fraction of one bin | Decision |
| --- | ---: | ---: | --- |
| current | 1,048,576 bp | 1.70% | keep |
| 4x | 4,194,304 bp | 0.43% | reject |
| 16x | 16,777,216 bp | 0.11% | reject |
| 64x | 67,108,864 bp | 0.03% | reject |

Coarser bins could only reduce an already negligible 0.00274% whole-archive
extension while making the primary gene-scale estimate less local. The exact
planner removes the need to infer transfer bytes from those bins. A finer-base
experiment may be useful, but the exact 5.49 GB source and 8.23 GiB archive are
not present in this checkout. No synthetic projection is reported as a fresh
construction measurement, and the v1 default remains unchanged.

## Commands and results

```text
./node_modules/.bin/tsc -p packages/browser/tsconfig.json --noEmit
./node_modules/.bin/tsc -p docs/tsconfig.json --noEmit
./packages/browser/node_modules/.bin/vitest run \
  packages/browser/test/reader.test.ts packages/browser/test/viewer.test.ts
./packages/browser/node_modules/.bin/tsup
./docs/node_modules/.bin/vitepress build docs
node docs/test/pages-smoke.mjs
```

Focused reader/viewer tests: 38/38 passed. Both TypeScript checks, the Pages
build, the five-test benchmark suite, and the Rust gate (74 unit/integration
tests plus doc tests) passed. The Pages smoke passed all three engines and both
configured public archives.

## Remaining limitations

- The whole-HPRC summary-base alternatives were not freshly encoded because
  the source/archive are absent locally; changing the format from analytical
  estimates would violate the project evidence rules.
- The composite overview is visually most informative with multiple bins; a
  sub-bin gene view correctly shows a broad whole-bin estimate and its partial
  coverage rather than inventing within-bin shape.
- Physical iOS and Android testing remains open; tablet and narrow layouts were
  exercised in Chromium at the retained viewports.
