import { readdir, stat } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { FileRangeSource } from "pangenome-range/node";
import {
  type HttpRangeRequest,
  HttpRangeSource,
  openPangenome,
  type PangenomeArchive,
  type QueryTrace,
  type RangeSource,
  TracingRangeSource,
} from "pangenome-range/reader";
import { initializeDecoder } from "./decoders.js";
import { decompressionSummary, requestBytes } from "./metrics.js";
import {
  buildSummary,
  collectEnvironment,
  createRunFiles,
  sha256File,
  writeRun,
} from "./reporting.js";
import type {
  ArchiveIdentity,
  BenchmarkQuery,
  CacheMode,
  DecoderName,
  DecoderSummary,
  QueryMeasurement,
  SerializableRequest,
} from "./types.js";
import { loadWorkload, matchesExpectedError } from "./workload.js";

interface SourceObserver {
  readonly source: RangeSource;
  snapshot(): number;
  requestsSince(
    index: number,
    context: {
      queryId: string;
      scenario: string;
      decoder: DecoderName;
    },
  ): SerializableRequest[];
}

function serializeHttpRequest(
  request: HttpRangeRequest,
  sequence: number,
  context: { queryId: string; scenario: string; decoder: DecoderName },
): SerializableRequest {
  const range =
    request.offset === undefined || request.length === undefined
      ? null
      : `bytes=${request.offset}-${request.offset + BigInt(request.length) - 1n}`;
  return {
    sequence,
    ...context,
    method: request.method,
    range,
    ...(request.offset === undefined
      ? {}
      : { offset: request.offset.toString() }),
    ...(request.length === undefined ? {} : { length: request.length }),
    status: request.status,
    bytes: request.responseBytes,
    elapsedMs: request.elapsedMs,
    source: "reader",
  };
}

function httpObserver(url: string, cache?: RequestCache): SourceObserver {
  const source = new HttpRangeSource(url, {
    ...(cache === undefined ? {} : { cache }),
  });
  return {
    source,
    snapshot: () => source.requests.length,
    requestsSince: (index, context) =>
      source.requests
        .slice(index)
        .map((request, sequence) =>
          serializeHttpRequest(request, sequence, context),
        ),
  };
}

async function fileObserver(path: string): Promise<SourceObserver> {
  const source = new TracingRangeSource(await FileRangeSource.open(path), {
    layer: "file",
  });
  return {
    source,
    snapshot: () => source.reads.length,
    requestsSince: (index, context) =>
      source.reads.slice(index).map((request, sequence) => ({
        sequence,
        ...context,
        method: "READ",
        range: `bytes=${request.offset}-${request.offset + BigInt(request.length) - 1n}`,
        offset: request.offset.toString(),
        length: request.length,
        bytes: request.bytesReturned,
        elapsedMs: request.elapsedMs,
        source: "reader",
      })),
  };
}

function errorText(error: unknown): string {
  return error instanceof Error
    ? `${error.name}: ${error.message}`
    : String(error);
}

function emptyTrace(): QueryTrace {
  return {
    dependencyRounds: 0,
    requestRanges: [],
    totalBytes: 0,
    uniqueBytes: 0,
    duplicateBytes: 0,
    bootstrapBytes: 0,
    directoryBytes: 0,
    payloadBytes: 0,
    cacheHits: { bootstrap: 0, directory: 0, payload: 0 },
    integrityMs: 0,
    decompressionMs: 0,
    decompressionTaskMs: 0,
    decodeMs: 0,
    mergeMs: 0,
    selectedChunks: 0,
    selectedNodes: 0,
    selectedTraversals: 0,
    canonicalHash: "",
  };
}

async function runQuery(options: {
  readonly archive: PangenomeArchive;
  readonly observer: SourceObserver;
  readonly query: BenchmarkQuery;
  readonly decoder: DecoderName;
  readonly scenario: string;
  readonly cacheMode: CacheMode;
  readonly httpCachePolicy: string;
  readonly openMs: number;
  readonly decompressorSamples: {
    clear(): void;
    readonly samplesMs: readonly number[];
  };
}): Promise<{
  measurement: QueryMeasurement;
  requests: SerializableRequest[];
}> {
  options.decompressorSamples.clear();
  const requestStart = options.observer.snapshot();
  const heapBefore = process.memoryUsage().heapUsed;
  const started = performance.now();
  let trace = emptyTrace();
  let failure: string | undefined;
  try {
    const result = await options.archive.query({
      sample: options.query.sample,
      contig: options.query.contig,
      start: options.query.start,
      end: options.query.end,
      context: options.query.context,
      trace: true,
    });
    trace = result.trace as QueryTrace;
  } catch (error) {
    failure = errorText(error);
  }
  const queryMs = performance.now() - started;
  const requests = options.observer.requestsSince(requestStart, {
    queryId: options.query.id,
    scenario: options.scenario,
    decoder: options.decoder,
  });
  const dataRequests = requests.filter(
    (request) => request.method !== "HEAD" && request.length !== 0,
  );
  const actual = requestBytes(dataRequests);
  const expectedErrorMatched = matchesExpectedError(
    options.query.expectedError,
    failure,
  );
  const canonicalMatched =
    failure === undefined &&
    options.query.expectedCanonicalHash !== undefined &&
    trace.canonicalHash === options.query.expectedCanonicalHash;
  const heapAfter = process.memoryUsage().heapUsed;
  return {
    requests,
    measurement: {
      queryId: options.query.id,
      queryClass: options.query.class,
      sample: options.query.sample,
      contig: options.query.contig,
      start: options.query.start,
      end: options.query.end,
      context: options.query.context,
      scenario: options.scenario,
      cacheMode: options.cacheMode,
      httpCachePolicy: options.httpCachePolicy,
      decoder: options.decoder,
      totalMs: options.openMs + queryMs,
      openMs: options.openMs,
      queryMs,
      actualRequests: dataRequests.length,
      actualBytes: actual.total,
      actualUniqueBytes: actual.unique,
      actualDuplicateBytes: actual.duplicate,
      readerObservedRequests: dataRequests.length,
      readerObservedBytes: actual.total,
      sourceOriginReconciled: true,
      plannedRequests: trace.requestRanges.length,
      plannedBytes: trace.totalBytes,
      dependencyRounds: trace.dependencyRounds,
      cacheHits: trace.cacheHits,
      decompression: decompressionSummary(
        options.decompressorSamples.samplesMs,
      ),
      integrityMs: trace.integrityMs,
      decompressionWallMs: trace.decompressionMs,
      decompressionTaskMs: trace.decompressionTaskMs,
      decodeMs: trace.decodeMs,
      mergeMs: trace.mergeMs,
      selectedChunks: trace.selectedChunks,
      selectedNodes: trace.selectedNodes,
      selectedTraversals: trace.selectedTraversals,
      ...(trace.canonicalHash.length === 0
        ? {}
        : { canonicalHash: trace.canonicalHash }),
      ...(options.query.expectedCanonicalHash === undefined
        ? {}
        : { expectedCanonicalHash: options.query.expectedCanonicalHash }),
      correctness: expectedErrorMatched || canonicalMatched,
      ...(failure === undefined ? {} : { error: failure }),
      ...(options.query.expectedError === undefined
        ? {}
        : { expectedError: options.query.expectedError }),
      peakHeapBytes: Math.max(heapBefore, heapAfter),
    },
  };
}

async function remoteIdentity(url: string): Promise<{
  size: string;
  etag?: string;
}> {
  const response = await fetch(url, { method: "HEAD" });
  if (!response.ok) {
    throw new Error(`archive HEAD returned HTTP ${response.status}`);
  }
  const length = response.headers.get("content-length");
  if (length === null || !/^\d+$/.test(length)) {
    throw new Error("archive HEAD did not provide a valid Content-Length");
  }
  const etag = response.headers.get("etag");
  return { size: length, ...(etag === null ? {} : { etag }) };
}

function workspaceDirectory(): string {
  return fileURLToPath(new URL("../../..", import.meta.url));
}

async function decoderAssetBytes(name: DecoderName): Promise<{
  javascriptBytes: number;
  wasmBytes: number;
}> {
  const workspace = workspaceDirectory();
  const reader = await stat(
    join(workspace, "packages/browser/dist/reader/index.js"),
  );
  if (name === "pure-js") {
    return { javascriptBytes: reader.size, wasmBytes: 0 };
  }
  const packageRoot = dirname(
    fileURLToPath(import.meta.resolve("@bokuweb/zstd-wasm")),
  );
  const javascriptFiles = (await readdir(packageRoot, { recursive: true }))
    .filter((path) => path.endsWith(".js"))
    .map((path) => join(packageRoot, path));
  const javascript = await Promise.all(
    javascriptFiles.map((path) => stat(path)),
  );
  const wasm = await stat(join(packageRoot, "zstd.wasm"));
  return {
    javascriptBytes:
      reader.size + javascript.reduce((total, file) => total + file.size, 0),
    wasmBytes: wasm.size,
  };
}

export interface ArchiveBenchmarkOptions {
  readonly url?: string;
  readonly file?: string;
  readonly workloadPath: string;
  readonly runId: string;
  readonly resultsDirectory: string;
  readonly modes: readonly CacheMode[];
  readonly decoders: readonly DecoderName[];
  readonly directoryCacheBytes: number;
  readonly payloadCacheBytes: number;
  readonly httpCache?: RequestCache;
}

export async function runArchiveBenchmark(
  options: ArchiveBenchmarkOptions,
): Promise<{ directory: string; summary: ReturnType<typeof buildSummary> }> {
  if ((options.url === undefined) === (options.file === undefined)) {
    throw new TypeError(
      "archive benchmark requires exactly one of url or file",
    );
  }
  const { workload, sha256: workloadSha256 } = await loadWorkload(
    options.workloadPath,
  );
  let archive: ArchiveIdentity;
  if (options.file !== undefined) {
    const metadata = await stat(options.file);
    const sha256 = await sha256File(options.file);
    if (sha256 !== workload.archiveSha256) {
      throw new Error(
        `archive SHA-256 ${sha256} does not match workload ${workload.archiveSha256}`,
      );
    }
    archive = {
      location: options.file,
      kind: "file",
      size: String(metadata.size),
      sha256,
    };
  } else {
    const metadata = await remoteIdentity(options.url as string);
    archive = {
      location: options.url as string,
      kind: "http",
      size: metadata.size,
      sha256: workload.archiveSha256,
      ...(metadata.etag === undefined ? {} : { etag: metadata.etag }),
    };
  }
  const files = await createRunFiles(options.resultsDirectory, options.runId);

  const measurements: QueryMeasurement[] = [];
  const requests: SerializableRequest[] = [];
  const decoderSummaries: DecoderSummary[] = [];
  let globalSequence = 0;
  for (const decoderName of options.decoders) {
    const initialized = await initializeDecoder(decoderName);
    const decoderSamples: number[] = [];
    let peakHeapBytes = process.memoryUsage().heapUsed;
    for (const mode of options.modes) {
      if (mode === "cold") {
        for (const query of workload.queries) {
          const observer =
            options.url === undefined
              ? await fileObserver(options.file as string)
              : httpObserver(options.url, options.httpCache);
          const openStarted = performance.now();
          const opened = await openPangenome({
            source: observer.source,
            directoryCacheBytes: options.directoryCacheBytes,
            payloadCacheBytes: options.payloadCacheBytes,
            decompressor: initialized.decompressor,
          });
          const openMs = performance.now() - openStarted;
          try {
            const result = await runQuery({
              archive: opened,
              observer,
              query,
              decoder: decoderName,
              scenario: "cold-library",
              cacheMode: "cold",
              httpCachePolicy: options.httpCache ?? "default",
              openMs,
              decompressorSamples: initialized.decompressor,
            });
            measurements.push(result.measurement);
            decoderSamples.push(...result.measurement.decompression.samplesMs);
            peakHeapBytes = Math.max(
              peakHeapBytes,
              result.measurement.peakHeapBytes ?? 0,
            );
            requests.push(
              ...result.requests.map((request) => ({
                ...request,
                sequence: globalSequence++,
              })),
            );
          } finally {
            await opened.close();
          }
        }
      } else {
        const observer =
          options.url === undefined
            ? await fileObserver(options.file as string)
            : httpObserver(options.url, options.httpCache);
        const opened = await openPangenome({
          source: observer.source,
          directoryCacheBytes: options.directoryCacheBytes,
          payloadCacheBytes: options.payloadCacheBytes,
          decompressor: initialized.decompressor,
        });
        try {
          for (const query of workload.queries) {
            try {
              await opened.query({ ...query, trace: false });
            } catch {
              // Expected-negative workload cases are measured below as failures.
            }
            const result = await runQuery({
              archive: opened,
              observer,
              query,
              decoder: decoderName,
              scenario: "warm-repeated-query",
              cacheMode: "warm",
              httpCachePolicy: options.httpCache ?? "default",
              openMs: 0,
              decompressorSamples: initialized.decompressor,
            });
            measurements.push(result.measurement);
            decoderSamples.push(...result.measurement.decompression.samplesMs);
            peakHeapBytes = Math.max(
              peakHeapBytes,
              result.measurement.peakHeapBytes ?? 0,
            );
            requests.push(
              ...result.requests.map((request) => ({
                ...request,
                sequence: globalSequence++,
              })),
            );
          }
        } finally {
          await opened.close();
        }
      }
    }
    const assets = await decoderAssetBytes(decoderName);
    decoderSummaries.push({
      name: decoderName,
      initializationMs: initialized.initializationMs,
      ...assets,
      decompression: decompressionSummary(decoderSamples),
      peakHeapBytes,
      limitation:
        "process.heapUsed is a coarse Node heap observation, not native/WASM peak RSS attribution",
    });
  }
  const limitations = [
    options.url === undefined
      ? "Positioned-file reads measure the reader and local storage path; they are not HTTP or browser measurements."
      : "Remote archive identity uses the workload checksum plus live size/ETag; the benchmark does not download the complete object to recompute SHA-256.",
    "Cold library mode creates a new archive reader per query; operating-system and remote CDN cache state are uncontrolled.",
    "Warm mode primes the exact query, retaining both directory and compressed-payload library caches within their configured byte budgets.",
  ];
  const summary = buildSummary({
    runId: options.runId,
    kind: "node",
    archive,
    workloadSha256,
    measurements,
    requests,
    decoderSummaries,
    limitations,
  });
  await writeRun({
    files,
    config: {
      schemaVersion: 1,
      command: "archive",
      archive,
      workloadPath: options.workloadPath,
      workload,
      modes: options.modes,
      decoders: options.decoders,
      libraryCache: {
        directoryBytes: options.directoryCacheBytes,
        payloadBytes: options.payloadCacheBytes,
      },
      httpCachePolicy: options.httpCache ?? "default",
    },
    environment: await collectEnvironment(),
    summary,
    requests,
  });
  if (summary.totals.failed > 0) {
    throw new Error(
      `${summary.totals.failed} of ${summary.totals.queries} benchmark queries failed correctness`,
    );
  }
  return { directory: files.directory, summary };
}
