import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { createServer } from "node:http";

function parseRange(value, size) {
  const match = /^bytes=(\d+)-(\d+)$/.exec(value ?? "");
  if (match === null) return undefined;
  const start = Number(match[1]);
  const end = Number(match[2]);
  if (
    !Number.isSafeInteger(start) ||
    !Number.isSafeInteger(end) ||
    start < 0 ||
    start > end ||
    end >= size
  ) {
    return undefined;
  }
  return { start, end };
}

export async function createRangeOrigin({
  archivePath,
  archiveBytes,
  etag = '"pangenome-range-test-object"',
}) {
  if ((archivePath === undefined) === (archiveBytes === undefined)) {
    throw new TypeError("provide exactly one of archivePath or archiveBytes");
  }
  const bytes =
    archiveBytes === undefined ? undefined : Buffer.from(archiveBytes);
  const size = bytes?.byteLength ?? Number((await stat(archivePath)).size);
  if (!Number.isSafeInteger(size)) {
    throw new RangeError("test origin requires a safely addressable file size");
  }
  const requests = [];
  const server = createServer(async (request, response) => {
    const started = performance.now();
    const common = {
      "Accept-Ranges": "bytes",
      "Access-Control-Allow-Headers": "Range, If-Range",
      "Access-Control-Allow-Methods": "GET, HEAD, OPTIONS",
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Expose-Headers":
        "Accept-Ranges, Content-Range, Content-Length, ETag",
      "Cache-Control": "public, max-age=31536000, immutable, no-transform",
      "Content-Type": "application/octet-stream",
      ETag: etag,
    };
    if (request.method === "OPTIONS") {
      response.writeHead(204, {
        ...common,
      });
      response.end();
      return;
    }
    if (request.url !== "/archive.pngr") {
      response.writeHead(404);
      response.end();
      return;
    }
    if (request.method === "HEAD") {
      response.writeHead(200, { ...common, "Content-Length": String(size) });
      response.end();
      requests.push({
        method: "HEAD",
        range: null,
        ifRange: request.headers["if-range"] ?? null,
        status: 200,
        bytes: 0,
        elapsedMs: performance.now() - started,
      });
      return;
    }
    if (request.method !== "GET") {
      response.writeHead(405, common);
      response.end();
      return;
    }
    const rangeValue = request.headers.range;
    const range = parseRange(rangeValue, size);
    if (range === undefined) {
      response.writeHead(416, {
        ...common,
        "Content-Range": `bytes */${size}`,
      });
      response.end();
      requests.push({
        method: "GET",
        range: rangeValue ?? null,
        ifRange: request.headers["if-range"] ?? null,
        status: 416,
        bytes: 0,
        elapsedMs: performance.now() - started,
      });
      return;
    }
    const length = range.end - range.start + 1;
    response.writeHead(206, {
      ...common,
      "Content-Length": String(length),
      "Content-Range": `bytes ${range.start}-${range.end}/${size}`,
    });
    if (bytes !== undefined) {
      response.end(bytes.subarray(range.start, range.end + 1));
    } else {
      const stream = createReadStream(archivePath, {
        start: range.start,
        end: range.end,
      });
      stream.on("error", (error) => response.destroy(error));
      stream.pipe(response);
    }
    response.on("finish", () => {
      requests.push({
        method: "GET",
        range: rangeValue,
        ifRange: request.headers["if-range"] ?? null,
        status: 206,
        bytes: length,
        elapsedMs: performance.now() - started,
      });
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (typeof address === "string" || address === null) {
    throw new Error("range origin did not bind an INET port");
  }
  return {
    url: `http://127.0.0.1:${address.port}/archive.pngr`,
    size,
    requests,
    clearRequests() {
      requests.length = 0;
    },
    close: () =>
      new Promise((resolve, reject) => {
        server.close((error) =>
          error === undefined ? resolve() : reject(error),
        );
      }),
  };
}
