# Viewer product requirements

## Product contract

`/demo` is a browser-native scientific viewer over one immutable `.pngr`
object. It opens the configured archive and `HLA-B` directly; there is no
showcase, dashboard, query backend, or separate overview application. The
bundled deterministic archive remains the offline fallback.

The four-row desktop shell owns the viewport:

```text
52 px    search and navigation toolbar
112 px   linear reference context
flex     reference-anchored SVG tube map
30 px    range and integrity status
```

Anonymous traversals retain the serialized
`anonymous-distinct-weighted-tile-paths` semantics. They are distinct,
integer-weighted evidence local to one source tile. They are not named people,
allele frequencies, or globally stitchable haplotypes.

## Navigation and loading

The location field accepts archive-native locus names and canonical
coordinates. Prefix suggestions come only from `archive.searchLoci()`.
Arrow keys, Enter, Escape, `Ctrl/Cmd+K`, and `/` work without a second command
palette. Browser history and the query string preserve exact regions.

Every region first calls `archive.planRegion()`. A detailed query is allowed
only when all three limits hold:

- span at most 100 kb;
- exact compressed payload bytes at most 4 MiB;
- at most eight physical payloads.

An oversized request retains the ruler and exact plan, performs no regional
payload read, and offers a deterministic 40 kb recommendation. Optional
`summary()` bins appear only as a thin context strip when at least four bins
are visible. They never replace exact directory-derived planning values.

Allowed queries stream through `archive.queryTiles()`. A newer source, search,
or history navigation aborts stale work. The preceding graph remains visible
until the first replacement tile is decoded. The status row reports archive
size, completed tiles, bytes, byte-range count, wall time, and integrity state.

## Tube-map model and rendering

The public `/viewer` entry has three framework-neutral stages:

```text
RegionTile[] -> buildTubeMapModel() -> layoutTubeMap() -> renderTubeMapSvg()
```

The adapter sorts tiles, selects the top 0/4/8/16 patterns, assigns tile-local
identifiers, records source provenance, and collapses bounded structural
segments. Pattern ordering is weight descending, tile start ascending, then
lexicographic oriented-node sequence. Selected patterns terminate at their
source tile and are never joined across a boundary.

The real reference traversal defines horizontal order. Reference segments are
dominant, alternate components use deterministic lanes above and below it,
skip edges arc across the backbone, and reverse nodes use a reversed notch.
Widths encode sequence length only approximately; the ruler is the exact
coordinate source. Fit, pointer-anchored wheel/pinch zoom, double-click zoom,
Home reset, and drag pan are local SVG transforms and do not issue archive
requests. Wheel deltas use a continuous bounded scale instead of treating each
trackpad event as a fixed zoom step, and repaints are coalesced to animation
frames.

Default post-collapse refusal limits are 400 displayed node groups and 800
topology edges. The renderer does not silently truncate beyond those limits.
At locus-fit scale labels are suppressed or shortened when their shapes are too
narrow; they return in full as the user zooms. Pattern labels sit outside their
lanes with enough deterministic vertical separation to avoid label collisions.

## Inspection and sources

Clicking a node opens a bounded drawer with its sequence preview, orientation,
boundary IDs for a collapsed chain, displayed neighbor counts, byte provenance,
and source-tile coordinates. Inspection does not unexpectedly expand the graph;
a collapsed chain has an explicit expansion action while retaining all member
IDs in the model. Clicking a pattern opens its exact integer weight, oriented
visit count, source tile, and the semantic warning above. Escape or an empty
graph click closes a drawer.

The archive menu supports the configured URL, another range-capable URL, and a
local `.pngr` file. Local files stay in the browser. The normal reader and
viewer exports contain no Vue, VitePress, Node built-ins, or native launcher.

## Evidence requirements

Unit tests cover parsing, region policy, recommendations, deterministic pattern
selection, reference order, lane placement, reverse orientation, collapse and
expansion, thickness bounds, and stale cancellation. A checked-in two-tile
golden model exercises an insertion branch, deletion-like skip, reverse node,
collapsed chain, and four weighted local patterns on `/tube-map-lab`, which is
excluded from documentation navigation.

The hermetic Pages test requires real `206` requests, coordinate and named-locus
navigation, history, pan/zoom controls, node and pattern inspection, custom URL,
local-file isolation, the golden route, no desktop overflow, and Chromium,
Firefox, and WebKit. Public-archive validation also checks bounded
pointer-anchored wheel zoom, label collisions, inspection without implicit
expansion, and explicit chain expansion. It remains conditional on the
configured URL and retains HLA-B plus a lower-complexity locus screenshot.

Outstanding scientific and performance constraints remain in
[Viewer format gaps](VIEWER_FORMAT_GAPS.md) and
[Viewer performance](VIEWER_PERFORMANCE.md).
