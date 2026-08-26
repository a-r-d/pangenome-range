import assert from "node:assert/strict";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { chromium } from "@playwright/test";
import {
  createModuleOrigin,
  createRangeOrigin,
  runBrowserBenchmark,
} from "../dist/index.js";

const workspace = resolve(import.meta.dirname, "../../..");
const archivePath = resolve(
  workspace,
  "test-data/conformance/micb-kir3dl1-reader-v1.pngr",
);
const workloadPath = resolve(
  workspace,
  "packages/benchmark/workloads/micb-kir3dl1-v1.json",
);
const selectedBrowsers = (
  process.env.PANGENOME_RANGE_BROWSERS ?? "chromium,firefox,webkit"
)
  .split(",")
  .filter(Boolean);
const selectedDecoders = (
  process.env.PANGENOME_RANGE_DECODERS ?? "pure-js,wasm"
)
  .split(",")
  .filter(Boolean);
const resultsDirectory = await mkdtemp(join(tmpdir(), "pngr-browser-bench-"));
const { directory, summary } = await runBrowserBenchmark({
  archivePath,
  workloadPath,
  runId: "browser-smoke",
  resultsDirectory,
  browsers: selectedBrowsers,
  decoders: selectedDecoders,
  directoryCacheBytes: 1_048_576,
  payloadCacheBytes: 33_554_432,
});
assert.equal(summary.totals.failed, 0);
assert.equal(
  summary.measurements.length,
  selectedBrowsers.length * selectedDecoders.length * 6,
);
assert.deepEqual(
  [...new Set(summary.measurements.map(({ browser }) => browser))],
  selectedBrowsers,
);
assert.deepEqual(
  [...new Set(summary.measurements.map(({ decoder }) => decoder))],
  selectedDecoders,
);
assert.equal(
  new Set(summary.measurements.map(({ scenario }) => scenario)).size,
  6,
);
assert(
  summary.measurements.every(
    (measurement) =>
      measurement.performanceMarks?.queryMs === measurement.queryMs &&
      measurement.performanceMarks?.totalMs === measurement.totalMs &&
      measurement.sourceOriginReconciled === true &&
      Number.isSafeInteger(measurement.actualRequestRounds),
  ),
);
const rawRequests = (await readFile(join(directory, "requests.ndjson"), "utf8"))
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line));
assert(
  rawRequests.every(
    (request) =>
      typeof request.startedAt === "string" &&
      typeof request.startedAtMs === "number",
  ),
);

const query = {
  id: "fault-query",
  class: "fault",
  sample: "GRCh38",
  contig: "chr6",
  start: 31_498_145,
  end: 31_511_124,
  context: 100,
};
if (selectedBrowsers.includes("chromium")) {
  const moduleOrigin = await createModuleOrigin(workspace);
  const browser = await chromium.launch({ headless: true });
  try {
    for (const [faults, expected] of [
      [{ missingExposedHeaders: true }, "exposed Accept-Ranges"],
      [{ missingCors: true }, "Failed to fetch"],
    ]) {
      const rangeOrigin = await createRangeOrigin({
        archives: [
          {
            route: "/archive.pngr",
            path: archivePath,
            etag: '"browser-fault"',
          },
        ],
        faults,
      });
      const context = await browser.newContext();
      try {
        const page = await context.newPage();
        await page.goto(moduleOrigin.url);
        let error;
        try {
          error = await page.evaluate(
            async ({ archiveUrl, query }) => {
              const runtimeUrl = "/browser/browser-runtime.js";
              const runtime = await import(runtimeUrl);
              const result = await runtime.runBrowserScenario({
                archiveUrl,
                query,
                decoder: "pure-js",
                scenario: "fault",
                httpCache: "no-store",
                directoryCacheBytes: 1024 * 1024,
                payloadCacheBytes: 32 * 1024 * 1024,
                wasmUrl: "/browser/zstd.wasm",
              });
              return result.error;
            },
            { archiveUrl: rangeOrigin.urls["/archive.pngr"], query },
          );
        } catch (caught) {
          error = String(caught);
        }
        assert.match(error, new RegExp(expected));
      } finally {
        await context.close();
        await rangeOrigin.close();
      }
    }
  } finally {
    await browser.close();
    await moduleOrigin.close();
  }
}

console.log(
  JSON.stringify(
    {
      runId: summary.runId,
      browsers: selectedBrowsers,
      decoders: selectedDecoders,
      scenarios: 6,
      measurements: summary.measurements.length,
      failures: summary.totals.failed,
      timingQualification: "loopback functional/local evidence only",
    },
    null,
    2,
  ),
);
