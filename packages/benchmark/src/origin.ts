import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { createServer, type Server } from "node:http";
import type { Socket } from "node:net";
import { Transform, type TransformCallback } from "node:stream";

export interface OriginArchive {
  readonly route: string;
  readonly path?: string;
  readonly bytes?: Uint8Array;
  readonly etag: string;
}

export interface OriginFaults {
  readonly ignoreRange?: boolean;
  readonly malformedContentRange?: boolean;
  readonly truncateBytes?: number;
  readonly etagChange?: boolean;
  readonly latencyMs?: number;
  readonly bandwidthBytesPerSecond?: number;
  readonly missingCors?: boolean;
  readonly missingExposedHeaders?: boolean;
}

export interface OriginRequestLog {
  readonly sequence: number;
  readonly method: string;
  readonly path: string;
  readonly range: string | null;
  readonly ifRange: string | null;
  readonly status: number;
  readonly bytes: number;
  readonly startedAt: string;
  readonly startedAtMs: number;
  readonly elapsedMs: number;
  readonly connectionId: string;
  readonly etag: string | null;
}

interface PreparedArchive extends OriginArchive {
  readonly size: number;
  readonly buffer?: Buffer;
}

export interface RangeOrigin {
  readonly baseUrl: string;
  readonly urls: Readonly<Record<string, string>>;
  readonly requests: OriginRequestLog[];
  clearRequests(): void;
  close(): Promise<void>;
}

class BandwidthThrottle extends Transform {
  readonly #bytesPerSecond: number;

  constructor(bytesPerSecond: number) {
    super();
    this.#bytesPerSecond = bytesPerSecond;
  }

  override _transform(
    chunk: Buffer,
    _encoding: BufferEncoding,
    callback: TransformCallback,
  ): void {
    const delay = Math.ceil((chunk.byteLength / this.#bytesPerSecond) * 1_000);
    setTimeout(() => callback(null, chunk), delay);
  }
}

function parseRange(
  value: string | undefined,
  size: number,
): { start: number; end: number } | undefined {
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

function validateFaults(faults: OriginFaults): void {
  for (const [value, label] of [
    [faults.truncateBytes, "truncateBytes"],
    [faults.latencyMs, "latencyMs"],
    [faults.bandwidthBytesPerSecond, "bandwidthBytesPerSecond"],
  ] as const) {
    if (value !== undefined && (!Number.isSafeInteger(value) || value <= 0)) {
      throw new RangeError(`${label} must be a positive safe integer`);
    }
  }
}

function wait(milliseconds: number | undefined): Promise<void> {
  return milliseconds === undefined
    ? Promise.resolve()
    : new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function route(value: string): string {
  if (!value.startsWith("/") || value.includes("?") || value.includes("#")) {
    throw new TypeError(`invalid archive route ${JSON.stringify(value)}`);
  }
  return value;
}

async function prepareArchive(
  archive: OriginArchive,
): Promise<PreparedArchive> {
  if ((archive.path === undefined) === (archive.bytes === undefined)) {
    throw new TypeError(
      `archive ${archive.route} must provide exactly one of path or bytes`,
    );
  }
  const buffer =
    archive.bytes === undefined ? undefined : Buffer.from(archive.bytes);
  const size =
    buffer?.byteLength ?? Number((await stat(archive.path as string)).size);
  if (!Number.isSafeInteger(size) || size <= 0) {
    throw new RangeError(`archive ${archive.route} has an invalid size`);
  }
  return {
    ...archive,
    route: route(archive.route),
    size,
    ...(buffer === undefined ? {} : { buffer }),
  };
}

function commonHeaders(
  archive: PreparedArchive,
  etag: string,
  faults: OriginFaults,
  corsOrigin: string,
): Record<string, string> {
  return {
    "Accept-Ranges": "bytes",
    "Cache-Control": "public, max-age=31536000, immutable, no-transform",
    "Content-Encoding": "identity",
    "Content-Type": "application/octet-stream",
    ETag: etag,
    Vary: "Origin",
    ...(!faults.missingCors
      ? {
          "Access-Control-Allow-Headers": "Range, If-Range",
          "Access-Control-Allow-Methods": "GET, HEAD, OPTIONS",
          "Access-Control-Allow-Origin": corsOrigin,
        }
      : {}),
    ...(!faults.missingExposedHeaders
      ? {
          "Access-Control-Expose-Headers":
            "Accept-Ranges, Content-Range, Content-Length, Content-Encoding, ETag",
        }
      : {}),
    "X-Pangenome-Range-Object-Bytes": String(archive.size),
  };
}

function changedEtag(etag: string): string {
  return etag.endsWith('"')
    ? `${etag.slice(0, -1)}-changed"`
    : `${etag}-changed`;
}

export async function createRangeOrigin(options: {
  readonly archives: readonly OriginArchive[];
  readonly faults?: OriginFaults;
  readonly host?: string;
  readonly corsOrigin?: string;
}): Promise<RangeOrigin> {
  if (options.archives.length === 0) {
    throw new TypeError("range origin requires at least one archive");
  }
  const faults = options.faults ?? {};
  validateFaults(faults);
  const prepared = await Promise.all(options.archives.map(prepareArchive));
  const byRoute = new Map(prepared.map((archive) => [archive.route, archive]));
  if (byRoute.size !== prepared.length) {
    throw new TypeError("archive routes must be unique");
  }
  const requests: OriginRequestLog[] = [];
  const connections = new WeakMap<Socket, string>();
  let nextConnection = 1;
  let nextSequence = 0;
  let identityRequestCount = 0;

  const connectionId = (socket: Socket): string => {
    let id = connections.get(socket);
    if (id === undefined) {
      id = `connection-${nextConnection}`;
      nextConnection += 1;
      connections.set(socket, id);
    }
    return id;
  };

  const server: Server = createServer(async (request, response) => {
    const started = performance.now();
    const startedAt = new Date().toISOString();
    const sequence = nextSequence;
    nextSequence += 1;
    const path = new URL(request.url ?? "/", "http://range.invalid").pathname;
    const archive = byRoute.get(path);
    let status = 500;
    let responseBytes = 0;
    let responseEtag: string | null = null;
    const finish = (): void => {
      requests.push({
        sequence,
        method: request.method ?? "UNKNOWN",
        path,
        range: request.headers.range ?? null,
        ifRange: Array.isArray(request.headers["if-range"])
          ? request.headers["if-range"].join(", ")
          : (request.headers["if-range"] ?? null),
        status,
        bytes: responseBytes,
        startedAt,
        startedAtMs: started,
        elapsedMs: performance.now() - started,
        connectionId: connectionId(request.socket),
        etag: responseEtag,
      });
    };
    response.once("finish", finish);
    response.once("close", () => {
      if (!response.writableFinished) finish();
    });

    if (archive === undefined) {
      status = 404;
      response.writeHead(status);
      response.end();
      return;
    }
    const etag =
      faults.etagChange && identityRequestCount > 0
        ? changedEtag(archive.etag)
        : archive.etag;
    identityRequestCount += 1;
    responseEtag = etag;
    const common = commonHeaders(
      archive,
      etag,
      faults,
      options.corsOrigin ?? "*",
    );
    if (request.method === "OPTIONS") {
      status = 204;
      response.writeHead(status, common);
      response.end();
      return;
    }
    if (request.method === "HEAD") {
      await wait(faults.latencyMs);
      status = 200;
      response.writeHead(status, { ...common, "Content-Length": archive.size });
      response.end();
      return;
    }
    if (request.method !== "GET") {
      status = 405;
      response.writeHead(status, common);
      response.end();
      return;
    }

    const parsedRange = parseRange(request.headers.range, archive.size);
    if (parsedRange === undefined && !faults.ignoreRange) {
      status = 416;
      response.writeHead(status, {
        ...common,
        "Content-Range": `bytes */${archive.size}`,
      });
      response.end();
      return;
    }
    await wait(faults.latencyMs);
    const start = faults.ignoreRange
      ? 0
      : (parsedRange as { start: number }).start;
    const end = faults.ignoreRange
      ? archive.size - 1
      : (parsedRange as { end: number }).end;
    const declaredLength = end - start + 1;
    const truncatedLength = Math.max(
      0,
      declaredLength - (faults.truncateBytes ?? 0),
    );
    responseBytes = truncatedLength;
    status = faults.ignoreRange ? 200 : 206;
    const contentRangeStart = faults.malformedContentRange ? start + 1 : start;
    response.writeHead(status, {
      ...common,
      "Content-Length": declaredLength,
      ...(status === 206
        ? {
            "Content-Range": `bytes ${contentRangeStart}-${end}/${archive.size}`,
          }
        : {}),
    });
    if (truncatedLength === 0) {
      response.end();
      return;
    }
    const actualEnd = start + truncatedLength - 1;
    if (archive.buffer !== undefined) {
      const body = archive.buffer.subarray(start, actualEnd + 1);
      if (faults.truncateBytes !== undefined) {
        response.write(body);
        setImmediate(() => response.destroy());
        return;
      }
      if (faults.bandwidthBytesPerSecond === undefined) {
        response.end(body);
      } else {
        const throttle = new BandwidthThrottle(faults.bandwidthBytesPerSecond);
        throttle.pipe(response);
        throttle.end(body);
      }
      return;
    }
    const stream = createReadStream(archive.path as string, {
      start,
      end: actualEnd,
    });
    stream.on("error", (error) => response.destroy(error));
    if (faults.bandwidthBytesPerSecond === undefined) {
      stream.pipe(response);
    } else {
      stream
        .pipe(new BandwidthThrottle(faults.bandwidthBytesPerSecond))
        .pipe(response);
    }
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, options.host ?? "127.0.0.1", resolve);
  });
  const address = server.address();
  if (typeof address === "string" || address === null) {
    throw new Error("range origin did not bind an INET port");
  }
  const baseUrl = `http://${options.host ?? "127.0.0.1"}:${address.port}`;
  return {
    baseUrl,
    urls: Object.fromEntries(
      prepared.map((archive) => [archive.route, `${baseUrl}${archive.route}`]),
    ),
    requests,
    clearRequests(): void {
      requests.length = 0;
    },
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.close((error) =>
          error === undefined ? resolve() : reject(error),
        );
        server.closeAllConnections();
      }),
  };
}
