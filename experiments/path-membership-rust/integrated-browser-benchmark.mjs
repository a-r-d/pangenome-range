import assert from "node:assert/strict";
import { readFile, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import {
  chromium,
  firefox,
  webkit,
} from "../../packages/benchmark/node_modules/@playwright/test/index.mjs";

const [
  archivePath,
  sample,
  contig,
  startText,
  endText,
  catalogPath,
  outputPath,
] = process.argv.slice(2);
if (!outputPath)
  throw new Error(
    "usage: node integrated-browser-benchmark.mjs ARCHIVE SAMPLE CONTIG START END CATALOG OUTPUT",
  );
const archive = await readFile(archivePath);
const query = {
  sample,
  contig,
  start: Number(startText),
  end: Number(endText),
};
const catalog = new Map(
  (await readFile(catalogPath, "utf8"))
    .trim()
    .split("\n")
    .map(JSON.parse)
    .filter((row) => row.type === "path")
    .map((row) => [row.canonical_path_id, row]),
);
const runtimePath = resolve(
  import.meta.dirname,
  "integrated-browser-runtime.mjs",
);
const workspace = resolve(import.meta.dirname, "../..");
const nobleRoot = resolve(
  workspace,
  "packages/browser/node_modules/@noble/hashes",
);
const fzstdPath = resolve(
  workspace,
  "packages/browser/node_modules/fzstd/esm/index.mjs",
);
const requests = [];
const etag = '"integrated-path-membership"';

function moduleResponse(response, bytes) {
  response.writeHead(200, {
    "Content-Type": "text/javascript",
    "Content-Length": bytes.length,
  });
  response.end(bytes);
}

const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, "http://127.0.0.1");
    if (url.pathname === "/")
      return moduleResponse(
        response,
        Buffer.from("<!doctype html><title>membership</title>"),
      );
    if (url.pathname === "/runtime.mjs")
      return moduleResponse(response, await readFile(runtimePath));
    if (url.pathname === "/fzstd.mjs")
      return moduleResponse(response, await readFile(fzstdPath));
    if (url.pathname.startsWith("/noble/") && url.pathname.endsWith(".js")) {
      const name = url.pathname.slice(7);
      if (name.includes("/") || name.includes(".."))
        return response.writeHead(404).end();
      return moduleResponse(response, await readFile(resolve(nobleRoot, name)));
    }
    if (url.pathname !== "/archive") return response.writeHead(404).end();
    const match = request.headers.range?.match(/^bytes=(\d+)-(\d+)$/);
    if (!match) return response.writeHead(416).end();
    const start = Number(match[1]);
    const end = Number(match[2]);
    if (
      !Number.isSafeInteger(start) ||
      !Number.isSafeInteger(end) ||
      start > end ||
      end >= archive.length
    )
      return response.writeHead(416).end();
    const body = archive.subarray(start, end + 1);
    requests.push({
      range: request.headers.range,
      bytes: body.length,
      status: 206,
    });
    response.writeHead(206, {
      "Accept-Ranges": "bytes",
      "Content-Range": `bytes ${start}-${end}/${archive.length}`,
      "Content-Length": body.length,
      "Content-Type": "application/octet-stream",
      "Content-Encoding": "identity",
      "Cache-Control": "public, max-age=31536000, immutable, no-transform",
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Expose-Headers":
        "Accept-Ranges, Content-Range, Content-Length, ETag",
      ETag: etag,
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
  throw new Error("server address unavailable");
const origin = `http://127.0.0.1:${address.port}`;
const browserTypes = { chromium, firefox, webkit };
const names = (process.env.PANGENOME_RANGE_MEMBERSHIP_BROWSERS ?? "chromium")
  .split(",")
  .filter(Boolean);
const results = [];
try {
  for (const name of names) {
    const browser = await browserTypes[name].launch({ headless: true });
    try {
      const page = await browser.newPage();
      await page.goto(origin, { waitUntil: "domcontentloaded" });
      const before = requests.length;
      const result = await page.evaluate(
        async (config) => {
          const { runIntegratedMembership } = await import("/runtime.mjs");
          return runIntegratedMembership(config);
        },
        {
          url: `${origin}/archive`,
          bootstrapBytes: 16384,
          fileBytes: archive.length,
          query,
        },
      );
      const expected = result.records.map((record) => {
        const row = catalog.get(record.canonical_path_id);
        return {
          canonical_path_id: row.canonical_path_id,
          raw_name: row.raw_name,
          sample: row.sample,
          contig: row.contig,
          haplotype: row.haplotype,
          fragment: row.fragment,
          path_sense: row.path_sense,
        };
      });
      assert.deepEqual(result.records, expected);
      const observed = requests.slice(before);
      assert.equal(
        result.totalBytes,
        observed.reduce((sum, request) => sum + request.bytes, 0),
      );
      results.push({
        browser: name,
        ...result,
        records: undefined,
        requests: observed,
      });
    } finally {
      await browser.close();
    }
  }
} finally {
  await new Promise((resolveClose) => server.close(resolveClose));
}

const report = {
  schema_version: 1,
  archive: resolve(archivePath),
  archive_bytes: (await stat(archivePath)).size,
  query,
  browsers: results,
};
await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`, {
  flag: "wx",
});
console.log(JSON.stringify(report));
