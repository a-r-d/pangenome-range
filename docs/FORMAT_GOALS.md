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
expansion, optional sidecars, and the byte/construction/query cost of true sample
continuation identity. Each answer belongs in a named, correctness-gated
experiment.

While the project remains pre-release, an incompatible format change replaces
v1 in place and regenerates all fixtures. It does not add a compatibility
decoder or reinterpret an old research object. Package semantic versions remain
independent of the on-disk v1 identifier.
