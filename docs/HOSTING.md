# Hosting an archive

The demo reads an immutable `.pngr` object directly. It does not need and must
not be given a query API or a GitHub Pages proxy. The configured object can live
on an ordinary static server, object store, CDN, or a static server exposed by a
Cloudflare Tunnel.

## Required response contract

For a request such as `Range: bytes=16384-20479`, the archive origin must return:

```http
HTTP/1.1 206 Partial Content
Accept-Ranges: bytes
Content-Range: bytes 16384-20479/TOTAL_OBJECT_BYTES
Content-Length: 4096
Content-Type: application/octet-stream
ETag: "A-STABLE-OBJECT-IDENTIFIER"
Access-Control-Allow-Origin: https://a-r-d.github.io
Access-Control-Expose-Headers: Accept-Ranges, Content-Range, Content-Length, ETag
Cache-Control: public, max-age=31536000, immutable, no-transform
```

`Access-Control-Allow-Origin: *` is also appropriate for a completely public
immutable archive. Do not transparently gzip, recompress, or otherwise transform
`.pngr` bytes. The ETag must remain stable for the life of a content-addressed
URL. A changed object belongs at a new URL.

The browser reader validates `206`, `Content-Range`, response length, and object
identity. It intentionally fails instead of silently downloading a large object
when an origin ignores `Range` and responds with `200`.

## Probe an origin before publishing it

Build the private benchmark CLI and compare several public byte ranges with the
known local archive:

```bash
pnpm --filter @pangenome-range/benchmark build
pnpm bench -- origin-check \
  --url "$PANGENOME_RANGE_ARCHIVE_URL" \
  --origin "https://a-r-d.github.io" \
  --file /srv/pangenome/archive.pngr \
  --sha256 "$PANGENOME_RANGE_ARCHIVE_SHA256"
```

This checks response status, range headers, CORS exposure, cache policy, ETag
stability, lengths, and byte equality. A successful `curl` or a generic `2xx`
response alone does not exercise the required range path.

## Cloudflare Tunnel deployment

Run a range-capable static server on the machine that owns the archive. Point a
Tunnel ingress hostname at that local server; the tunnel carries HTTP but does
not replace the server's range or CORS configuration.

```yaml
# ~/.cloudflared/config.yml on the archive host
tunnel: YOUR_TUNNEL_ID
credentials-file: /path/to/YOUR_TUNNEL_ID.json
ingress:
  - hostname: archive.example.org
    service: http://127.0.0.1:8080
  - service: http_status:404
```

Create the DNS route with `cloudflared tunnel route dns YOUR_TUNNEL_ID
archive.example.org`, then probe the final HTTPS URL with the command above.
Disable any Cloudflare rule that transforms or compresses `.pngr` responses;
retain the origin's `Content-Range`, CORS, ETag, and `no-transform` headers.

## Canonical demo archives

The Pages workflow defaults to the content-addressed HPRC v2.1 / GENCODE v50
archive at:

```text
https://archives.ard.ninja/pangenome-range/sha256/ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63/hprc-v1-gencode-v50-disk-t8.pngr
```

Its byte length is 8,832,749,949 and its SHA-256 is:

```text
ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63
```

The current origin host serves that object from:

```text
/srv/data/public-archives/pangenome-range/sha256/ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63/hprc-v1-gencode-v50-disk-t8.pngr
```

This is a deployed viewer/demo object, distinct from the later 677-byte-larger
release-hardening measurement archive carrying the new provenance extension.
The content-addressed path prevents the demo object from being mistaken for
those later bytes.

The source menu also includes the content-addressed PPanG rice chromosome 6
archive:

```text
https://archives.ard.ninja/pangenome-range/sha256/c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c/rice-chr06-mc-xa7-anonymous.pngr
```

It is 325,664,519 bytes with SHA-256
`c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c`.
The workflow maps an optional `PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL`
repository variable to `VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL`.

The named-membership chicken chromosome 15 archive is published at:

```text
https://archives.ard.ninja/pangenome-range/sha256/fcb19b2c6e850c16e7e831613f34d27feef477331064cde5b16137492e6d1b43/chicken-chr15-named.pngr
```

It is 15,939,592 bytes with SHA-256
`fcb19b2c6e850c16e7e831613f34d27feef477331064cde5b16137492e6d1b43`.
The source graph is CC BY 4.0; provenance and attribution are retained in the
archive and `results/named-membership/chicken/REPORT.md`. The origin probe passed
range, CORS, immutable-cache, ETag, and local-byte-equality checks.

## Connect the deployed demo

The checked-in Pages workflow supplies the canonical HPRC, 1000 Genomes, rice,
and chicken URLs. Poplar is an opt-in source because its derived-data redistribution
terms are not yet confirmed. Repository variables named `PANGENOME_RANGE_DEMO_ARCHIVE_URL`,
`PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL`, and
`PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL`, and
`PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL` may replace the defaults with other immutable
HTTPS `.pngr` URLs without a code change. `PANGENOME_RANGE_DEMO_POPLAR_ARCHIVE_URL`
adds the locally validated Chr19 archive only after data-use approval. The workflow
maps them to:

```text
VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL
VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL
VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL
VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL
VITE_PANGENOME_RANGE_DEMO_POPLAR_ARCHIVE_URL
```

The local Poplar candidate is 75,902,344 bytes with SHA-256
`baf33e2e181efa4485f8ea2a253b24e4bda08ef5725c4ddf9585b495ddafe6ae`.
Do not copy it to the public origin until the review in `data/poplar/` is resolved.

The deployed demo selects **Configured external archive** by default while
retaining the deterministic bundled fixture, custom URL, and local-file
fallbacks. Test the deployed page in a fresh browser session and confirm its
request panel contains `payload` ranges rather than a whole-object response.
