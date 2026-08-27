# Viewer product requirements

## Product contract

The explorer is a browser-only scientific application over an immutable
`.pngr` object. It composes the public reader and framework-neutral viewer; it
does not parse archive bytes, upload local filenames, or require a query
backend. The configured archive is selected by
`VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL`; the bundled fixture remains the
fallback.

The application preserves the format's declared semantics:

- the reference traversal has real sample, contig, coordinate, orientation,
  and fragment identity;
- `anonymous-distinct-weighted-tile-paths` are integer-weighted local evidence,
  not named individuals or globally stitchable haplotypes;
- summary values are exact tile-record totals, not unique variants, alleles,
  frequencies, or people.

## Research workflow

The full-viewport `/demo` route provides a command bar, layer and display
controls, genomic canvas, selection inspector, and collapsible technical
evidence panel. A researcher can:

1. open the configured object, fixture, arbitrary compatible remote URL, or a
   local file;
2. navigate with canonical coordinates or the archive-native named-locus
   index, including prefix suggestions, aliases, recent searches, and keyboard
   selection;
3. explore wide intervals through `archive.summary()` without fetching graph
   payloads;
4. load or progressively stream a detailed region through
   `archive.queryTiles()` when deterministic byte and record budgets allow it;
5. pan, pinch, wheel, or use the keyboard while settled genomic requests are
   debounced and stale work is cancelled;
6. inspect loci, all eight summary counters, nodes, edges, local weighted
   traversals, archive provenance, request ranges, and canonical hashes;
7. copy a canonical coordinate or shareable remote URL and use browser history
   to revisit genomic viewports.

Local transforms keep pointer interaction immediate. The application tracks
the visual viewport, settled requested interval, loaded interval, and a
predicted adjacent interval separately. Adjacent data is not fetched until a
controlled benchmark establishes a benefit; the evidence panel labels this
state explicitly.

## Levels of detail

`selectLevelOfDetail()` is deterministic and summary-driven. It chooses a
summary level targeting approximately one bin per four horizontal pixels, then
tests encoded bytes, decoded bytes, node records, edge records, and occurrence
records against explicit render/query budgets. Span alone never authorizes a
detailed query.

- **Overview:** summary tracks only.
- **Regional:** summary context plus an explicit decision to load detail.
- **Detailed:** progressively streamed topology and tile-local traversal
  evidence.
- **Base:** the same bounded graph with sequence labels and node orientation
  when the pixel budget makes them legible.

Advanced users may override the automatic detail refusal. The resulting model
and canvas budgets still cap retained graph objects and traversal lanes.

## Detailed layout and inspection

The viewer places the real reference traversal on a coordinate spine in stable
reference order. Alternate components are assigned to their nearest reference
anchors and colored into deterministic non-overlapping lanes. Insertions,
inversions, deletions/bypasses, and unanchored components have distinct visual
forms. Progressive tile completion order cannot reorder already known
reference nodes or change stable alternate lane colors.

Node, edge, and traversal hit testing returns source-tile provenance. Traversal
rows remain grouped by source tile and ranked by exact integer weight; they are
never merged into a synthetic cross-tile sample. The inspector exposes the
available regional payload range and sizes without inventing fields the format
does not contain.

## Interaction and accessibility

The explorer supports visible focus, semantic buttons and inputs, an ARIA live
status region, a keyboard-help panel, `Ctrl/Cmd+K`, arrow/Enter search
selection, keyboard pan/zoom, Escape cancellation, high-DPI rendering,
reduced-motion preferences, persistent layers/theme, and responsive tablet
panels. Light and dark screenshots are retained as browser evidence.

## Acceptance and evidence

The hermetic Pages test requires real strict `206` responses, a populated and
absent named-index state, coordinate and prefix navigation, hover/selection,
local-file isolation, custom URL loading, stale-load cancellation, actionable
bad-range errors, browser history-compatible URLs, all three browser engines,
and desktop/dark/tablet screenshots. Reader/viewer unit tests cover command
parsing, LOD selection, hit testing, and progressive layout stability.

Measured limitations and outstanding product work are recorded in
[Viewer performance](VIEWER_PERFORMANCE.md) and
[Viewer format gaps](VIEWER_FORMAT_GAPS.md).
