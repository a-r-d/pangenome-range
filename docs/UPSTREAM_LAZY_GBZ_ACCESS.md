# Upstream issue draft: bounded read-only GBZ access

## Problem

`gbz` 0.7.0 plus `simple-sds` 0.4.2 fully deserializes a GBZ before the first
regional payload can be selected. A 5,492,627,216-byte HPRC v2.1 source reached
8,776,080 KiB peak RSS even when the downstream encoder was restricted to two
16 KiB chunks. The archive writer itself used no occurrence index, spool, or
general scratch.

## Minimal API needed

A read-only adapter should provide, with borrowed or bounded decoded state:

- metadata and reference-path discovery;
- node sequence and sequence-length lookup;
- packed GBWT record lookup by oriented handle;
- reference path handle/fragment lookup;
- sparse indexed reference-position lookup and LF continuation.

The repository-side `PangenomeSource` trait is the experiment seam. The
existing fully loaded adapter remains the oracle.

## Constraints

- no row or tuple per global haplotype visit;
- no full haplotype expansion;
- no SQLite/GBZ-base occurrence table;
- deterministic borrowed records equal to the current in-memory decoder;
- checked section offsets and lengths;
- bounded caches with explicit byte limits;
- thread-safe read-only access, or explicit per-worker bounded handles.

## Proposed pilot

1. mmap only the underlying simple-sds sections needed for metadata, sequences,
   GBWT records, and sparse reference positions;
2. encode `GRCh38#chr6` for two chunks and compare bytes with
   `LoadedGbzSource`;
3. extend to one chromosome while recording mmap residency/RSS, page faults,
   source reads, time to first payload, and cache peaks;
4. authorize a whole-source attempt only if memory remains bounded by mapped
   source residency plus the documented encoder queue/chunk budget.

Success means byte-identical archive output and source-oracle hashes with a
measured bounded working set. Merely moving the same full deserialization
behind mmap without measuring residency is not sufficient.
