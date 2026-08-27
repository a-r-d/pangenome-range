import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import {
  type BrowserType,
  chromium,
  firefox,
  type Page,
  webkit,
} from "@playwright/test";
import {
  decompressionSummary,
  observedRequestRounds,
  percentile,
  requestBytes,
} from "./metrics.js";
import { createRangeOrigin, type RangeOrigin } from "./origin.js";
import {
  buildSummary,
  collectEnvironment,
  createRunFiles,
  sha256File,
  writeRun,
} from "./reporting.js";
import type {
  BenchmarkQuery,
  DecoderName,
  DecoderSummary,
  QueryMeasurement,
  SerializableRequest,
} from "./types.js";
import { loadWorkload, matchesExpectedError } from "./workload.js";

interface AssetRequest {
  readonly path: string;
  readonly bytes: number;
}

export interface ModuleOrigin {
  readonly url: string;
  readonly requests: AssetRequest[];
  clearRequests(): void;
  close(): Promise<void>;
}

interface RuntimeResult {
  readonly initializationMs: number;
  readonly openMs: number;
  readonly queryMs: number;
  readonly totalMs: number;
  readonly sourceRequests: ReadonlyArray<{
    readonly method: "HEAD" | "GET";
    readonly offset?: string;
    readonly length?: number;
    readonly status: number;
    readonly responseBytes: number;
    readonly elapsedMs: number;
  }>;
  readonly decompressionSamplesMs: readonly number[];
  readonly trace?: {
    readonly dependencyRounds: number;
    readonly requestRanges: ReadonlyArray<{
      readonly offset: string;
      readonly length: number;
      readonly layer: string;
    }>;
    readonly totalBytes: number;
    readonly integrityMs: number;
    readonly decompressionMs: number;
    readonly decompressionTaskMs: number;
    readonly cacheHits: {
      readonly bootstrap: number;
      readonly directory: number;
      readonly payload: number;
    };
    readonly decodeMs: number;
    readonly mergeMs: number;
    readonly selectedChunks: number;
    readonly selectedNodes: number;
    readonly selectedTraversals: number;
    readonly canonicalHash: string;
  };
  readonly error?: string;
  readonly peakHeapBytes?: number;
  readonly performanceMarks: Readonly<Record<string, number>>;
}

const browserTypes: Readonly<Record<string, BrowserType>> = {
  chromium,
  firefox,
  webkit,
};

function contentType(path: string): string {
  switch (extname(path)) {
    case ".js":
      return "text/javascript";
    case ".wasm":
      return "application/wasm";
    default:
      return "application/octet-stream";
  }
}

export async function createModuleOrigin(
  workspace: string,
): Promise<ModuleOrigin> {
  const readerPath = join(workspace, "packages/browser/dist/reader/index.js");
  const browserDirectory = join(workspace, "packages/benchmark/dist/browser");
  const requests: AssetRequest[] = [];
  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://module.invalid");
    if (url.pathname === "/") {
      const body = Buffer.from(`<!doctype html>
<meta charset="utf-8">
<title>pangenome-range benchmark</title>
<script type="importmap">
{"imports":{"pangenome-range/reader":"/reader.js"}}
</script>
`);
      response.writeHead(200, {
        "Cache-Control": "no-store",
        "Content-Length": body.byteLength,
        "Content-Type": "text/html",
      });
      response.end(body);
      return;
    }
    let path: string | undefined;
    if (url.pathname === "/reader.js") {
      path = readerPath;
    } else if (url.pathname.startsWith("/browser/")) {
      const candidate = resolve(
        browserDirectory,
        url.pathname.slice("/browser/".length),
      );
      if (candidate.startsWith(`${resolve(browserDirectory)}${sep}`)) {
        path = candidate;
      }
    }
    if (path === undefined) {
      response.writeHead(404);
      response.end();
      return;
    }
    try {
      const bytes = await readFile(path);
      requests.push({ path: url.pathname, bytes: bytes.byteLength });
      response.writeHead(200, {
        "Cache-Control": "public, max-age=31536000, immutable, no-transform",
        "Content-Encoding": "identity",
        "Content-Length": bytes.byteLength,
        "Content-Type": contentType(path),
      });
      response.end(bytes);
    } catch {
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise<void>((resolveListen, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolveListen);
  });
  const address = server.address();
  if (typeof address === "string" || address === null) {
    throw new Error("module origin did not bind an INET port");
  }
  return {
    url: `http://127.0.0.1:${address.port}`,
    requests,
    clearRequests(): void {
      requests.length = 0;
    },
    close: () =>
      new Promise<void>((resolveClose, reject) => {
        server.close((error) =>
          error === undefined ? resolveClose() : reject(error),
        );
      }),
  };
}

function workspaceDirectory(): string {
  return fileURLToPath(new URL("../../..", import.meta.url));
}

async function runtime(
  page: Page,
  options: {
    readonly archiveUrl: string;
    readonly query: BenchmarkQuery;
    readonly decoder: DecoderName;
    readonly scenario: string;
    readonly reuseKey?: string;
    readonly httpCache?: RequestCache;
    readonly directoryCacheBytes: number;
    readonly payloadCacheBytes: number;
  },
): Promise<RuntimeResult> {
  return page.evaluate(async (input) => {
    const runtimeUrl = "/browser/browser-runtime.js";
    const module = await import(runtimeUrl);
    return module.runBrowserScenario({
      ...input,
      wasmUrl: "/browser/zstd.wasm",
    });
  }, options) as Promise<RuntimeResult>;
}

async function closeRuntime(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const runtimeUrl = "/browser/browser-runtime.js";
    const module = await import(runtimeUrl);
    await module.closeBrowserScenarios();
  });
}

function originRequests(
  origin: RangeOrigin,
  context: {
    browser: string;
    decoder: DecoderName;
    queryId: string;
    scenario: string;
  },
): SerializableRequest[] {
  return origin.requests.map((request) => ({
    sequence: request.sequence,
    ...context,
    method: request.method,
    range: request.range,
    status: request.status,
    bytes: request.bytes,
    elapsedMs: request.elapsedMs,
    startedAt: request.startedAt,
    startedAtMs: request.startedAtMs,
    connectionId: request.connectionId,
    source: "origin",
  }));
}

type RuntimeSourceRequest = RuntimeResult["sourceRequests"][number];

function isRuntimeDataRequest(
  request: RuntimeSourceRequest,
): request is RuntimeSourceRequest & { offset: string; length: number } {
  return (
    request.method === "GET" &&
    request.offset !== undefined &&
    request.length !== undefined
  );
}

function measurement(
  query: BenchmarkQuery,
  browser: string,
  decoder: DecoderName,
  scenario: string,
  cacheMode: "cold" | "warm",
  httpCachePolicy: string,
  result: RuntimeResult,
  observed: readonly SerializableRequest[],
): QueryMeasurement {
  const trace = result.trace;
  const dataRequests = observed.filter(
    (request) => request.method === "GET" && request.range !== null,
  );
  const sourceDataRequests = result.sourceRequests.filter(isRuntimeDataRequest);
  const bytes = requestBytes(dataRequests);
  const readerObservedBytes = sourceDataRequests.reduce(
    (total, request) => total + request.responseBytes,
    0,
  );
  const sourceSignatures = sourceDataRequests
    .map(
      (request) =>
        `bytes=${request.offset}-${BigInt(request.offset) + BigInt(request.length) - 1n}:${request.status}:${request.responseBytes}`,
    )
    .sort();
  const originSignatures = dataRequests
    .map((request) => `${request.range}:${request.status}:${request.bytes}`)
    .sort();
  const sourceOriginReconciled =
    sourceSignatures.length === originSignatures.length &&
    sourceSignatures.every(
      (signature, index) => signature === originSignatures[index],
    );
  const expectedErrorMatched = matchesExpectedError(
    query.expectedError,
    result.error,
  );
  const canonicalMatched =
    result.error === undefined &&
    query.expectedCanonicalHash !== undefined &&
    trace?.canonicalHash === query.expectedCanonicalHash;
  return {
    queryId: query.id,
    queryClass: query.class,
    sample: query.sample,
    contig: query.contig,
    start: query.start,
    end: query.end,
    context: query.context,
    scenario,
    cacheMode,
    httpCachePolicy,
    decoder,
    browser,
    totalMs: result.totalMs,
    openMs: result.openMs,
    queryMs: result.queryMs,
    actualRequests: dataRequests.length,
    actualBytes: bytes.total,
    actualUniqueBytes: bytes.unique,
    actualDuplicateBytes: bytes.duplicate,
    actualRequestRounds: observedRequestRounds(dataRequests) ?? 0,
    readerObservedRequests: sourceDataRequests.length,
    readerObservedBytes,
    sourceOriginReconciled,
    plannedRequests: trace?.requestRanges.length ?? 0,
    plannedBytes: trace?.totalBytes ?? 0,
    dependencyRounds: trace?.dependencyRounds ?? 0,
    cacheHits: trace?.cacheHits ?? {
      bootstrap: 0,
      directory: 0,
      payload: 0,
    },
    decompression: decompressionSummary(result.decompressionSamplesMs),
    integrityMs: trace?.integrityMs ?? 0,
    decompressionWallMs: trace?.decompressionMs ?? 0,
    decompressionTaskMs: trace?.decompressionTaskMs ?? 0,
    decodeMs: trace?.decodeMs ?? 0,
    mergeMs: trace?.mergeMs ?? 0,
    selectedChunks: trace?.selectedChunks ?? 0,
    selectedNodes: trace?.selectedNodes ?? 0,
    selectedTraversals: trace?.selectedTraversals ?? 0,
    ...(trace?.canonicalHash === undefined
      ? {}
      : { canonicalHash: trace.canonicalHash }),
    ...(query.expectedCanonicalHash === undefined
      ? {}
      : { expectedCanonicalHash: query.expectedCanonicalHash }),
    correctness:
      (expectedErrorMatched || canonicalMatched) && sourceOriginReconciled,
    ...(result.error === undefined ? {} : { error: result.error }),
    ...(query.expectedError === undefined
      ? {}
      : { expectedError: query.expectedError }),
    ...(result.peakHeapBytes === undefined
      ? {}
      : { peakHeapBytes: result.peakHeapBytes }),
    performanceMarks: result.performanceMarks,
  };
}

function selectQueries(queries: readonly BenchmarkQuery[]): {
  anchor: BenchmarkQuery;
  nearby: BenchmarkQuery;
  distant: BenchmarkQuery;
} {
  const positive = queries.filter(
    (query) => query.expectedCanonicalHash !== undefined,
  );
  const anchor = positive[0];
  if (anchor === undefined) {
    throw new Error("browser workload requires at least one positive query");
  }
  return {
    anchor,
    nearby:
      positive.find((query) => query.class.includes("nearby")) ??
      positive[1] ??
      anchor,
    distant:
      positive.find((query) => query.class.includes("distant")) ??
      positive.find((query) => query.class.startsWith("random-")) ??
      positive.at(-1) ??
      anchor,
  };
}

export interface BrowserBenchmarkOptions {
  readonly archivePath: string;
  readonly workloadPath: string;
  readonly runId: string;
  readonly resultsDirectory: string;
  readonly browsers: readonly string[];
  readonly decoders: readonly DecoderName[];
  readonly directoryCacheBytes: number;
  readonly payloadCacheBytes: number;
}

export async function runBrowserBenchmark(
  options: BrowserBenchmarkOptions,
): Promise<{ directory: string; summary: ReturnType<typeof buildSummary> }> {
  const workspace = workspaceDirectory();
  const { workload, sha256: workloadSha256 } = await loadWorkload(
    options.workloadPath,
  );
  const archiveSha256 = await sha256File(options.archivePath);
  if (archiveSha256 !== workload.archiveSha256) {
    throw new Error(
      `archive SHA-256 ${archiveSha256} does not match workload ${workload.archiveSha256}`,
    );
  }
  const archiveMetadata = await stat(options.archivePath);
  const files = await createRunFiles(options.resultsDirectory, options.runId);
  const rangeOrigin = await createRangeOrigin({
    archives: [
      {
        route: "/archive.pngr",
        path: options.archivePath,
        etag: `"sha256-${archiveSha256}"`,
      },
    ],
  });
  const moduleOrigin = await createModuleOrigin(workspace);
  const archiveUrl = rangeOrigin.urls["/archive.pngr"] as string;
  const selected = selectQueries(workload.queries);
  const measurements: QueryMeasurement[] = [];
  const requests: SerializableRequest[] = [];
  const versions: Record<string, string> = {};
  const decoderEvidence = new Map<
    DecoderName,
    {
      initialization: number[];
      decompression: number[];
      assets: Map<string, number>;
      peakHeap: number[];
    }
  >();
  let globalSequence = 0;
  try {
    for (const browserName of options.browsers) {
      const browserType = browserTypes[browserName];
      if (browserType === undefined) {
        throw new TypeError(`unsupported browser ${browserName}`);
      }
      const browser = await browserType.launch({ headless: true });
      versions[browserName] = browser.version();
      try {
        for (const decoderName of options.decoders) {
          const evidence = decoderEvidence.get(decoderName) ?? {
            initialization: [],
            decompression: [],
            assets: new Map<string, number>(),
            peakHeap: [],
          };
          decoderEvidence.set(decoderName, evidence);

          const runCold = async (
            scenario: string,
            cache: RequestCache | undefined,
          ): Promise<void> => {
            const context = await browser.newContext();
            const page = await context.newPage();
            moduleOrigin.clearRequests();
            rangeOrigin.clearRequests();
            try {
              await page.goto(moduleOrigin.url);
              const result = await runtime(page, {
                archiveUrl,
                query: selected.anchor,
                decoder: decoderName,
                scenario,
                ...(cache === undefined ? {} : { httpCache: cache }),
                directoryCacheBytes: options.directoryCacheBytes,
                payloadCacheBytes: options.payloadCacheBytes,
              });
              const observed = originRequests(rangeOrigin, {
                browser: browserName,
                decoder: decoderName,
                queryId: selected.anchor.id,
                scenario,
              });
              measurements.push(
                measurement(
                  selected.anchor,
                  browserName,
                  decoderName,
                  scenario,
                  "cold",
                  cache ?? "default",
                  result,
                  observed,
                ),
              );
              evidence.initialization.push(result.initializationMs);
              evidence.decompression.push(...result.decompressionSamplesMs);
              if (result.peakHeapBytes !== undefined) {
                evidence.peakHeap.push(result.peakHeapBytes);
              }
              for (const asset of moduleOrigin.requests) {
                evidence.assets.set(asset.path, asset.bytes);
              }
              requests.push(
                ...observed.map((request) => ({
                  ...request,
                  sequence: globalSequence++,
                })),
              );
            } finally {
              await context.close();
            }
          };
          await runCold("cold-library-transport-no-store", "no-store");
          await runCold("cold-library-normal-http-cache", undefined);

          const context = await browser.newContext();
          const page = await context.newPage();
          try {
            await page.goto(moduleOrigin.url);
            const runWarm = async (parameters: {
              scenario: string;
              query: BenchmarkQuery;
              reuseKey: string;
              directoryCacheBytes: number;
              payloadCacheBytes: number;
              prime?: BenchmarkQuery;
            }): Promise<void> => {
              if (parameters.prime !== undefined) {
                await runtime(page, {
                  archiveUrl,
                  query: parameters.prime,
                  decoder: decoderName,
                  scenario: `${parameters.scenario}-prime`,
                  reuseKey: parameters.reuseKey,
                  httpCache: "no-store",
                  directoryCacheBytes: parameters.directoryCacheBytes,
                  payloadCacheBytes: parameters.payloadCacheBytes,
                });
              }
              rangeOrigin.clearRequests();
              const result = await runtime(page, {
                archiveUrl,
                query: parameters.query,
                decoder: decoderName,
                scenario: parameters.scenario,
                reuseKey: parameters.reuseKey,
                httpCache: "no-store",
                directoryCacheBytes: parameters.directoryCacheBytes,
                payloadCacheBytes: parameters.payloadCacheBytes,
              });
              const observed = originRequests(rangeOrigin, {
                browser: browserName,
                decoder: decoderName,
                queryId: parameters.query.id,
                scenario: parameters.scenario,
              });
              measurements.push(
                measurement(
                  parameters.query,
                  browserName,
                  decoderName,
                  parameters.scenario,
                  "warm",
                  "no-store",
                  result,
                  observed,
                ),
              );
              evidence.decompression.push(...result.decompressionSamplesMs);
              if (result.peakHeapBytes !== undefined) {
                evidence.peakHeap.push(result.peakHeapBytes);
              }
              requests.push(
                ...observed.map((request) => ({
                  ...request,
                  sequence: globalSequence++,
                })),
              );
            };
            const prefix = `${browserName}-${decoderName}`;
            await runWarm({
              scenario: "warm-directory-cache",
              query: selected.anchor,
              reuseKey: `${prefix}-directory`,
              directoryCacheBytes: options.directoryCacheBytes,
              payloadCacheBytes: 0,
              prime: selected.anchor,
            });
            await runWarm({
              scenario: "repeated-same-query",
              query: selected.anchor,
              reuseKey: `${prefix}-repeat`,
              directoryCacheBytes: options.directoryCacheBytes,
              payloadCacheBytes: options.payloadCacheBytes,
              prime: selected.anchor,
            });
            await runWarm({
              scenario: "nearby-pan-query",
              query: selected.nearby,
              reuseKey: `${prefix}-pan`,
              directoryCacheBytes: options.directoryCacheBytes,
              payloadCacheBytes: options.payloadCacheBytes,
              prime: selected.anchor,
            });
            await runWarm({
              scenario: "distant-random-query",
              query: selected.distant,
              reuseKey: `${prefix}-pan`,
              directoryCacheBytes: options.directoryCacheBytes,
              payloadCacheBytes: options.payloadCacheBytes,
            });
            await closeRuntime(page);
          } finally {
            await context.close();
          }
        }
      } finally {
        await browser.close();
      }
    }
  } finally {
    await moduleOrigin.close();
    await rangeOrigin.close();
  }
  const decoderSummaries: DecoderSummary[] = [...decoderEvidence].map(
    ([name, evidence]) => {
      const javascriptBytes = [...evidence.assets]
        .filter(([path]) => !path.endsWith(".wasm"))
        .reduce((total, [, bytes]) => total + bytes, 0);
      const wasmBytes = [...evidence.assets]
        .filter(([path]) => path.endsWith(".wasm"))
        .reduce((total, [, bytes]) => total + bytes, 0);
      return {
        name,
        initializationMs: percentile(evidence.initialization, 0.5) ?? 0,
        javascriptBytes,
        wasmBytes,
        decompression: decompressionSummary(evidence.decompression),
        peakHeapBytes:
          evidence.peakHeap.length === 0
            ? null
            : Math.max(...evidence.peakHeap),
        limitation:
          browserNameForMemory(options.browsers) +
          "; asset bytes are unique module-origin response bodies for this decoder",
      };
    },
  );
  const summary = buildSummary({
    runId: options.runId,
    kind: "browser",
    archive: {
      location: options.archivePath,
      kind: "http",
      size: String(archiveMetadata.size),
      sha256: archiveSha256,
      etag: `"sha256-${archiveSha256}"`,
    },
    workloadSha256,
    measurements,
    requests,
    decoderSummaries,
    limitations: [
      "All retained browser timings use a loopback origin and are functional/local evidence, not public-network or CDN performance.",
      "Cold HTTP-cache scenarios use a fresh ephemeral Playwright browser context. This establishes an empty context cache, but does not claim control over operating-system caches.",
      "Warm library-cache scenarios force transport no-store so directory and payload cache effects are not confused with the browser HTTP cache.",
      "Actual request counts/bytes/rounds come from range-origin logs; reader-observed fetches are retained and must reconcile with the origin. Planned counts/bytes and phase timings come from the reader query trace and Performance API.",
      "Peak JavaScript heap is available only where the browser exposes performance.memory and excludes native/WASM memory.",
    ],
  });
  await writeRun({
    files,
    config: {
      schemaVersion: 1,
      command: "browser",
      archivePath: options.archivePath,
      workloadPath: options.workloadPath,
      workload,
      browsers: options.browsers,
      decoders: options.decoders,
      scenarios: [
        "cold-library-transport-no-store",
        "cold-library-normal-http-cache",
        "warm-directory-cache",
        "repeated-same-query",
        "nearby-pan-query",
        "distant-random-query",
      ],
      libraryCache: {
        directoryBytes: options.directoryCacheBytes,
        payloadBytes: options.payloadCacheBytes,
      },
      origin: {
        cors: "*",
        exposedHeaders: true,
        cacheControl: "public, max-age=31536000, immutable, no-transform",
        contentEncoding: "identity",
      },
    },
    environment: await collectEnvironment(versions),
    summary,
    requests,
  });
  if (summary.totals.failed > 0) {
    throw new Error(
      `${summary.totals.failed} of ${summary.totals.queries} browser measurements failed correctness`,
    );
  }
  return { directory: files.directory, summary };
}

function browserNameForMemory(browsers: readonly string[]): string {
  return browsers.includes("chromium")
    ? "performance.memory may be available in Chromium only"
    : "performance.memory was unavailable in the selected browsers";
}
