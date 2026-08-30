# Format goals and current research candidate

File-format v1 (`PNGRNG01` with `PNGRGN01` record-preserving payloads) is
the only current format. Rust and TypeScript implement it from the normative
[File Format v1](FILE_FORMAT_V1.md), share one golden conformance matrix, and
have completed a whole-source build plus real HTTP-range smoke. The project is
still unreleased and the format is not stable.

Candidate work should pursue and measure:

- very few range requests and serial dependency rounds;
- small independently cacheable bootstrap metadata;
- low read amplification and duplicate-byte retrieval;
- minimal expansion relative to the same source GBZ;
- independently decompressible regions;
- unsigned 64-bit offsets and lengths;
- deterministic, bounded-memory construction;
- browser decoding without native bindings;
- immutable static-object hosting without a custom query backend;
- complete node, edge, reference, coordinate, multiplicity, weight, and
  provenance semantics;
- deterministic validation and corruption detection;
- explicit bounds for collapsed graph regions and malformed inputs.

Open research questions include compressed-record sharing, acceptable archive
expansion, optional sidecars, and archive-wide construction/query cost for named
identity. Each answer belongs in a named, correctness-gated experiment.

Optional named source-path membership is now a registered v1 feature. Direct bounded
construction, persistent-cache reuse, Rust validation, and the public TypeScript API
are implemented. Fixed membership-directory pages scale with graph directory pages,
not a root-wide tile list. The feature preserves tile-local GBWT source identity; it
does not redefine anonymous graph-query traversals as globally stitchable samples.
Whole-HPRC construction remains unmeasured and 1000G remains an explicit memory-safety
exclusion, not an inferred result.

While the project remains pre-release, an incompatible format change replaces
v1 in place and regenerates all fixtures. It does not add a compatibility
decoder or reinterpret an old research object. Package semantic versions remain
independent of the on-disk v1 identifier.
