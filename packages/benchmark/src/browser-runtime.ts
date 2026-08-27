import {
  type ChunkDecompressor,
  FzstdDecompressor,
  HttpRangeSource,
  openPangenome,
  type PangenomeArchive,
  type QueryTrace,
  type RegionQuery,
} from "pangenome-range/reader";

type RuntimeDecoder = "pure-js" | "wasm";

interface TimedDecoder extends ChunkDecompressor {
  readonly samplesMs: number[];
  clear(): void;
}

interface RuntimeState {
  readonly archive: PangenomeArchive;
  readonly source: HttpRangeSource;
  readonly decoder: TimedDecoder;
  readonly initializationMs: number;
  readonly openMs: number;
}

interface BrowserRuntimeOptions {
  readonly archiveUrl: string;
  readonly query: Omit<RegionQuery, "signal" | "trace">;
  readonly decoder: RuntimeDecoder;
  readonly scenario: string;
  readonly reuseKey?: string;
  readonly httpCache?: RequestCache;
  readonly directoryCacheBytes: number;
  readonly payloadCacheBytes: number;
  readonly wasmUrl: string;
}

const states = new Map<string, RuntimeState>();
let wasmModule: Promise<typeof import("@bokuweb/zstd-wasm")> | undefined;

function timed(delegate: ChunkDecompressor): TimedDecoder {
  const samplesMs: number[] = [];
  return {
    samplesMs,
    clear(): void {
      samplesMs.length = 0;
    },
    decompress(
      compressed: Uint8Array,
      expectedLength: number,
      options?: { signal?: AbortSignal },
    ): Uint8Array | Promise<Uint8Array> {
      const started = performance.now();
      try {
        const result = delegate.decompress(compressed, expectedLength, options);
        if (result instanceof Uint8Array) {
          samplesMs.push(performance.now() - started);
          return result;
        }
        return Promise.resolve(result).finally(() => {
          samplesMs.push(performance.now() - started);
        });
      } catch (error) {
        samplesMs.push(performance.now() - started);
        throw error;
      }
    },
  };
}

async function decoder(
  name: RuntimeDecoder,
  wasmUrl: string,
): Promise<TimedDecoder> {
  if (name === "pure-js") return timed(new FzstdDecompressor());
  wasmModule ??= import("@bokuweb/zstd-wasm").then(async (wasm) => {
    const initialize = wasm.init as (path?: string) => Promise<void>;
    await initialize(wasmUrl);
    return wasm;
  });
  const wasm = await wasmModule;
  return timed({
    decompress(
      compressed: Uint8Array,
      expectedLength: number,
      options?: { signal?: AbortSignal },
    ): Uint8Array {
      options?.signal?.throwIfAborted();
      const result = wasm.decompress(compressed);
      options?.signal?.throwIfAborted();
      if (result.byteLength !== expectedLength) {
        throw new RangeError(
          `WASM zstd decoded ${result.byteLength} bytes, expected ${expectedLength}`,
        );
      }
      return result;
    },
  });
}

async function createState(
  options: BrowserRuntimeOptions,
): Promise<RuntimeState> {
  const initializationStarted = performance.now();
  const initializedDecoder = await decoder(options.decoder, options.wasmUrl);
  const initializationMs = performance.now() - initializationStarted;
  const source = new HttpRangeSource(options.archiveUrl, {
    ...(options.httpCache === undefined ? {} : { cache: options.httpCache }),
  });
  const openStarted = performance.now();
  const archive = await openPangenome({
    source,
    directoryCacheBytes: options.directoryCacheBytes,
    payloadCacheBytes: options.payloadCacheBytes,
    decompressor: initializedDecoder,
  });
  return {
    archive,
    source,
    decoder: initializedDecoder,
    initializationMs,
    openMs: performance.now() - openStarted,
  };
}

function memoryBytes(): number | undefined {
  const memory = (
    performance as Performance & {
      memory?: { usedJSHeapSize?: number };
    }
  ).memory;
  return memory?.usedJSHeapSize;
}

export async function runBrowserScenario(
  options: BrowserRuntimeOptions,
): Promise<Record<string, unknown>> {
  const scenarioMark = `${options.scenario}-${crypto.randomUUID()}`;
  performance.mark(`${scenarioMark}-start`);
  let state =
    options.reuseKey === undefined ? undefined : states.get(options.reuseKey);
  const created = state === undefined;
  state ??= await createState(options);
  if (options.reuseKey !== undefined && created) {
    states.set(options.reuseKey, state);
  }
  const requestStart = created ? 0 : state.source.requests.length;
  state.decoder.clear();
  performance.mark(`${scenarioMark}-query-start`);
  const heapBefore = memoryBytes();
  let trace: QueryTrace | undefined;
  let error: string | undefined;
  try {
    const result = await state.archive.query({ ...options.query, trace: true });
    trace = result.trace;
  } catch (caught) {
    error =
      caught instanceof Error
        ? `${caught.name}: ${caught.message}`
        : String(caught);
  }
  performance.mark(`${scenarioMark}-query-end`);
  performance.measure(
    `${scenarioMark}-query`,
    `${scenarioMark}-query-start`,
    `${scenarioMark}-query-end`,
  );
  const queryMs =
    performance.getEntriesByName(`${scenarioMark}-query`).at(-1)?.duration ?? 0;
  const heapAfter = memoryBytes();
  const requests = state.source.requests.slice(requestStart).map((request) => ({
    ...request,
    ...(request.offset === undefined
      ? {}
      : { offset: request.offset.toString() }),
  }));
  performance.mark(`${scenarioMark}-end`);
  performance.measure(
    `${scenarioMark}-total`,
    `${scenarioMark}-start`,
    `${scenarioMark}-end`,
  );
  const totalMs =
    performance.getEntriesByName(`${scenarioMark}-total`).at(-1)?.duration ?? 0;
  const decompressionMs = state.decoder.samplesMs.reduce(
    (total, sample) => total + sample,
    0,
  );
  const result = {
    initializationMs: state.initializationMs,
    openMs: options.reuseKey === undefined ? state.openMs : 0,
    queryMs,
    totalMs,
    sourceRequests: requests,
    decompressionSamplesMs: [...state.decoder.samplesMs],
    ...(trace === undefined
      ? {}
      : {
          trace: {
            ...trace,
            requestRanges: trace.requestRanges.map((range) => ({
              ...range,
              offset: range.offset.toString(),
            })),
          },
        }),
    ...(error === undefined ? {} : { error }),
    ...((heapBefore ?? heapAfter) === undefined
      ? {}
      : { peakHeapBytes: Math.max(heapBefore ?? 0, heapAfter ?? 0) }),
    performanceMarks: {
      initializationMs: created ? state.initializationMs : 0,
      openMs: created ? state.openMs : 0,
      queryMs,
      decompressionWallMs: trace?.decompressionMs ?? 0,
      decompressionTaskMs: trace?.decompressionTaskMs ?? decompressionMs,
      decodeMs: trace?.decodeMs ?? 0,
      mergeMs: trace?.mergeMs ?? 0,
      totalMs,
    },
  };
  if (options.reuseKey === undefined) await state.archive.close();
  return result;
}

export async function closeBrowserScenarios(): Promise<void> {
  await Promise.all([...states.values()].map(({ archive }) => archive.close()));
  states.clear();
}
