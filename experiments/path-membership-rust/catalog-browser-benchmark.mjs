import assert from "node:assert/strict";
import { readFile, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import {
  chromium,
  firefox,
  webkit,
} from "../../packages/benchmark/node_modules/@playwright/test/index.mjs";

const [pagedPath, buildStatsPath, queryIdsPath, catalogPath, outputPath] =
  process.argv.slice(2);
if (!outputPath) {
  throw new Error(
    "usage: node catalog-browser-benchmark.mjs PAGED BUILD_STATS QUERY_IDS CATALOG OUTPUT",
  );
}

const workspace = resolve(import.meta.dirname, "../..");
const runtimePath = resolve(import.meta.dirname, "catalog-browser-runtime.mjs");
const nobleRoot = resolve(
  workspace,
  "packages/browser/node_modules/@noble/hashes",
);
const fzstdPath = resolve(
  workspace,
  "packages/browser/node_modules/fzstd/esm/index.mjs",
);
const paged = await readFile(pagedPath);
const buildStats = JSON.parse(await readFile(buildStatsPath, "utf8"));
const queryIds = (await readFile(queryIdsPath, "utf8"))
  .split(/\s+/)
  .filter(Boolean)
  .map(Number);
const querySet = new Set(queryIds);
const expected = (await readFile(catalogPath, "utf8"))
  .trim()
  .split("\n")
  .map(JSON.parse)
  .filter(
    (value) => value.type === "path" && querySet.has(value.canonical_path_id),
  )
  .map(
    ({
      canonical_path_id,
      raw_name,
      sample,
      contig,
      haplotype,
      fragment,
      path_sense,
    }) => ({
      canonical_path_id,
      raw_name,
      sample,
      contig,
      haplotype,
      fragment,
      path_sense,
    }),
  )
  .sort((left, right) => left.canonical_path_id - right.canonical_path_id);
const etag = '"paged-catalog-experiment"';
const requests = [];

function sendModule(response, bytes) {
  response.writeHead(200, {
    "Content-Type": "text/javascript; charset=utf-8",
    "Content-Length": bytes.byteLength,
  });
  response.end(bytes);
}

const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, "http://127.0.0.1");
    if (url.pathname === "/") {
      const body = Buffer.from(
        "<!doctype html><title>paged catalog benchmark</title>",
      );
      response.writeHead(200, {
        "Content-Type": "text/html; charset=utf-8",
        "Content-Length": body.byteLength,
      });
      response.end(body);
      return;
    }
    if (url.pathname === "/runtime.mjs") {
      sendModule(response, await readFile(runtimePath));
      return;
    }
    if (url.pathname === "/fzstd.mjs") {
      sendModule(response, await readFile(fzstdPath));
      return;
    }
    if (url.pathname.startsWith("/noble/") && url.pathname.endsWith(".js")) {
      const filename = url.pathname.slice("/noble/".length);
      if (filename.includes("/") || filename.includes("..")) {
        response.writeHead(404).end();
        return;
      }
      sendModule(response, await readFile(resolve(nobleRoot, filename)));
      return;
    }
    if (url.pathname !== "/catalog") {
      response.writeHead(404).end();
      return;
    }
    const range = request.headers.range?.match(/^bytes=(\d+)-(\d+)$/);
    if (!range) {
      response.writeHead(416).end();
      return;
    }
    const start = Number(range[1]);
    const end = Number(range[2]);
    if (
      !Number.isSafeInteger(start) ||
      !Number.isSafeInteger(end) ||
      start > end ||
      end >= paged.length
    ) {
      response.writeHead(416).end();
      return;
    }
    const body = paged.subarray(start, end + 1);
    requests.push({
      range: request.headers.range,
      status: 206,
      bytes: body.byteLength,
    });
    response.writeHead(206, {
      "Accept-Ranges": "bytes",
      "Content-Range": `bytes ${start}-${end}/${paged.length}`,
      "Content-Length": body.byteLength,
      "Content-Type": "application/octet-stream",
      ETag: etag,
      "Cache-Control": "public, max-age=31536000, immutable, no-transform",
      "Content-Encoding": "identity",
    });
    response.end(body);
  } catch (error) {
    response.writeHead(500).end(String(error));
  }
});

await new Promise((resolveListen, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolveListen);
});
const address = server.address();
if (!address || typeof address === "string")
  throw new Error("server has no TCP address");
const origin = `http://127.0.0.1:${address.port}`;
const browsers = {
  chromium,
  firefox,
  webkit,
};
const requestedBrowsers = (
  process.env.PANGENOME_RANGE_CATALOG_BROWSERS ?? "chromium"
)
  .split(",")
  .filter(Boolean);
const results = [];
try {
  for (const name of requestedBrowsers) {
    const browserType = browsers[name];
    if (!browserType) throw new Error(`unknown browser ${name}`);
    const browser = await browserType.launch({ headless: true });
    try {
      const page = await browser.newPage();
      await page.goto(origin, { waitUntil: "domcontentloaded" });
      const before = requests.length;
      const result = await page.evaluate(
        async ({ origin, rootBytes, fileBytes, queryIds }) => {
          const { runCatalogBenchmark } = await import("/runtime.mjs");
          return runCatalogBenchmark({
            url: `${origin}/catalog`,
            rootBytes,
            fileBytes,
            queryIds,
            maxDataRanges: 3,
            iterations: 100,
          });
        },
        {
          origin,
          rootBytes: buildStats.header_bytes + buildStats.directory_bytes,
          fileBytes: paged.length,
          queryIds,
        },
      );
      assert.deepEqual(result.records, expected);
      const observed = requests.slice(before);
      assert.equal(observed.length, result.ranges.length);
      assert.equal(
        observed.reduce((total, request) => total + request.bytes, 0),
        result.totalBytes,
      );
      results.push({ browser: name, ...result, requests: observed });
    } finally {
      await browser.close();
    }
  }
} finally {
  await new Promise((resolveClose) => server.close(resolveClose));
}

const report = {
  schema_version: 1,
  paged_catalog: resolve(pagedPath),
  paged_bytes: (await stat(pagedPath)).size,
  catalog: resolve(catalogPath),
  query_path_ids: queryIds.length,
  exact_matches: expected.length,
  browsers: results,
};
await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`, {
  flag: "wx",
});
console.log(JSON.stringify(report));
