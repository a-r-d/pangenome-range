# Research hypothesis and baselines

## Why GBZ matters

GBZ combines GBWT path compression with graph sequences. It is path-first: nodes
and edges exist through path use, which makes repeated haplotypes compress
exceptionally well. In the published 1000 Genomes Project experiment, 5,008
haplotypes represented about 3.2 billion graph bases and 2.1251 trillion node
traversals. The same dataset occupied 9,534.9 GiB as GFA, 2,231.3 GiB as
gzip-compressed GFA, and only 16.84 GiB as GBZ. Any range-oriented alternative
must measure its size expansion against that result, not merely against GFA.

The [GBZ paper](https://doi.org/10.1093/bioinformatics/btac656) describes a
format optimized for compactness and fast loading into in-memory GBWT/GBWTGraph
structures. That is an excellent batch/local design. It does not by itself imply
that a client can discover and fetch a small genomic interval using a few HTTP
ranges.

## A separate remote-access problem

Remote interactivity makes latency and locality first-class constraints. A
layout may be compact yet require serial metadata lookups or scattered small
reads. Conversely, an aggressively tiled layout may answer quickly but lose the
population-scale redundancy that makes GBZ valuable. The research question is
whether we can find a useful point on this frontier:

- small bootstrap metadata and few dependent range requests;
- low byte/read amplification for reference-interval queries;
- independently decodable regions suitable for static object hosting;
- modest size expansion relative to the source GBZ;
- exact reconstruction of local graph, path, and coordinate semantics.

The intended deployment has no required database server or custom query backend.
An HTTP server with standards-compliant byte ranges should eventually suffice.

## GBZ-base is a baseline, not a foil

[GBZ-base](https://doi.org/10.64898/2026.07.10.737775) stores GBZ-like graph
records and reference-position indexes in SQLite for interactive local queries.
It preserves much of the GBWT record representation and supports extracting
local subgraphs, context, snarls, and haplotype traversals. The paper's HPRC v2.1
experiment reports 10.7 GiB for GBZ-base versus 5.7 GiB for GBZ, and recommends a
local SSD for best performance.

That is a strong local-access baseline and an oracle candidate. It should be
benchmarked on the same query corpus and hardware. This project explores a
different constraint—few static-object HTTP ranges—not an assumption that SQLite
or GBZ-base is intrinsically unsuitable.

## Why PMTiles and COG are useful analogies

[PMTiles v3](https://github.com/protomaps/PMTiles/blob/master/spec/v3/spec.md)
puts a fixed header and root directory in the first 16 KiB, then uses optional
leaf directories to find independently stored tile payloads. It demonstrates
bounded bootstrap reads, hierarchical lookup, 64-bit offsets, clustering, and a
single immutable object. We should test those ideas; pangenome regions are not
map tiles, so the directory key and payload organization remain open questions.

[Cloud Optimized GeoTIFF](https://github.com/cogeotiff/cog-spec) keeps the base
format but orders metadata, overviews, and tiled imagery so a range client can
fetch initial metadata and relevant blocks. It demonstrates that internal
ordering, independent blocks, and coarse-to-fine representations can turn a
static file into a remote query surface without a server-side database.

These are design analogies, not evidence that genomic graphs should copy either
format literally.

## Current data caution

The HPRC resource repository lists v2.1 Minigraph-Cactus whole-genome and
per-chromosome GBZ resources, but currently warns that year-2 graphs have not
been fully QC'd, are not published, and may have known issues. Tier 2/3 work must
record exact object URLs/checksums and follow the linked HPRC Data Use Protocol.

## Falsifiable working hypothesis

A useful representation should reconstruct the same local semantics as GBZ while
substantially reducing remote request count and transferred bytes relative to
opening/deserializing the full GBZ. The hypothesis fails if candidate layouts
require unacceptable size expansion, cannot bound lookup round trips, or lose
path/reference semantics. Results should show the tradeoff rather than hiding a
failure behind one headline metric.
