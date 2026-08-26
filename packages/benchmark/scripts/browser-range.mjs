import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "@playwright/test";
import { createRangeOrigin } from "./range-origin.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const workspace = resolve(scriptDirectory, "../../..");
const readerBundlePath = resolve(
  workspace,
  "packages/browser/dist/reader/index.js",
);

function decodeHex(value) {
  return Buffer.from(value.trim(), "hex");
}

function u64(value) {
  const result = Buffer.alloc(8);
  result.writeBigUInt64LE(BigInt(value));
  return result;
}

function string(value) {
  const bytes = Buffer.from(value);
  return Buffer.concat([u64(bytes.length), bytes]);
}

function encodeManifest(manifest) {
  return Buffer.concat([
    string(manifest.sample),
    string(manifest.contig),
    u64(manifest.start),
    u64(manifest.end),
    u64(manifest.gridStart),
    u64(manifest.windowSize),
    u64(manifest.bucketSpan),
    u64(manifest.firstPageOffset),
    u64(manifest.pageCount),
    u64(manifest.entryCount),
    Buffer.from([3, 0, 0, 0, 0, 0, 0, 0]),
  ]);
}

function buildSyntheticArchive(compressedPayload, rawPayloadLength) {
  const pageBytes = 4096;
  const bucketSpan = 524_288;
  const dummy = {
    sample: "dummy",
    contig: "dummy",
    start: 0,
    end: bucketSpan * 16,
    gridStart: 0,
    windowSize: 16_384,
    bucketSpan,
    firstPageOffset: 0,
    pageCount: 16,
    entryCount: 0,
  };
  const target = {
    sample: "GRCh38",
    contig: "chr1",
    start: 100,
    end: 102,
    gridStart: 0,
    windowSize: 16_384,
    bucketSpan,
    firstPageOffset: 0,
    pageCount: 1,
    entryCount: 1,
  };
  const provisionalRoot = Buffer.concat([
    u64(2),
    encodeManifest(dummy),
    encodeManifest(target),
  ]);
  const rootEnd = 64 + provisionalRoot.length;
  dummy.firstPageOffset = rootEnd;
  target.firstPageOffset = rootEnd + dummy.pageCount * pageBytes;
  const root = Buffer.concat([
    u64(2),
    encodeManifest(dummy),
    encodeManifest(target),
  ]);
  assert.equal(root.length, provisionalRoot.length);
  const dataOffset = rootEnd + (dummy.pageCount + target.pageCount) * pageBytes;
  const header = Buffer.alloc(64);
  header.write("PNGRNG01", 0, "ascii");
  header.writeUInt32LE(1, 8);
  header.writeUInt32LE(64, 12);
  header.writeBigUInt64LE(64n, 16);
  header.writeBigUInt64LE(BigInt(root.length), 24);
  header.writeBigUInt64LE(1n, 32);
  header.writeBigUInt64LE(BigInt(dataOffset), 40);
  const pages = Buffer.alloc((dummy.pageCount + target.pageCount) * pageBytes);
  const targetPage = dummy.pageCount * pageBytes;
  pages.writeUInt32LE(1, targetPage);
  pages.writeUInt32LE(40, targetPage + 4);
  pages.writeBigUInt64LE(0n, targetPage + 8);
  pages.writeBigUInt64LE(100n, targetPage + 16);
  pages.writeBigUInt64LE(102n, targetPage + 24);
  pages.writeBigUInt64LE(BigInt(dataOffset), targetPage + 32);
  pages.writeBigUInt64LE(BigInt(compressedPayload.length), targetPage + 40);
  pages.writeBigUInt64LE(BigInt(rawPayloadLength), targetPage + 48);
  return Buffer.concat([header, root, pages, compressedPayload]);
}

async function createModuleOrigin() {
  const readerBundle = await readFile(readerBundlePath);
  const server = createServer((request, response) => {
    if (request.url === "/reader.js") {
      response.writeHead(200, {
        "Content-Length": String(readerBundle.length),
        "Content-Type": "text/javascript",
      });
      response.end(readerBundle);
      return;
    }
    response.writeHead(200, { "Content-Type": "text/html" });
    response.end(
      "<!doctype html><meta charset=utf-8><title>range smoke</title>",
    );
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (typeof address === "string" || address === null) {
    throw new Error("module origin did not bind an INET port");
  }
  return {
    url: `http://127.0.0.1:${address.port}`,
    close: () =>
      new Promise((resolve, reject) => {
        server.close((error) =>
          error === undefined ? resolve() : reject(error),
        );
      }),
  };
}

function parseArguments(values) {
  const result = {};
  for (let index = 0; index < values.length; index += 2) {
    const flag = values[index];
    const value = values[index + 1];
    if (!flag?.startsWith("--") || value === undefined) {
      throw new Error(`invalid argument ${flag ?? "<missing>"}`);
    }
    result[flag.slice(2)] = value;
  }
  return result;
}

async function runBrowser(browserType, name, moduleOrigin, rangeOrigin, query) {
  const browser = await browserType.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.goto(moduleOrigin.url);
    const result = await page.evaluate(
      async ({ archiveUrl, query }) => {
        const reader = await import("/reader.js");
        const started = performance.now();
        const source = new reader.HttpRangeSource(archiveUrl);
        const archive = await reader.openPangenome(source);
        const openedMs = performance.now() - started;
        const queried = await archive.query(query);
        const totalMs = performance.now() - started;
        const summary = {
          formatVersion: archive.formatVersion,
          references: archive.references().length,
          semantics: queried.semantics,
          tiles: queried.tiles.length,
          nodes: queried.tiles.reduce(
            (total, tile) => total + tile.nodeIds.length,
            0,
          ),
          traversals: queried.tiles.reduce(
            (total, tile) => total + tile.traversalWeights.length,
            0,
          ),
          encodedPayloadBytes: queried.tiles.reduce(
            (total, tile) => total + tile.encodedLength,
            0,
          ),
          sourceRequests: source.requests.map((request) => ({
            ...request,
            ...(request.offset === undefined
              ? {}
              : { offset: String(request.offset) }),
          })),
          openedMs,
          totalMs,
        };
        await archive.close();
        return summary;
      },
      { archiveUrl: rangeOrigin.url, query },
    );
    await page.close();
    return { browser: name, ...result };
  } finally {
    await browser.close();
  }
}

const args = parseArguments(process.argv.slice(2));
const isFullArchive = args.archive !== undefined;
let archiveBytes;
let archivePath;
let query;
let expectedTiles;
let engines;
if (isFullArchive) {
  archivePath = resolve(args.archive);
  query = {
    sample: args.sample,
    contig: args.contig,
    start: Number(args.start),
    end: Number(args.end),
  };
  expectedTiles = Number(args["expected-tiles"]);
  engines = [[chromium, "chromium"]];
} else {
  const compressed = decodeHex(
    await readFile(
      resolve(workspace, "test-data/golden/record-region-v1.zstd3.hex"),
      "utf8",
    ),
  );
  const raw = decodeHex(
    await readFile(
      resolve(workspace, "test-data/golden/record-region-v1.hex"),
      "utf8",
    ),
  );
  archiveBytes = buildSyntheticArchive(compressed, raw.length);
  query = { sample: "GRCh38", contig: "chr1", start: 100, end: 102 };
  expectedTiles = 1;
  engines = [
    [chromium, "chromium"],
    [firefox, "firefox"],
    [webkit, "webkit"],
  ];
}

const rangeOrigin = await createRangeOrigin({
  ...(archivePath === undefined ? { archiveBytes } : { archivePath }),
  etag: `"${args.etag ?? "synthetic-record-v1"}"`,
});
const moduleOrigin = await createModuleOrigin();
const measurements = [];
try {
  for (const [browserType, name] of engines) {
    rangeOrigin.clearRequests();
    const measurement = await runBrowser(
      browserType,
      name,
      moduleOrigin,
      rangeOrigin,
      query,
    );
    assert.equal(measurement.formatVersion, 1);
    assert.equal(
      measurement.semantics,
      "anonymous-distinct-weighted-tile-paths",
    );
    assert.equal(measurement.tiles, expectedTiles);
    assert(measurement.nodes > 0);
    const headRequests = rangeOrigin.requests.filter(
      ({ method }) => method === "HEAD",
    );
    const rangeRequests = rangeOrigin.requests.filter(
      ({ method }) => method === "GET",
    );
    assert.equal(
      headRequests.length,
      1,
      JSON.stringify(
        { originRequests: rangeOrigin.requests, measurement },
        (_key, value) => (typeof value === "bigint" ? String(value) : value),
      ),
    );
    assert.equal(headRequests[0].status, 200);
    assert(rangeRequests.length >= 3);
    assert(rangeRequests.every((request) => request.status === 206));
    assert(rangeRequests.slice(1).every((request) => request.ifRange !== null));
    const fetchedBytes = rangeRequests.reduce(
      (total, request) => total + request.bytes,
      0,
    );
    assert(fetchedBytes < rangeOrigin.size);
    measurements.push({
      ...measurement,
      headRequests: headRequests.length,
      requests: rangeRequests.length,
      fetchedBytes,
      ranges: rangeRequests.map((request) => request.range),
    });
  }
} finally {
  await moduleOrigin.close();
  await rangeOrigin.close();
}

console.log(
  JSON.stringify(
    {
      archiveBytes: rangeOrigin.size,
      ...(archiveBytes === undefined
        ? {}
        : {
            archiveSha256: createHash("sha256")
              .update(archiveBytes)
              .digest("hex"),
          }),
      query,
      measurements,
    },
    null,
    2,
  ),
);
