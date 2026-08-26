import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  HttpRangeResponseError,
  HttpRangeSource,
  RemoteObjectChangedError,
} from "../../browser/dist/reader/index.js";
import {
  checkOrigin,
  createRangeOrigin,
  parseWorkload,
  runArchiveBenchmark,
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

test("validates the shared workload schema", async () => {
  const workload = parseWorkload(
    JSON.parse(await readFile(workloadPath, "utf8")),
  );
  assert.equal(workload.schemaVersion, 1);
  assert(workload.queries.some((query) => query.class === "random-1000"));
  assert(workload.queries.some((query) => query.class === "boundary-start"));
  assert(workload.queries.some((query) => query.class === "absent-region"));
  assert.throws(
    () => parseWorkload({ ...workload, schemaVersion: 2 }),
    /unsupported workload schemaVersion/,
  );
  assert.throws(
    () =>
      parseWorkload({
        ...workload,
        queries: [{ ...workload.queries[0], expectedCanonicalHash: "bad" }],
      }),
    /64 lowercase hex/,
  );
});

test("serves strict ranges, logs connections, and validates origin metadata", async () => {
  const temporary = await mkdtemp(join(tmpdir(), "pngr-origin-"));
  const localPath = join(temporary, "archive.pngr");
  const bytes = Buffer.from(
    Array.from({ length: 256 }, (_value, index) => index),
  );
  await writeFile(localPath, bytes);
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  const origin = await createRangeOrigin({
    archives: [
      {
        route: "/archive.pngr",
        path: localPath,
        etag: `"sha256-${sha256}"`,
      },
    ],
  });
  try {
    const url = origin.urls["/archive.pngr"];
    const source = new HttpRangeSource(url);
    assert.equal(await source.size(), 256n);
    assert.deepEqual(
      [...(await source.read(8n, 16))],
      [...bytes.subarray(8, 24)],
    );
    assert.equal(origin.requests.length, 2);
    assert(origin.requests.every((request) => request.connectionId.length > 0));
    assert.equal(origin.requests[1].range, "bytes=8-23");
    const report = await checkOrigin({
      url,
      localFile: localPath,
      expectedSha256: sha256,
    });
    assert.equal(report.passed, true, JSON.stringify(report, null, 2));
    assert(report.ranges.every((range) => range.matchedLocal === true));
  } finally {
    await origin.close();
  }
});

test("origin validation checks the configured deployment origin", async () => {
  const origin = await createRangeOrigin({
    archives: [
      { route: "/archive.pngr", bytes: Buffer.alloc(64), etag: '"cors"' },
    ],
    corsOrigin: "https://pages.example",
  });
  try {
    const url = origin.urls["/archive.pngr"];
    assert.equal(
      (await checkOrigin({ url, requestOrigin: "https://pages.example" }))
        .passed,
      true,
    );
    const mismatch = await checkOrigin({
      url,
      requestOrigin: "https://wrong.example",
    });
    assert.equal(
      mismatch.checks.find((check) => check.name === "CORS")?.passed,
      false,
    );
  } finally {
    await origin.close();
  }
});

async function withFault(faults, callback) {
  const bytes = Buffer.alloc(1_024, 7);
  const origin = await createRangeOrigin({
    archives: [
      {
        route: "/archive.pngr",
        bytes,
        etag: '"fault-fixture"',
      },
    ],
    faults,
  });
  try {
    await callback(origin.urls["/archive.pngr"], origin);
  } finally {
    await origin.close();
  }
}

test("fault modes trigger strict reader failures and controlled timing", async () => {
  await withFault({ ignoreRange: true }, async (url) => {
    const source = new HttpRangeSource(url);
    await source.size();
    await assert.rejects(
      source.read(0n, 16),
      (error) =>
        error instanceof HttpRangeResponseError &&
        /ignored Range/.test(error.message),
    );
  });
  await withFault({ malformedContentRange: true }, async (url) => {
    const source = new HttpRangeSource(url);
    await source.size();
    await assert.rejects(source.read(0n, 16), /Content-Range/);
  });
  await withFault({ truncateBytes: 1 }, async (url) => {
    const source = new HttpRangeSource(url);
    await source.size();
    await assert.rejects(source.read(0n, 16));
  });
  await withFault({ etagChange: true }, async (url) => {
    const source = new HttpRangeSource(url);
    await source.size();
    await assert.rejects(
      source.read(0n, 16),
      (error) => error instanceof RemoteObjectChangedError,
    );
  });
  await withFault(
    { latencyMs: 20, bandwidthBytesPerSecond: 640 },
    async (url, origin) => {
      const source = new HttpRangeSource(url, { useHead: false });
      const started = performance.now();
      await source.read(0n, 64);
      const elapsed = performance.now() - started;
      assert(
        elapsed >= 80,
        `expected throttled read >=80 ms, received ${elapsed}`,
      );
      assert(origin.requests[0].elapsedMs >= 80);
    },
  );
  await withFault({ missingCors: true }, async (url) => {
    const result = await checkOrigin({ url });
    assert.equal(result.passed, false);
    assert.equal(
      result.checks.find((check) => check.name === "CORS")?.passed,
      false,
    );
  });
  await withFault({ missingExposedHeaders: true }, async (url) => {
    const result = await checkOrigin({ url });
    assert.equal(result.passed, false);
    assert.equal(
      result.checks.find((check) => check.name === "exposed range headers")
        ?.passed,
      false,
    );
  });
});

test("writes complete Node cold/warm JS/WASM evidence and refuses overwrite", async () => {
  const temporary = await mkdtemp(join(tmpdir(), "pngr-node-bench-"));
  const options = {
    file: archivePath,
    workloadPath,
    runId: "node-smoke",
    resultsDirectory: temporary,
    modes: ["cold", "warm"],
    decoders: ["pure-js", "wasm"],
    directoryCacheBytes: 1_048_576,
    payloadCacheBytes: 33_554_432,
  };
  const { directory, summary } = await runArchiveBenchmark(options);
  assert.equal(summary.totals.failed, 0);
  assert.equal(summary.measurements.length, 40);
  assert.deepEqual(
    summary.decoderSummaries.map(({ name }) => name),
    ["pure-js", "wasm"],
  );
  assert(
    summary.measurements.every(
      (measurement) => measurement.sourceOriginReconciled === true,
    ),
  );
  for (const file of [
    "config.json",
    "environment.json",
    "requests.ndjson",
    "queries.csv",
    "summary.json",
    "REPORT.md",
  ]) {
    assert((await readFile(join(directory, file))).byteLength > 0);
  }
  await assert.rejects(runArchiveBenchmark(options), /EEXIST/);
});
