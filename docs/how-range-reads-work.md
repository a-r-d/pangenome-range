---
outline: deep
---

<script setup>
import RangeReadAnimation from './components/RangeReadAnimation.vue'
</script>

# How range reads work

A `.pngr` archive is one immutable byte object. A browser answers a genomic
region with ordinary HTTP `Range` requests: a small bootstrap, one arithmetic
directory lookup, and one parallel payload round. There is no query server.

<ClientOnly>
  <RangeReadAnimation />
</ClientOnly>

The animation uses the published HPRC HLA-B shape: five range reads and about
74 KB from an 8.8 GB object. Offsets inside the diagram are schematic; the
three-round dependency is not.

## The object

```text
64-byte PNGRNG01 header
variable root manifest
optional versioned extension directory
contiguous fixed 4 KiB arithmetic directory pages
independently compressed PNGRGN01 regional payloads
```

The header is always 64 bytes at offset 0. It names the root and the first byte
after the directory pages (`data_offset`). The root names each real reference
sample/contig, its coordinate span, window size, bucket span, and where that
reference’s 4 KiB pages begin. Payloads never have to be parsed to find a
region.

## Three dependency rounds

1. **Bootstrap.** The reader fetches 16 KiB from offset 0. That covers the
   header, the root, and usually the extension directory. From the chr6
   manifest it knows `grid_start`, `bucket_span`, and `first_page_offset`.
2. **Directory.** Coordinates are not searched. They are converted to a page:

   ```text
   page_offset = first_page_offset
               + floor((q_start − grid_start) / bucket_span) × 4096
   ```

   For window 16,384 bp, `bucket_span` is 524,288 bp. HLA-B at 31,353,194 lands
   on page 59. That page is 4 KiB and holds at most 72 entries. Adaptive splits
   stay inside the parent bucket, so this is not a second index level.
3. **Payloads.** Each overlapping entry names an absolute payload offset,
   compressed length, uncompressed length, and BLAKE3-128. Those ranges are
   fetched in one parallel round, integrity-checked, decompressed as a single
   zstd frame, and decoded as `PNGRGN01`. The rest of the object is not read.

Bootstrap bytes that already cover a needed directory page are reused. A later
query against the same archive hits the bootstrap/root cache and does not
repeat round 1.

## Why this layout

Population graphs are huge, but a browser usually needs one locus. Putting a
row in the file for every haplotype visit does not scale, and neither does a
custom query daemon. Fixed-size directory pages plus independently compressed
tiles keep the served representation a static object: any origin that can
answer `206 Partial Content` is enough.

Named-path membership, gene search, and summary pyramids are optional
extensions on the same object. They are not required to decode a regional
graph.

Normative layout: [File Format v1](./FILE_FORMAT_V1.md). Design notes:
[fixed-window archive](./FIXED_WINDOW_ARCHIVE.md).
