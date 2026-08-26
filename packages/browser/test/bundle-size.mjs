import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { gzipSync } from "node:zlib";

const entries = {
  reader: new URL("../dist/reader/index.js", import.meta.url),
  node: new URL("../dist/node/index.js", import.meta.url),
  viewer: new URL("../dist/viewer/index.js", import.meta.url),
};
const measurements = {};
for (const [name, url] of Object.entries(entries)) {
  const bytes = await readFile(url);
  measurements[name] = {
    rawBytes: bytes.byteLength,
    gzipBytes: gzipSync(bytes, { level: 9 }).byteLength,
  };
}

assert(
  measurements.reader.rawBytes <= 160 * 1024,
  `reader bundle is ${measurements.reader.rawBytes} bytes; budget is 163840`,
);
assert(
  measurements.reader.gzipBytes <= 50 * 1024,
  `reader gzip is ${measurements.reader.gzipBytes} bytes; budget is 51200`,
);
assert(
  measurements.node.rawBytes <= 8 * 1024,
  `node entry is ${measurements.node.rawBytes} bytes; budget is 8192`,
);

console.log(JSON.stringify({ budgetsPassed: true, measurements }, null, 2));
