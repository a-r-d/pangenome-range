import { writeFile } from "node:fs/promises";
import { FileRangeSource } from "pangenome-range/node";
import { sha256File } from "./reporting.js";

export interface OriginCheckOptions {
  readonly url: string;
  readonly requestOrigin?: string;
  readonly localFile?: string;
  readonly expectedSha256?: string;
  readonly reportPath?: string;
}

export interface OriginCheckResult {
  readonly schemaVersion: 1;
  readonly url: string;
  readonly passed: boolean;
  readonly size?: string;
  readonly etag?: string;
  readonly expectedSha256?: string;
  readonly ranges: ReadonlyArray<{
    readonly start: string;
    readonly end: string;
    readonly status: number;
    readonly bytes: number;
    readonly elapsedMs: number;
    readonly matchedLocal: boolean | null;
  }>;
  readonly checks: ReadonlyArray<{
    readonly name: string;
    readonly passed: boolean;
    readonly detail: string;
  }>;
}

const EXPOSED_HEADERS = [
  "accept-ranges",
  "content-range",
  "content-length",
  "etag",
];

function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  return (
    left.byteLength === right.byteLength &&
    left.every((value, index) => value === right[index])
  );
}

export async function checkOrigin(
  options: OriginCheckOptions,
): Promise<OriginCheckResult> {
  const checks: Array<{ name: string; passed: boolean; detail: string }> = [];
  const check = (name: string, passed: boolean, detail: string): void => {
    checks.push({ name, passed, detail });
  };
  const requestOrigin = options.requestOrigin ?? "https://example.invalid";
  const headers = { Origin: requestOrigin };
  const headStarted = performance.now();
  const head = await fetch(options.url, { method: "HEAD", headers });
  const headMs = performance.now() - headStarted;
  check(
    "HEAD",
    head.status === 200,
    `HTTP ${head.status} in ${headMs.toFixed(3)} ms`,
  );
  const lengthHeader = head.headers.get("content-length");
  const size =
    lengthHeader !== null && /^\d+$/.test(lengthHeader)
      ? BigInt(lengthHeader)
      : undefined;
  check(
    "Content-Length",
    size !== undefined && size > 0n,
    lengthHeader ?? "missing",
  );
  check(
    "Accept-Ranges",
    head.headers.get("accept-ranges")?.toLowerCase() === "bytes",
    head.headers.get("accept-ranges") ?? "missing",
  );
  const etag = head.headers.get("etag") ?? undefined;
  check("ETag", etag !== undefined && etag.length > 0, etag ?? "missing");
  const encoding = head.headers.get("content-encoding");
  check(
    "identity encoding",
    encoding === null || encoding.toLowerCase() === "identity",
    encoding ?? "implicit identity",
  );
  check(
    "no-transform",
    head.headers
      .get("cache-control")
      ?.toLowerCase()
      .includes("no-transform") === true,
    head.headers.get("cache-control") ?? "missing",
  );
  const allowOrigin = head.headers.get("access-control-allow-origin");
  check(
    "CORS",
    allowOrigin === "*" || allowOrigin === requestOrigin,
    `${allowOrigin ?? "missing"} (requested ${requestOrigin})`,
  );
  const exposed = new Set(
    (head.headers.get("access-control-expose-headers") ?? "")
      .split(",")
      .map((value) => value.trim().toLowerCase())
      .filter(Boolean),
  );
  const missingExposed = EXPOSED_HEADERS.filter(
    (header) => !exposed.has(header),
  );
  check(
    "exposed range headers",
    missingExposed.length === 0,
    missingExposed.length === 0
      ? EXPOSED_HEADERS.join(", ")
      : `missing ${missingExposed.join(", ")}`,
  );
  const preflight = await fetch(options.url, {
    method: "OPTIONS",
    headers: {
      Origin: requestOrigin,
      "Access-Control-Request-Method": "GET",
      "Access-Control-Request-Headers": "Range, If-Range",
    },
  });
  const preflightOrigin = preflight.headers.get("access-control-allow-origin");
  const allowedMethods = new Set(
    (preflight.headers.get("access-control-allow-methods") ?? "")
      .split(",")
      .map((value) => value.trim().toUpperCase()),
  );
  const allowedHeaders = new Set(
    (preflight.headers.get("access-control-allow-headers") ?? "")
      .split(",")
      .map((value) => value.trim().toLowerCase()),
  );
  check(
    "CORS preflight",
    preflight.ok &&
      (preflightOrigin === "*" || preflightOrigin === requestOrigin) &&
      (allowedMethods.has("*") || allowedMethods.has("GET")) &&
      (allowedHeaders.has("*") ||
        (allowedHeaders.has("range") && allowedHeaders.has("if-range"))),
    `HTTP ${preflight.status}, origin ${preflightOrigin ?? "missing"}, methods ${[...allowedMethods].join(",") || "missing"}, headers ${[...allowedHeaders].join(",") || "missing"}`,
  );

  let local: FileRangeSource | undefined;
  if (options.localFile !== undefined) {
    local = await FileRangeSource.open(options.localFile);
    const localSize = await local.size();
    check(
      "local size",
      size !== undefined && localSize === size,
      `local ${localSize}, remote ${size ?? "unknown"}`,
    );
    if (options.expectedSha256 !== undefined) {
      const localSha256 = await sha256File(options.localFile);
      check(
        "local SHA-256",
        localSha256 === options.expectedSha256,
        localSha256,
      );
    }
  } else if (options.expectedSha256 !== undefined) {
    check(
      "content-addressed ETag",
      etag?.includes(options.expectedSha256) === true,
      etag ?? "missing ETag; provide --file for byte comparison",
    );
  }

  const ranges: Array<{
    start: string;
    end: string;
    status: number;
    bytes: number;
    elapsedMs: number;
    matchedLocal: boolean | null;
  }> = [];
  if (size !== undefined && size > 0n) {
    const candidates: Array<readonly [bigint, bigint]> = [
      [0n, size < 16n ? size - 1n : 15n],
      [8n, size < 24n ? size - 1n : 23n],
      [24n, size < 40n ? size - 1n : 39n],
      [size > 32n ? size - 32n : 0n, size - 1n],
    ];
    for (const [start, end] of candidates) {
      if (start > end || start >= size) continue;
      const started = performance.now();
      const response = await fetch(options.url, {
        headers: {
          ...headers,
          Range: `bytes=${start}-${end}`,
          ...(etag === undefined ? {} : { "If-Range": etag }),
        },
      });
      const elapsedMs = performance.now() - started;
      if (response.status !== 206) {
        response.body?.cancel().catch(() => undefined);
        check(
          `range ${start}-${end}`,
          false,
          `HTTP ${response.status}; origin may have returned the full object`,
        );
        ranges.push({
          start: start.toString(),
          end: end.toString(),
          status: response.status,
          bytes: 0,
          elapsedMs,
          matchedLocal: null,
        });
        continue;
      }
      const bytes = new Uint8Array(await response.arrayBuffer());
      const expectedLength = Number(end - start + 1n);
      const contentRange = response.headers.get("content-range");
      const responseEtag = response.headers.get("etag");
      const responseAllowOrigin = response.headers.get(
        "access-control-allow-origin",
      );
      const responseExposed = new Set(
        (response.headers.get("access-control-expose-headers") ?? "")
          .split(",")
          .map((value) => value.trim().toLowerCase()),
      );
      const metadataCorrect =
        contentRange === `bytes ${start}-${end}/${size}` &&
        response.headers.get("content-length") === String(expectedLength) &&
        response.headers.get("accept-ranges")?.toLowerCase() === "bytes" &&
        bytes.byteLength === expectedLength &&
        responseEtag === etag &&
        (responseAllowOrigin === "*" ||
          responseAllowOrigin === requestOrigin) &&
        EXPOSED_HEADERS.every((header) => responseExposed.has(header)) &&
        (response.headers.get("content-encoding") === null ||
          response.headers.get("content-encoding")?.toLowerCase() ===
            "identity");
      let matchedLocal: boolean | null = null;
      if (local !== undefined) {
        const expected = await local.read(start, expectedLength);
        matchedLocal = equalBytes(bytes, expected);
      }
      check(
        `range ${start}-${end}`,
        metadataCorrect && matchedLocal !== false,
        `HTTP 206, ${bytes.byteLength} bytes, Content-Range ${contentRange ?? "missing"}, ETag ${responseEtag ?? "missing"}${matchedLocal === null ? "" : `, local match ${matchedLocal}`}`,
      );
      ranges.push({
        start: start.toString(),
        end: end.toString(),
        status: response.status,
        bytes: bytes.byteLength,
        elapsedMs,
        matchedLocal,
      });
    }
  }
  await local?.close();
  const result: OriginCheckResult = {
    schemaVersion: 1,
    url: options.url,
    passed: checks.every((item) => item.passed),
    ...(size === undefined ? {} : { size: size.toString() }),
    ...(etag === undefined ? {} : { etag }),
    ...(options.expectedSha256 === undefined
      ? {}
      : { expectedSha256: options.expectedSha256 }),
    ranges,
    checks,
  };
  if (options.reportPath !== undefined) {
    await writeFile(options.reportPath, `${JSON.stringify(result, null, 2)}\n`);
  }
  return result;
}

export function formatOriginCheck(result: OriginCheckResult): string {
  const lines = [
    `origin-check: ${result.passed ? "PASS" : "FAIL"}`,
    `url: ${result.url}`,
    `size: ${result.size ?? "unknown"}`,
    `etag: ${result.etag ?? "missing"}`,
  ];
  for (const check of result.checks) {
    lines.push(
      `${check.passed ? "PASS" : "FAIL"} ${check.name}: ${check.detail}`,
    );
  }
  return lines.join("\n");
}
