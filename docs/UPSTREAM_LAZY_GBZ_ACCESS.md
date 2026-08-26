# Upstream issue draft: bounded read-only GBZ access

## Problem

`gbz` 0.7.0 plus `simple-sds` 0.4.2 exposes only full deserialization for a GBZ.
The repository has now worked around that limitation with a project-owned
disk-backed adapter, but doing so requires parsing locked upstream serialization
details. An official bounded interface would reduce that maintenance burden.

The final whole encode of the same 5,492,627,216-byte HPRC source fell from
8,775,928 to 608,060 KiB peak RSS. It produced the identical 8,828,788,418-byte
archive and used 11,921,858,427 bytes of ephemeral cache. Whole wall increased
from 438.72 to 499.34 seconds. The archive writer itself still used no
occurrence index, payload spool, or general scratch.

## Minimal API needed

A read-only adapter should provide, with borrowed or bounded decoded state:

- metadata and reference-path discovery;
- node sequence and sequence-length lookup;
- packed GBWT record lookup by oriented handle;
- reference path handle/fragment lookup;
- sparse indexed reference-position lookup and LF continuation.

The repository-side `PangenomeSource` trait is the production seam.
`DiskGbzSource` is the default; the fully loaded adapter remains the oracle.

## Constraints

- no row or tuple per global haplotype visit;
- no full haplotype expansion;
- no SQLite/GBZ-base occurrence table;
- deterministic borrowed records equal to the current in-memory decoder;
- checked section offsets and lengths;
- bounded caches with explicit byte limits;
- thread-safe read-only access, or explicit per-worker bounded handles.

## Proposed pilot

1. expose metadata, packed-record, and sequence sections without full load;
2. expose validated serialized offsets or a stable bounded random-access API;
3. compare an upstream implementation with the retained project-owned HPRC
   whole run for bytes, RSS, faults, source reads, and preprocessing time;
4. replace the project parser only after exact archive identity is proven.

Success means byte-identical archive output and source-oracle hashes with a
measured bounded working set. Merely moving the same full deserialization
behind mmap without measuring residency is not sufficient.
