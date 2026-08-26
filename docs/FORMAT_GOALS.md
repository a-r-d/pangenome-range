# Format goals and current research candidate

Archive v4 (`PNGRNG04` with `PNGRGN04` record-preserving regional payloads) is
the current measured research candidate. It is implemented by Rust and the
browser reader, has cross-language golden fixtures, and has completed a
whole-source build plus real HTTP range smoke. It is not yet a stable public
format: incompatible changes still require an explicit new version and full
cross-language conformance tranche.

Candidate experiments should pursue and measure:

- very few range requests, including few serial dependency rounds;
- small bootstrap metadata, ideally cacheable independently of region payloads;
- low read amplification and low duplicate-byte retrieval;
- minimal total-size expansion relative to the same source GBZ;
- independently decompressible regions or blocks;
- unsigned 64-bit offsets and lengths wherever serialized;
- streamable, bounded-memory construction if practical;
- browser-decodable primitives with no JavaScript binding requirement yet;
- immutable static-object hosting on S3, R2, GCS, or a conventional HTTP CDN;
- no required database server or custom query backend;
- preservation of node, edge, haplotype/path, reference-path, and coordinate
  semantics;
- deterministic validation and corruption detection;
- explicit behavior for absent metadata, fragmented paths, large nodes, and
  pathological/collapsed graph regions.

Questions still open include whether a future candidate should share compressed
records across tiles, how much archive expansion is acceptable, whether a small
sidecar is worth comparing with a single object, and whether true sample
continuation identity merits a separately costed mode. Each answer belongs in a
named experiment with measurements; old version bytes are never reinterpreted.
