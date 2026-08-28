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
Toolbar controls expose concise hover titles. The three graph options also
provide keyboard-focusable explanations of chain collapse, sequence-letter
display, and source-tile boundaries. **Source** opens the configured archive,
a remote `.pngr` URL, or a local file. **Share** opens a modal containing the
exact current region URL, a copy action, and visible success or fallback
feedback; it never relies on a transient status-row message alone.
The linear reference context keeps coordinate labels below the selected-locus
highlight boundary so its border never crosses readable coordinate text.

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
Home reset, drag pan, and independent vertical-lane compression/expansion are
local SVG transforms and do not issue archive requests. Vertical spacing does
not change genomic scale or reference coordinates. Fit resets horizontal pan,
fits the reference between the side margins, and selects the largest vertical
lane scale that keeps the current nodes, topology, patterns, and labels inside
the viewport padding. Wheel deltas use a continuous bounded scale instead of
treating each trackpad event as a fixed zoom step, and repaints are coalesced to
animation frames.

The ordinary desktop display budget is 2,500 node groups and 5,000 topology
edges. These are renderer-responsiveness limits, not archive, query, or data
integrity limits. The renderer does not silently truncate beyond them. A
refused view explains whether linear chains are collapsed, offers the existing
40 kb recommendation, and permits an explicit **Open anyway** override up to a
finite 10,000 node-group / 20,000-edge safety ceiling. The override is scoped
to the current region and resets on navigation or when chain simplification is
changed.

At locus-fit scale labels are suppressed or shortened when their shapes are too
narrow; they return in full as the user zooms. Node labels use deterministic
10/9/8/7/6.5-pixel tiers. A narrow collapsed node may show a count such as
`3×`; a long node ID may show an ellipsis plus its distinguishing suffix. The
full exact ID remains in the accessibility label and inspector, and even the
short form is hidden when it cannot fit inside the node. Pattern labels sit
outside their lanes with enough deterministic vertical separation to avoid
label collisions.
Colored traversal lanes fan out only between nodes, then converge into bounded
ports inside each visited node. Connectors render behind opaque nodes; only a
short, lower-opacity, reduced-width port stub renders over a node edge. Visible
stroke width decreases at overview zoom and as the selected pattern count grows,
while a transparent 12-pixel hit path preserves pointer and keyboard usability.
One compound SVG port path per pattern keeps the attachment visible without
creating an element per visit or obscuring the center label. Selecting one
pattern emphasizes its complete connector/port route and mutes the other local
evidence.

## Inspection and sources

Clicking a node opens a bounded drawer with its sequence preview, orientation,
boundary IDs for a collapsed chain, displayed neighbor counts, byte provenance,
and source-tile coordinates. Inspection does not unexpectedly expand the graph;
a collapsed chain has an explicit expansion action while retaining all member
IDs in the model. Clicking a pattern opens its exact integer weight, oriented
visit count, source tile, and the semantic warning above. Escape or an empty
graph click closes a drawer.

The Source menu presents the configured HPRC, 1000 Genomes, and PPanG rice
archives as named presets in a dropdown, plus the bundled fixture, another
range-capable URL, and a local `.pngr` file. It identifies the 1000 Genomes
source as NA19239 haplotype-0 population-path coordinates rather than GRCh38
and makes the lack of named-locus annotations explicit. It identifies the rice
source as NATELBORO chromosome 6 with anonymous weighted tile-local traversals,
not named accessions. Local files stay in the browser. The normal reader and
viewer exports contain no Vue, VitePress, Node built-ins, or native launcher.

The URL contract restores the source (`archive`), either a named `locus` or an
exact `sample`/`contig`/`start`/`end` region, horizontal `zoom`, normalized
horizontal `center`, and vertical `vscale`. Normalized center is independent of
the recipient's viewport width. Legacy `configured` and `population` source
values remain accepted as aliases. Custom remote URLs may be linked explicitly;
local file handles cannot be restored by a URL because browser security requires
the user to choose the file again.

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
expansion, vertical-lane controls, zoom/density-responsive pattern weight, and
explicit chain expansion. The configured-archive gate also checks that all
narrow alternate nodes in CHAD receive non-colliding adaptive labels with 16
patterns visible. It remains conditional on the configured URL and retains
HLA-B plus lower-complexity and dense-label locus screenshots.

Outstanding scientific and performance constraints remain in
[Viewer format gaps](VIEWER_FORMAT_GAPS.md) and
[Viewer performance](VIEWER_PERFORMANCE.md).
