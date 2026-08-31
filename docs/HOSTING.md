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
archive with exact named GBWT source-path membership at:

```text
https://archives.ard.ninja/pangenome-range/sha256/82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a/hprc-v2.1-gencode-v50-named-membership-82585cb612effbf4.pngr
```

Its byte length is 10,836,425,558 and its SHA-256 is:

```text
82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a
```

The current origin host serves that object from:

```text
/srv/data/public-archives/pangenome-range/sha256/82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a/hprc-v2.1-gencode-v50-named-membership-82585cb612effbf4.pngr
```

This archive contains 363,105 membership tile pages, 75,587,329 canonical
traversal groups, 174,838,191 memberships, and 53,150 catalog records. Full
structural validation and the retained seven-query GBZ source oracle both
passed before publication. Named identity remains an explicit experimental
extension; graph-only readers continue to use the unchanged regional payloads.

The source menu also includes the content-addressed PPanG rice chromosome 6
archive:

```text
https://archives.ard.ninja/pangenome-range/sha256/c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c/rice-chr06-mc-xa7-anonymous.pngr
```

It is 325,664,519 bytes with SHA-256
`c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c`.
The workflow maps an optional `PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL`
repository variable to `VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL`.

The complete named-membership chicken pangenome archive is published at:

```text
https://archives.ard.ninja/pangenome-range/sha256/93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e/chicken-whole-named.pngr
```

It is 1,498,984,132 bytes with SHA-256
`93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e`.
It covers all 207 `bGalGal1b` reference paths (1,052,949,595 reference bases),
contains 12,237 exact source-path catalog entries, and includes 25,437 mapped
gene rows. The former chromosome 15 object remains available as retained
bounded evidence, but it is no longer the demo default.
The Zenodo graph file is CC BY 4.0. Provenance, license, and attribution are
retained in `data/chicken/sources.json`. The origin probe passed range, CORS,
immutable-cache, ETag, checksum, and local-byte-equality checks.

## Connect the deployed demo

The checked-in Pages workflow supplies the canonical chicken, HPRC, 1000 Genomes,
and rice URLs. Repository variables named `PANGENOME_RANGE_DEMO_ARCHIVE_URL`,
`PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL`, and
`PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL`, and
`PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL` may replace the defaults with other immutable
HTTPS `.pngr` URLs without a code change. The workflow maps them to:

```text
VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL
VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL
VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL
VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL
```

The deployed demo selects the whole chicken archive by default when it is
configured, while
retaining the deterministic bundled fixture, custom URL, and local-file
fallbacks. Test the deployed page in a fresh browser session and confirm its
request panel contains `payload` ranges rather than a whole-object response.
