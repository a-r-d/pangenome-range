import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, firefox, webkit } from "@playwright/test";

const docsDirectory = dirname(dirname(fileURLToPath(import.meta.url)));
const repository = dirname(docsDirectory);
const siteDirectory = join(docsDirectory, ".vitepress", "dist");
const fixturePath = join(
  repository,
  "test-data",
  "conformance",
  "format-v1.pngr",
);
const indexedFixturePath = join(
  repository,
  "test-data",
  "golden",
  "record-archive-v1.pngr",
);
const fixture = await readFile(fixturePath);
const indexedFixture = await readFile(indexedFixturePath);
const etag = `"sha256-${createHash("sha256").update(fixture).digest("hex")}"`;
const requests = [];
const configuredArchiveUrl =
  process.env.VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL ?? "";
const screenshotBase =
  process.env.PANGENOME_RANGE_DEMO_SCREENSHOT ??
  "/tmp/pangenome-range-demo.png";
const screenshotPath = (state) =>
  screenshotBase.replace(/\.png$/, `-${state}.png`);

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
    await new Promise((resolve) => setTimeout(resolve, 300));
    serveArchive(request, response, fixture, false);
    return;
  }
  if (url.pathname.endsWith("/broken.pngr")) {
    serveArchive(request, response, fixture, true);
    return;
  }
  if (url.pathname.endsWith("/record.pngr")) {
    serveArchive(request, response, indexedFixture, false);
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
    respond(response, 200, contentType(filePath), await readFile(filePath));
  } catch {
    respond(response, 404, "text/plain", Buffer.from("Not found"));
  }
});

await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
assert(address !== null && typeof address === "object");
const baseUrl = `http://127.0.0.1:${address.port}/pangenome-range`;
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
const pageErrors = [];
page.on("pageerror", (error) => pageErrors.push(error.message));

try {
  await page.goto(
    `${baseUrl}/demo?archive=fixture&sample=GRCh38&contig=chr1&start=100&end=102`,
  );
  await waitForReady(page, "fixture browser");
  await assertHealthyBrowser(page);
  await assertNoPageOverflow(page, "fixture browser");
  assert(
    requests.some(
      (request) =>
        request.path.endsWith("/fixtures/format-v1.pngr") &&
        request.status === 206,
    ),
    "the browser did not perform a real 206 request for the fixture",
  );
  await page.screenshot({ path: screenshotPath("fixture"), fullPage: true });

  const search = page.getByRole("textbox", {
    name: "Search gene or coordinate",
  });
  await search.fill("GRCh38 chr1:100-101");
  await search.press("Enter");
  await waitForReady(page, "first coordinate");
  const firstHistoryUrl = page.url();
  await search.fill("GRCh38 chr1:101-102");
  await search.press("Enter");
  await waitForReady(page, "second coordinate");
  const secondHistoryUrl = page.url();
  assert.notEqual(firstHistoryUrl, secondHistoryUrl);
  await page.getByRole("button", { name: "Back" }).click();
  await page.waitForURL(firstHistoryUrl);
  await waitForReady(page, "history back");
  assert.equal(page.url(), firstHistoryUrl);
  await page.getByRole("button", { name: "Forward" }).click();
  await page.waitForURL(secondHistoryUrl);
  await waitForReady(page, "history forward");
  assert.equal(page.url(), secondHistoryUrl);
  await page.getByRole("button", { name: "Zoom in" }).click();
  await page.getByRole("button", { name: "Zoom out" }).click();
  await page.getByRole("button", { name: "Fit" }).click();
  await page.keyboard.press(
    process.platform === "darwin" ? "Meta+k" : "Control+k",
  );
  await page.waitForFunction(
    () =>
      document.activeElement?.getAttribute("aria-label") ===
      "Search gene or coordinate",
  );
  assert.equal(
    await search.evaluate((element) => element === document.activeElement),
    true,
  );

  const node = page.locator("[data-node-key]").first();
  if ((await node.count()) > 0) {
    await node.click();
    await page.getByRole("complementary", { name: "Node inspector" }).waitFor();
    await page.getByRole("button", { name: "Close inspector" }).click();
  }

  await page.getByRole("button", { name: "Archive" }).click();
  await page
    .getByPlaceholder("https://…/archive.pngr")
    .fill(`${baseUrl}/record.pngr`);
  await page.getByRole("button", { name: "Open remote URL" }).click();
  await waitForReady(page, "indexed archive");
  await search.fill("rang");
  await page.getByRole("option", { name: /PNGRTEST/ }).waitFor();
  await page.screenshot({ path: screenshotPath("search"), fullPage: true });
  await search.press("Enter");
  await waitForReady(page, "named locus");
  assert.match(await search.inputValue(), /PNGRTEST/);
  const pattern = page.locator("[data-pattern-id]").first();
  if ((await pattern.count()) > 0) {
    await pattern.focus();
    await pattern.press("Enter");
    await page
      .getByRole("complementary", { name: "Pattern inspector" })
      .getByText(/not a named person/i)
      .waitFor();
    await page.getByRole("button", { name: "Close inspector" }).click();
  }

  const requestCountBeforeLocal = requests.length;
  await page.getByRole("button", { name: "Archive" }).click();
  await page.locator('input[type="file"]').setInputFiles(fixturePath);
  await waitForReady(page, "local file");
  assert.equal(requests.length, requestCountBeforeLocal);

  await page.goto(`${baseUrl}/tube-map-lab`);
  await page.getByText("Golden tube-map laboratory").waitFor();
  await page.locator('[data-node-key="c:102,103,104,105,106,107"]').waitFor();
  await page.locator('[data-pattern-id="T2-P1"]').waitFor();
  await page.locator('[data-node-key="n:301"]').waitFor();
  await page.screenshot({ path: screenshotPath("golden"), fullPage: true });

  const browserMatrix = ["chromium"];
  for (const [name, engine] of [
    ["firefox", firefox],
    ["webkit", webkit],
  ]) {
    const matrixBrowser = await engine.launch({ headless: true });
    try {
      const matrixPage = await matrixBrowser.newPage({
        viewport: { width: 1280, height: 900 },
      });
      const errors = [];
      matrixPage.on("pageerror", (error) => errors.push(error.message));
      await matrixPage.goto(
        `${baseUrl}/demo?archive=fixture&sample=GRCh38&contig=chr1&start=100&end=102`,
      );
      await waitForReady(matrixPage, `${name} fixture browser`);
      await matrixPage
        .getByRole("img", { name: /Tube map for GRCh38 chr1/ })
        .waitFor();
      assert.deepEqual(errors, [], `${name} reported page errors`);
      browserMatrix.push(name);
    } finally {
      await matrixBrowser.close();
    }
  }

  let configuredArchive;
  if (configuredArchiveUrl.length > 0) {
    configuredArchive = await exerciseConfiguredArchive(browser, baseUrl);
  }
  assert.deepEqual(pageErrors, []);
  console.log(
    JSON.stringify(
      {
        pagesBaseUrl: baseUrl,
        actualRangeResponses: requests.filter(
          (request) => request.status === 206,
        ).length,
        browserHistoryPassed: true,
        localFilePassed: true,
        nodeInspectorPassed: true,
        patternInspectorPassed: true,
        browserMatrix,
        configuredArchive,
        fixtureScreenshot: screenshotPath("fixture"),
        goldenScreenshot: screenshotPath("golden"),
        searchScreenshot: screenshotPath("search"),
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

async function exerciseConfiguredArchive(browser, baseUrl) {
  const page = await browser.newPage({
    viewport: { width: 1600, height: 1000 },
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(`${baseUrl}/demo`);
    await waitForReady(page, "configured HLA-B", 45_000);
    await page
      .getByRole("textbox", { name: "Search gene or coordinate" })
      .waitFor();
    assert.match(
      await page.locator(".reference-track__heading").innerText(),
      /GRCh38.*chr6/,
    );
    const counts = await page.evaluate(() => ({
      nodes: document.querySelectorAll("[data-node-key]").length,
      edges: document.querySelectorAll("[data-edge-key]").length,
      patterns: document.querySelectorAll("[data-pattern-id]").length,
      svgElements: document.querySelectorAll(".pngr-tube-map-svg *").length,
      status: document.querySelector(".browser-status")?.textContent?.trim(),
      metrics: { ...document.querySelector(".browser-status")?.dataset },
    }));
    assert(counts.nodes > 0 && counts.nodes <= 400);
    assert(counts.edges <= 800);
    assert.equal(counts.patterns, 8);
    assert.equal(await visibleLabelCollisionCount(page), 0);
    await page.screenshot({ path: screenshotPath("hla-b"), fullPage: true });
    const interactions = await exerciseGraphControls(page);
    const search = page.getByRole("textbox", {
      name: "Search gene or coordinate",
    });
    await search.fill("MICB");
    await page
      .getByRole("option", { name: /^MICB / })
      .waitFor({ timeout: 15_000 });
    await search.press("Enter");
    await waitForReady(page, "configured MICB", 45_000);
    const micb = await page.evaluate(() => ({
      nodes: document.querySelectorAll("[data-node-key]").length,
      edges: document.querySelectorAll("[data-edge-key]").length,
      patterns: document.querySelectorAll("[data-pattern-id]").length,
      svgElements: document.querySelectorAll(".pngr-tube-map-svg *").length,
      status: document.querySelector(".browser-status")?.textContent?.trim(),
      metrics: { ...document.querySelector(".browser-status")?.dataset },
    }));
    await page.screenshot({ path: screenshotPath("micb"), fullPage: true });
    assert.deepEqual(errors, []);
    return {
      hla: { ...counts, ...interactions },
      hlaScreenshot: screenshotPath("hla-b"),
      micb,
      micbScreenshot: screenshotPath("micb"),
    };
  } finally {
    await page.close();
  }
}

async function exerciseGraphControls(page) {
  await page.getByRole("button", { name: "Fit" }).click();
  const graph = page.locator(".tube-map-view");
  const reference = page.locator(".pngr-node--reference").first();
  const graphBox = await graph.boundingBox();
  assert(graphBox !== null, "graph viewport has no bounding box");
  const before = await reference.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return { center: rect.left + rect.width / 2, width: rect.width };
  });
  await graph.dispatchEvent("wheel", {
    clientX: before.center,
    clientY: graphBox.y + graphBox.height / 2,
    deltaMode: 0,
    deltaX: 0,
    deltaY: -80,
  });
  await page.waitForTimeout(100);
  const after = await reference.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return { center: rect.left + rect.width / 2, width: rect.width };
  });
  const wheelZoomRatio = after.width / before.width;
  const wheelAnchorShift = Math.abs(after.center - before.center);
  assert(
    wheelZoomRatio > 1.04 && wheelZoomRatio < 1.08,
    `one wheel step changed zoom by ${wheelZoomRatio.toFixed(3)}x`,
  );
  assert(
    wheelAnchorShift < 0.75,
    `cursor-anchored wheel zoom shifted ${wheelAnchorShift.toFixed(2)} px`,
  );
  await page.getByRole("button", { name: "Fit" }).click();

  let singleClickPreservedCollapse = true;
  let expandedNodeCount;
  const collapsed = page.locator('[data-node-key^="c:"]').first();
  if ((await collapsed.count()) > 0) {
    const nodeCount = await page.locator("[data-node-key]").count();
    await collapsed.click();
    const inspector = page.getByRole("complementary", {
      name: "Node inspector",
    });
    await inspector.waitFor();
    singleClickPreservedCollapse =
      (await page.locator("[data-node-key]").count()) === nodeCount;
    assert(singleClickPreservedCollapse);
    assert.equal(await inspector.locator("details").count(), 0);
    await inspector
      .getByRole("button", { name: "Expand chain in graph" })
      .click();
    await page.waitForFunction(
      (beforeCount) =>
        document.querySelectorAll("[data-node-key]").length > beforeCount,
      nodeCount,
    );
    expandedNodeCount = await page.locator("[data-node-key]").count();
  }
  return {
    wheelZoomRatio,
    wheelAnchorShift,
    singleClickPreservedCollapse,
    expandedNodeCount,
  };
}

async function visibleLabelCollisionCount(page) {
  return page.evaluate(() => {
    const rectangles = [
      ...document.querySelectorAll(".pngr-pattern-label, .pngr-node-label"),
    ]
      .map((element) => element.getBoundingClientRect())
      .filter((rect) => rect.width > 0 && rect.height > 0);
    let collisions = 0;
    for (let left = 0; left < rectangles.length; left += 1) {
      for (let right = left + 1; right < rectangles.length; right += 1) {
        const first = rectangles[left];
        const second = rectangles[right];
        const overlapX =
          Math.min(first.right, second.right) -
          Math.max(first.left, second.left);
        const overlapY =
          Math.min(first.bottom, second.bottom) -
          Math.max(first.top, second.top);
        if (overlapX > 1 && overlapY > 1) collisions += 1;
      }
    }
    return collisions;
  });
}

async function assertHealthyBrowser(page) {
  await page.getByRole("img", { name: /Tube map for GRCh38 chr1/ }).waitFor();
  assert.equal(await page.locator(".pangenome-browser").count(), 1);
  assert.equal(
    await page.locator(".showcase-stage, .overview-stage, .tool-rail").count(),
    0,
  );
  assert.equal(await page.getByRole("button", { name: "Archive" }).count(), 1);
  assert.equal(await page.getByRole("button", { name: "Share" }).count(), 1);
  assert.match(
    await page.locator(".browser-status").innerText(),
    /SHA-256 verified|integrity/i,
  );
}

async function waitForReady(page, label, timeout = 15_000) {
  try {
    await page
      .locator('.browser-status[data-phase="ready"]')
      .waitFor({ timeout });
  } catch (error) {
    const phase = await page
      .locator(".browser-status")
      .getAttribute("data-phase");
    const message = await page
      .locator(".browser-status")
      .innerText()
      .catch(() => "");
    throw new Error(
      `${label} did not reach ready (phase=${phase}, message=${JSON.stringify(message)}): ${error instanceof Error ? error.message : error}`,
    );
  }
}

async function assertNoPageOverflow(page, label) {
  const dimensions = await page.evaluate(() => ({
    viewportWidth: innerWidth,
    viewportHeight: innerHeight,
    pageWidth: document.documentElement.scrollWidth,
    pageHeight: document.documentElement.scrollHeight,
  }));
  assert(
    dimensions.pageWidth <= dimensions.viewportWidth,
    `${label} has horizontal overflow`,
  );
  assert(
    dimensions.pageHeight <= dimensions.viewportHeight,
    `${label} has vertical overflow`,
  );
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
