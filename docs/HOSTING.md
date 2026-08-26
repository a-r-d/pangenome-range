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

## Connect the deployed demo

The repository contains no production archive URL. Set the GitHub Actions
repository variable `PANGENOME_RANGE_DEMO_ARCHIVE_URL` to the final immutable
HTTPS `.pngr` URL. The Pages workflow maps it to:

```text
VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL
```

Run the Pages workflow again. The demo will then offer **Configured external
archive** while retaining the deterministic bundled fixture and local-file
fallback. Test the deployed page in a fresh browser session and confirm its
request panel contains `payload` ranges rather than a whole-object response.
