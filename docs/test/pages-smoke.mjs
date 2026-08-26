import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const docsDirectory = dirname(dirname(fileURLToPath(import.meta.url)));
const repository = dirname(docsDirectory);
const siteDirectory = join(docsDirectory, ".vitepress", "dist");
const fixturePath = join(
  repository,
  "test-data",
  "conformance",
  "format-v1.pngr",
);
const fixture = await readFile(fixturePath);
const etag = `"sha256-${createHash("sha256").update(fixture).digest("hex")}"`;
const requests = [];

await stat(join(siteDirectory, "demo.html")).catch(() => {
  throw new Error("built Pages site is missing; run pnpm docs:build first");
});

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", "http://127.0.0.1");
  if (!url.pathname.startsWith("/pangenome-range/")) {
    respond(
      response,
      404,
      "text/plain",
      Buffer.from("Pages base path required"),
    );
    return;
  }
  if (url.pathname.endsWith("/slow.pngr")) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    serveArchive(request, response, fixture, false);
    return;
  }
  if (url.pathname.endsWith("/broken.pngr")) {
    serveArchive(request, response, fixture, true);
    return;
  }
  if (url.pathname.endsWith(".pngr")) {
    serveArchive(request, response, fixture, false);
    return;
  }
  const relative = url.pathname.slice("/pangenome-range/".length);
  const requestedFile = relative.length === 0 ? "index.html" : relative;
  const withHtml =
    extname(requestedFile).length === 0
      ? `${requestedFile}.html`
      : requestedFile;
  const filePath = normalize(join(siteDirectory, withHtml));
  if (!filePath.startsWith(siteDirectory)) {
    respond(response, 403, "text/plain", Buffer.from("Forbidden"));
    return;
  }
  try {
    const bytes = await readFile(filePath);
    respond(response, 200, contentType(filePath), bytes);
  } catch {
    respond(response, 404, "text/plain", Buffer.from("Not found"));
  }
});

await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
assert(address !== null && typeof address === "object");
const baseUrl = `http://127.0.0.1:${address.port}/pangenome-range`;
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1050 } });
const browserErrors = [];
page.on("pageerror", (error) => browserErrors.push(error.message));

try {
  await page.goto(
    `${baseUrl}/demo?archive=fixture&sample=GRCh38&contig=chr1&start=100&end=102&context=100`,
  );
  await page.locator('.phase[data-phase="ready"]').waitFor();
  await assertHealthyDemo(page);
  assert(
    requests.some(
      (request) =>
        request.path.endsWith("/fixtures/format-v1.pngr") &&
        request.status === 206,
    ),
    "the built demo did not make a real 206 request for its bundled archive",
  );

  await page.getByRole("button", { name: "Zoom in" }).click();
  await page.locator('[data-viewer-zoom="1.250"]').waitFor();
  await exerciseNodeHoverAndSelection(page);
  await page.getByRole("button", { name: "Pan right" }).click();
  await page.getByRole("button", { name: "Reset view" }).click();
  await page.locator('[data-viewer-zoom="1.000"]').waitFor();

  const remoteRequestsBeforeLocal = requests.length;
  await page.locator('input[type="file"]').setInputFiles(fixturePath);
  await page.getByRole("button", { name: "Load region" }).click();
  await page.locator('.phase[data-phase="ready"]').waitFor();
  await page.getByText("format-v1.pngr (local file)").waitFor();
  assert.equal(
    requests.length,
    remoteRequestsBeforeLocal,
    "local file mode unexpectedly performed an HTTP request",
  );

  await page.getByLabel("Archive source").selectOption("custom");
  await page.getByLabel("Remote archive URL").fill(`${baseUrl}/slow.pngr`);
  await page.getByRole("button", { name: "Load region" }).click();
  await page.locator('.phase[data-phase="opening"]').waitFor();
  await page.getByLabel("Archive source").selectOption("fixture");
  await page.getByRole("button", { name: /Load/ }).click();
  await page.locator('.phase[data-phase="ready"]').waitFor();
  assert.equal(
    await page.getByRole("alert").count(),
    0,
    "cancelled load leaked a stale error",
  );

  await page.getByLabel("Archive source").selectOption("custom");
  await page.getByLabel("Remote archive URL").fill(`${baseUrl}/broken.pngr`);
  await page.getByRole("button", { name: "Load region" }).click();
  await page.getByRole("alert").waitFor();
  await assertContains(
    await page.getByRole("alert").innerText(),
    /206|Content-Range|range/i,
  );

  await page.getByLabel("Archive source").selectOption("fixture");
  await page.getByRole("button", { name: /Load/ }).click();
  await page.locator('.phase[data-phase="ready"]').waitFor();
  await page.screenshot({
    path:
      process.env.PANGENOME_RANGE_DEMO_SCREENSHOT ??
      "/tmp/pangenome-range-demo.png",
    fullPage: true,
  });
  assert.deepEqual(browserErrors, []);
  console.log(
    JSON.stringify(
      {
        pagesBaseUrl: baseUrl,
        actualRangeResponses: requests.filter(
          (request) => request.status === 206,
        ).length,
        localFilePassed: true,
        cancellationPassed: true,
        actionableRangeErrorPassed: true,
        screenshot:
          process.env.PANGENOME_RANGE_DEMO_SCREENSHOT ??
          "/tmp/pangenome-range-demo.png",
      },
      null,
      2,
    ),
  );
} finally {
  await browser.close();
  await new Promise((resolve, reject) =>
    server.close((error) => (error === undefined ? resolve() : reject(error))),
  );
}

async function assertHealthyDemo(page) {
  await page
    .getByRole("img", { name: /Pangenome graph for GRCh38 chr1/ })
    .waitFor();
  await page.getByText("anonymous-distinct-weighted-tile-paths").waitFor();
  await page.getByText("Canonical hash:").waitFor();
  assert.equal(await page.getByLabel("Archive source").inputValue(), "fixture");
  assert.equal(
    await page.getByLabel("Reference / sample").inputValue(),
    "GRCh38",
  );
  assert.equal(await page.getByLabel("Contig").inputValue(), "chr1");
  assert.equal(await page.getByLabel("Preset region").count(), 1);
  assert.equal(
    await page.getByLabel("Pangenome graph visualization").count(),
    1,
  );
}

async function exerciseNodeHoverAndSelection(page) {
  const canvas = page.locator("[data-viewer-canvas]");
  const box = await canvas.boundingBox();
  assert(box !== null, "viewer canvas has no browser layout box");
  let found = false;
  for (const xFraction of [0.2, 0.3, 0.4, 0.6, 0.7, 0.8]) {
    for (const yFraction of [0.34, 0.42, 0.5]) {
      await page.mouse.move(
        box.x + box.width * xFraction,
        box.y + box.height * yFraction,
      );
      const detail = await page.locator("[data-viewer-detail]").innerText();
      if (/^node \d+/.test(detail)) {
        await page.mouse.down();
        await page.mouse.up();
        assert.match(
          await page.locator("[data-viewer-detail]").innerText(),
          /^node \d+/,
        );
        found = true;
        break;
      }
    }
    if (found) break;
  }
  assert(found, "hover scan did not find a rendered node");
}

function serveArchive(request, response, bytes, broken) {
  const range = request.headers.range;
  if (range === undefined) {
    record(request, 200, range);
    archiveHeaders(response, bytes.byteLength);
    response.writeHead(200, { "Content-Length": bytes.byteLength });
    response.end(request.method === "HEAD" ? undefined : bytes);
    return;
  }
  const match = /^bytes=(\d+)-(\d+)$/.exec(range);
  if (match === null) {
    record(request, 416, range);
    response.writeHead(416, { "Content-Range": `bytes */${bytes.byteLength}` });
    response.end();
    return;
  }
  const start = Number(match[1]);
  const end = Math.min(Number(match[2]), bytes.byteLength - 1);
  const body = bytes.subarray(start, end + 1);
  record(request, 206, range);
  archiveHeaders(response, bytes.byteLength);
  response.writeHead(206, {
    "Content-Length": body.byteLength,
    "Content-Range": broken
      ? `bytes ${start + 1}-${end}/${bytes.byteLength}`
      : `bytes ${start}-${end}/${bytes.byteLength}`,
  });
  response.end(body);
}

function archiveHeaders(response, size) {
  response.setHeader("Accept-Ranges", "bytes");
  response.setHeader("Access-Control-Allow-Origin", "*");
  response.setHeader(
    "Access-Control-Expose-Headers",
    "Accept-Ranges, Content-Range, Content-Length, ETag",
  );
  response.setHeader(
    "Cache-Control",
    "public, max-age=31536000, immutable, no-transform",
  );
  response.setHeader("Content-Type", "application/octet-stream");
  response.setHeader("ETag", etag);
  response.setHeader("X-Archive-Size", String(size));
}

function record(request, status, range) {
  requests.push({
    method: request.method,
    path: new URL(request.url ?? "/", "http://127.0.0.1").pathname,
    range,
    status,
  });
}

function respond(response, status, type, bytes) {
  response.writeHead(status, {
    "Content-Type": type,
    "Content-Length": bytes.byteLength,
  });
  response.end(bytes);
}

function contentType(path) {
  switch (extname(path)) {
    case ".html":
      return "text/html; charset=utf-8";
    case ".js":
      return "text/javascript; charset=utf-8";
    case ".css":
      return "text/css; charset=utf-8";
    case ".json":
      return "application/json";
    case ".svg":
      return "image/svg+xml";
    case ".woff2":
      return "font/woff2";
    default:
      return "application/octet-stream";
  }
}

async function assertContains(value, pattern) {
  assert.match(value, pattern);
}
