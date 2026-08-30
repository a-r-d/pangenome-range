import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { gzipSync } from "node:zlib";

const entries = {
  reader: new URL("../dist/reader/index.js", import.meta.url),
  node: new URL("../dist/node/index.js", import.meta.url),
  viewer: new URL("../dist/viewer/index.js", import.meta.url),
};
// Named-path membership adds strict catalog, directory, and multiplicity codecs.
// Keep the network-sensitive gzip ceiling unchanged while allowing their readable
// unminified ESM in the single-file reader entry.
const readerRawBudget = 208 * 1024;
const readerGzipBudget = 50 * 1024;
const measurements = {};
for (const [name, url] of Object.entries(entries)) {
  const bytes = await readFile(url);
  const source = bytes.toString("utf8");
  if (name === "reader" || name === "viewer") {
    for (const forbidden of [
      "node:child_process",
      "@pangenome-range/cli-",
      "pangenome-range.mjs",
    ]) {
      assert.equal(
        source.includes(forbidden),
        false,
        `${name} bundle contains native launcher marker ${forbidden}`,
      );
    }
  }
  measurements[name] = {
    rawBytes: bytes.byteLength,
    gzipBytes: gzipSync(bytes, { level: 9 }).byteLength,
  };
}

assert(
  measurements.reader.rawBytes <= readerRawBudget,
  `reader bundle is ${measurements.reader.rawBytes} bytes; budget is ${readerRawBudget}`,
);
assert(
  measurements.reader.gzipBytes <= readerGzipBudget,
  `reader gzip is ${measurements.reader.gzipBytes} bytes; budget is ${readerGzipBudget}`,
);
assert(
  measurements.node.rawBytes <= 8 * 1024,
  `node entry is ${measurements.node.rawBytes} bytes; budget is 8192`,
);
assert(
  measurements.viewer.rawBytes <= 64 * 1024,
  `viewer bundle is ${measurements.viewer.rawBytes} bytes; budget is 65536`,
);
assert(
  measurements.viewer.gzipBytes <= 16 * 1024,
  `viewer gzip is ${measurements.viewer.gzipBytes} bytes; budget is 16384`,
);

console.log(
  JSON.stringify(
    { budgetsPassed: true, nativeLauncherIsolated: true, measurements },
    null,
    2,
  ),
);
