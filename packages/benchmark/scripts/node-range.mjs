import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { FileRangeSource } from "../../browser/dist/node/index.js";
import {
  BlobRangeSource,
  HttpRangeSource,
  openPangenome,
} from "../../browser/dist/reader/index.js";
import { createRangeOrigin } from "./range-origin.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const workspace = resolve(scriptDirectory, "../../..");
const metadataPath = resolve(
  workspace,
  "test-data/conformance/micb-kir3dl1-reader-v1.json",
);
const metadata = JSON.parse(await readFile(metadataPath, "utf8"));
const archivePath = resolve(workspace, metadata.archive.path);
const archiveBytes = await readFile(archivePath);
assert.equal(archiveBytes.byteLength, metadata.archive.bytes);
assert.equal(
  createHash("sha256").update(archiveBytes).digest("hex"),
  metadata.archive.sha256,
);

function rangeHeader({ offset, length }) {
  return `bytes=${offset}-${offset + BigInt(length) - 1n}`;
}

async function querySource(source, query) {
  const archive = await openPangenome(source);
  try {
    const result = await archive.query({ ...query, trace: true });
    assert.equal(result.trace.canonicalHash, query.canonicalHash);
    assert(result.tiles.length > 0);
    assert(result.graph.nodes.ids.length > 0);
    return {
      hash: result.trace.canonicalHash,
      tiles: result.tiles.length,
      nodes: result.graph.nodes.ids.length,
      traversals: result.trace.selectedTraversals,
      trace: result.trace,
    };
  } finally {
    await archive.close();
  }
}

const rangeOrigin = await createRangeOrigin({
  archivePath,
  etag: `"${metadata.archive.sha256}"`,
});
const measurements = [];
try {
  for (const query of metadata.queries) {
    rangeOrigin.clearRequests();
    const httpSource = new HttpRangeSource(rangeOrigin.url);
    const http = await querySource(httpSource, query);
    const headRequests = rangeOrigin.requests.filter(
      ({ method }) => method === "HEAD",
    );
    const getRequests = rangeOrigin.requests.filter(
      ({ method }) => method === "GET",
    );
    assert.equal(headRequests.length, 1);
    assert.equal(headRequests[0].status, 200);
    assert(getRequests.length > 0);
    assert(getRequests.every(({ status }) => status === 206));
    assert(
      getRequests.every(
        ({ ifRange }) => ifRange === `"${metadata.archive.sha256}"`,
      ),
    );
    const ranges = getRequests.map(({ range }) => range);
    const traceRanges = http.trace.requestRanges.map(rangeHeader);
    assert.deepEqual(ranges, traceRanges);
    const fetchedBytes = getRequests.reduce(
      (total, request) => total + request.bytes,
      0,
    );
    assert.equal(fetchedBytes, http.trace.totalBytes);
    assert(fetchedBytes < archiveBytes.byteLength);
    if (query.http !== undefined) {
      assert.deepEqual(ranges, query.http.ranges);
      assert.equal(fetchedBytes, query.http.fetchedBytes);
      assert.equal(getRequests.length, query.http.requests);
    }

    const file = await querySource(
      await FileRangeSource.open(archivePath),
      query,
    );
    const blob = await querySource(
      new BlobRangeSource(new Blob([archiveBytes])),
      query,
    );
    assert.equal(file.hash, http.hash);
    assert.equal(blob.hash, http.hash);
    assert.equal(file.nodes, http.nodes);
    assert.equal(blob.nodes, http.nodes);
    measurements.push({
      id: query.id,
      canonicalHash: http.hash,
      tiles: http.tiles,
      nodes: http.nodes,
      traversals: http.traversals,
      requests: getRequests.length,
      fetchedBytes,
      ranges,
      localSourcesMatched: ["FileRangeSource", "BlobRangeSource"],
    });
  }
} finally {
  await rangeOrigin.close();
}

console.log(
  JSON.stringify(
    {
      archiveBytes: archiveBytes.byteLength,
      archiveSha256: metadata.archive.sha256,
      measurements,
    },
    null,
    2,
  ),
);
