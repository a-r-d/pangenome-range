# ADR 0003: implement a small reference-anchored SVG renderer

- Status: accepted
- Date: 2026-08-27

## Context

The first demo used a general Canvas controller and exposed multiple product
modes around a dense raw graph. The replacement needs a recognizable tube map,
DOM hit targets, crisp sequence labels, bounded complexity, and no backend.

SequenceTubeMap was inspected at its current `vgteam/sequenceTubeMap` source,
including `src/util/tubemap.js` and its React wrapper. Its MIT license permits
reuse, and its reference-first ordering, curved paths, orientation marks,
redundant-node handling, and hover emphasis are useful interaction precedents.
The core, however, is a mutable module of more than 5,000 lines coupled to D3,
deep-copied VG tracks and reads, React application state, and broader track and
server infrastructure. Extracting it would require a compatibility layer larger
than the bounded renderer needed here.

## Decision

Implement a local framework-neutral pipeline:

```text
buildTubeMapModel() -> layoutTubeMap() -> renderTubeMapSvg()
```

The real reference traversal fixes horizontal order. Alternate components use
deterministic reference attachments and lanes; there is no force simulation.
SVG supplies accessible node and pattern hit targets. The adapter, not the
renderer, owns deterministic pattern selection, tile-local identifiers,
structural collapse, provenance, and refusal limits.

No SequenceTubeMap source was copied or adapted. Consequently this change does
not add a third-party code notice. The project license remains unchanged.

## Consequences

The public viewer bundle is substantially smaller and no longer imports the old
Canvas application. The renderer is intentionally less general than
SequenceTubeMap and currently uses display-oriented structural bundles for
complex fixed-window payloads. Exact node membership remains inspectable, but
the collapsed shape is not an exact genomic-length encoding. A future renderer
change must preserve the framework and transport boundaries and must be backed
by a new golden fixture and browser evidence.
