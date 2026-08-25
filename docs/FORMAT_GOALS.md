# Format goals, not a format

No archive layout, magic bytes, block size, directory key, compression codec, or
on-disk schema is selected at this stage.

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

Questions deliberately left open include whether indexes are hierarchical,
whether payloads are reference-windowed or graph-structural, how shared
haplotype information crosses region boundaries, which compression units are
independent, and whether a small sidecar is worth comparing with a single-object
layout. Each answer belongs in a named experiment with measurements.

