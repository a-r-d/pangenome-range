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
const namedFixturePath = join(
  repository,
  "test-data",
  "golden",
  "path-membership-v1.pngr",
);
const fixture = await readFile(fixturePath);
const indexedFixture = await readFile(indexedFixturePath);
const namedFixture = await readFile(namedFixturePath);
const etag = `"sha256-${createHash("sha256").update(fixture).digest("hex")}"`;
const requests = [];
const configuredArchiveUrl =
  process.env.VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL ?? "";
const populationArchiveUrl =
  process.env.VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL ?? "";
const riceArchiveUrl =
  process.env.VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL ?? "";
const chickenArchiveUrl =
  process.env.VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL ?? "";
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
  if (url.pathname.endsWith("/named.pngr")) {
    serveArchive(request, response, namedFixture, false);
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
  await waitForViewportUrl(page, "first coordinate");
  const firstHistoryUrl = page.url();
  await search.fill("GRCh38 chr1:101-102");
  await search.press("Enter");
  await waitForReady(page, "second coordinate");
  await waitForViewportUrl(page, "second coordinate");
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
  assert.equal(
    await page
      .getByRole("button", { name: "Decrease vertical spacing" })
      .count(),
    1,
  );
  assert.equal(
    await page
      .getByRole("button", { name: "Increase vertical spacing" })
      .count(),
    1,
  );
  const verticalBorder = await page
    .locator(".toolbar-vertical")
    .evaluate((group) => {
      const groupRect = group.getBoundingClientRect();
      const buttonRect = group.querySelector("button").getBoundingClientRect();
      const style = getComputedStyle(group);
      return {
        top: style.borderTopWidth,
        bottom: style.borderBottomWidth,
        topInset: buttonRect.top - groupRect.top,
        bottomInset: groupRect.bottom - buttonRect.bottom,
      };
    });
  assert.deepEqual(verticalBorder, {
    top: "1px",
    bottom: "1px",
    topInset: 1,
    bottomInset: 1,
  });
  await page.getByText("Options", { exact: true }).click();
  assert.equal(await page.locator(".help-tooltip").count(), 3);
  await page.locator(".help-tooltip").first().focus();
  assert.equal(
    await page.locator(".help-tooltip__content").first().isVisible(),
    true,
  );
  await page.getByText("Options", { exact: true }).click();
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

  await page.getByRole("button", { name: "Share" }).click();
  const shareDialog = page.getByRole("dialog", {
    name: "Copy exact region link",
  });
  await shareDialog.waitFor();
  assert.equal(
    await shareDialog.getByLabel("Shareable region link").inputValue(),
    page.url(),
  );
  assert.equal(
    await shareDialog.getByRole("button", { name: "Copy link" }).count(),
    1,
  );
  await shareDialog.getByRole("button", { name: "Close share dialog" }).click();

  await page.getByRole("button", { name: "Source" }).click();
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
    const requestsBeforeInspector = requests.length;
    const patternId = await pattern.getAttribute("data-pattern-id");
    await pattern.focus();
    await pattern.press("Enter");
    await page
      .getByRole("complementary", { name: "Pattern inspector" })
      .getByText(/anonymous tile-local evidence only/i)
      .waitFor();
    assert.equal(
      requests.length,
      requestsBeforeInspector,
      "anonymous inspector unexpectedly requested named membership",
    );
    assert.equal(
      await page
        .locator(".pngr-pattern-port.is-selected")
        .getAttribute("data-pattern-port-id"),
      patternId,
    );
    assert.equal(
      await page.locator(".pngr-pattern-port.is-muted").count(),
      Math.max(0, (await page.locator("[data-pattern-id]").count()) - 1),
    );
    await page.getByRole("button", { name: "Close inspector" }).click();
  }

  await page.getByRole("button", { name: "Source" }).click();
  await page
    .getByPlaceholder("https://…/archive.pngr")
    .fill(`${baseUrl}/named.pngr`);
  await page.getByRole("button", { name: "Open remote URL" }).click();
  await waitForReady(page, "named-membership archive");
  const namedPattern = page.locator("[data-pattern-id]").first();
  await namedPattern.focus();
  await namedPattern.press("Enter");
  const namedInspector = page.getByRole("complementary", {
    name: "Pattern inspector",
  });
  await namedInspector
    .getByRole("heading", { name: "Named source paths" })
    .waitFor();
  await namedInspector.getByText("Unique named paths").waitFor();
  const firstNamedRecord = namedInspector.locator("li").first();
  await firstNamedRecord.waitFor();
  const firstSample = (
    await firstNamedRecord.locator("strong").textContent()
  )?.trim();
  assert(firstSample, "named path record did not resolve a sample");
  const pathFilter = namedInspector.getByRole("searchbox", {
    name: "Filter named source paths",
  });
  await pathFilter.fill(firstSample);
  assert((await namedInspector.locator("li").count()) > 0);
  await pathFilter.fill("no-such-source-path");
  assert.equal(await namedInspector.locator("li").count(), 0);
  await pathFilter.fill("");
  await firstNamedRecord
    .getByRole("button", { name: "Highlight path" })
    .click();
  await page.getByRole("button", { name: "Close inspector" }).click();
  await page.locator(".pngr-pattern-port.is-selected").first().waitFor();

  const requestCountBeforeLocal = requests.length;
  await page.getByRole("button", { name: "Source" }).click();
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
  let populationArchive;
  if (populationArchiveUrl.length > 0) {
    populationArchive = await exercisePopulationArchive(browser, baseUrl);
  }
  let riceArchive;
  if (riceArchiveUrl.length > 0) {
    riceArchive = await exerciseRiceArchive(browser, baseUrl);
  }
  let chickenArchive;
  if (chickenArchiveUrl.length > 0) {
    chickenArchive = await exerciseChickenArchive(browser, baseUrl);
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
        populationArchive,
        riceArchive,
        chickenArchive,
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
    await page.goto(
      `${baseUrl}/demo?archive=hprc&locus=HLA-B&zoom=0.32&center=0.5&vscale=1.3`,
    );
    await waitForReady(page, "configured HLA-B", 45_000);
    const search = page.getByRole("textbox", {
      name: "Search gene or coordinate",
    });
    await search.waitFor();
    assert.match(await search.inputValue(), /HLA-B/);
    await assertViewportState(page, { zoom: 0.32, center: 0.5, vscale: 1.3 });
    await page.getByRole("button", { name: "Source" }).click();
    const sourceDialog = page.getByRole("dialog", { name: "Archive source" });
    assert.equal(
      await sourceDialog.getByLabel("Demo archive").inputValue(),
      "hprc",
    );
    assert.equal(
      await sourceDialog.getByRole("option", { name: /1000 Genomes/ }).count(),
      populationArchiveUrl.length > 0 ? 1 : 0,
    );
    assert.equal(
      await sourceDialog.getByRole("option", { name: /PPanG rice/ }).count(),
      riceArchiveUrl.length > 0 ? 1 : 0,
    );
    await sourceDialog
      .getByRole("button", { name: "Close archive source" })
      .click();
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
    assert.equal(
      await page.locator("[data-pattern-port-id]").count(),
      counts.patterns,
    );
    assert.equal(await patternPortMismatchCount(page), 0);
    assert.equal(await visibleLabelCollisionCount(page), 0);
    await page.screenshot({ path: screenshotPath("hla-b"), fullPage: true });
    const interactions = await exerciseGraphControls(page);
    const linkedViewport = new URL(page.url()).searchParams;
    assert(linkedViewport.has("zoom"));
    assert(linkedViewport.has("center"));
    assert(linkedViewport.has("vscale"));
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
    await search.fill("CHAD");
    await page
      .getByRole("option", { name: /^CHAD / })
      .waitFor({ timeout: 15_000 });
    await search.press("Enter");
    await waitForReady(page, "configured CHAD", 45_000);
    await page.getByLabel("Local-pattern count").selectOption("16");
    const chad = await page.evaluate(() => ({
      nodes: document.querySelectorAll("[data-node-key]").length,
      alternateNodes: document.querySelectorAll(".pngr-node--alternate").length,
      alternateLabels: document.querySelectorAll(
        ".pngr-node--alternate .pngr-node-label",
      ).length,
      abbreviatedLabels: document.querySelectorAll(
        '.pngr-node-label[data-label-mode="abbreviated"]',
      ).length,
      minimumLabelFontSize: Math.min(
        ...[...document.querySelectorAll(".pngr-node-label")].map((label) =>
          Number(label.getAttribute("font-size")),
        ),
      ),
      patterns: document.querySelectorAll("[data-pattern-id]").length,
      svgElements: document.querySelectorAll(".pngr-tube-map-svg *").length,
    }));
    assert(chad.alternateNodes > 0);
    assert(chad.alternateLabels > 0);
    assert(chad.abbreviatedLabels > 0);
    assert.equal(chad.patterns, 16);
    assert.equal(await visibleLabelCollisionCount(page), 0);
    assert.equal(await referenceLabelOverlapCount(page), 0);
    await page.screenshot({ path: screenshotPath("chad"), fullPage: true });

    await page.getByText("Options", { exact: true }).click();
    await page.getByLabel("Simplify linear chains").uncheck();
    await search.fill("CRISP1");
    await page
      .getByRole("option", { name: /^CRISP1 / })
      .waitFor({ timeout: 15_000 });
    await search.press("Enter");
    await waitForReady(page, "configured CRISP1", 45_000);
    const crisp = await page.evaluate(() => ({
      nodes: document.querySelectorAll("[data-node-key]").length,
      edges: document.querySelectorAll("[data-edge-key]").length,
      svgElements: document.querySelectorAll(".pngr-tube-map-svg *").length,
      metrics: { ...document.querySelector(".browser-status")?.dataset },
    }));
    assert(crisp.nodes > 400 && crisp.nodes <= 2_500);
    assert(crisp.edges > 800 && crisp.edges <= 5_000);
    assert.equal(await page.locator(".graph-state--oversized").count(), 0);
    await page.screenshot({ path: screenshotPath("crisp1"), fullPage: true });

    await search.fill("GRCh38#chr6:49,815,000-49,895,000");
    await search.press("Enter");
    await waitForReady(page, "configured dense CRISP1", 45_000);
    const warning = page.locator(".graph-state--oversized");
    await warning.waitFor();
    assert.match(await warning.innerText(), /graph data is intact/i);
    await page.getByRole("button", { name: "Open anyway" }).click();
    await page.waitForFunction(
      () =>
        document.querySelector(".graph-state--oversized") === null &&
        document.querySelectorAll("[data-node-key]").length > 2_500,
      undefined,
      { timeout: 30_000 },
    );
    const crispOpenAnyway = await page.evaluate(() => ({
      nodes: document.querySelectorAll("[data-node-key]").length,
      edges: document.querySelectorAll("[data-edge-key]").length,
      svgElements: document.querySelectorAll(".pngr-tube-map-svg *").length,
      metrics: { ...document.querySelector(".browser-status")?.dataset },
    }));
    assert(crispOpenAnyway.nodes <= 10_000);
    assert(crispOpenAnyway.edges <= 20_000);
    await page.screenshot({
      path: screenshotPath("crisp1-open-anyway"),
      fullPage: true,
    });
    assert.deepEqual(errors, []);
    return {
      hla: { ...counts, ...interactions },
      hlaScreenshot: screenshotPath("hla-b"),
      micb,
      micbScreenshot: screenshotPath("micb"),
      chad,
      chadScreenshot: screenshotPath("chad"),
      crisp,
      crispScreenshot: screenshotPath("crisp1"),
      crispOpenAnyway,
      crispOpenAnywayScreenshot: screenshotPath("crisp1-open-anyway"),
    };
  } finally {
    await page.close();
  }
}

async function exercisePopulationArchive(browser, baseUrl) {
  const page = await browser.newPage({
    viewport: { width: 1600, height: 1000 },
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(
      `${baseUrl}/demo?archive=1000g&sample=NA19239&contig=1&start=0&end=32768&zoom=0.35&center=0.5&vscale=1`,
    );
    await waitForReady(page, "1000 Genomes coordinate view", 45_000);
    assert.match(
      await page
        .getByRole("textbox", { name: "Search gene or coordinate" })
        .inputValue(),
      /NA19239.*1/,
    );
    await assertViewportState(page, { zoom: 0.35, center: 0.5, vscale: 1 });
    await page.getByRole("button", { name: "Source" }).click();
    const sourceDialog = page.getByRole("dialog", { name: "Archive source" });
    assert.equal(
      await sourceDialog.getByLabel("Demo archive").inputValue(),
      "1000g",
    );
    assert.match(
      await sourceDialog.innerText(),
      /population-path coordinates/i,
    );
    await page.screenshot({
      path: screenshotPath("1000g"),
      fullPage: true,
    });
    assert.deepEqual(errors, []);
    return {
      source: "1000g",
      nodes: await page.locator("[data-node-key]").count(),
      screenshot: screenshotPath("1000g"),
    };
  } finally {
    await page.close();
  }
}

async function exerciseRiceArchive(browser, baseUrl) {
  const page = await browser.newPage({
    viewport: { width: 1600, height: 1000 },
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(
      `${baseUrl}/demo?archive=rice&locus=Xa7&zoom=2.856&center=0.600576&vscale=1.45`,
    );
    await waitForReady(page, "PPanG rice Xa7 view", 45_000);
    assert.equal(
      await page
        .getByRole("textbox", { name: "Search gene or coordinate" })
        .inputValue(),
      "Xa7",
    );
    await assertViewportState(page, {
      zoom: 2.856,
      center: 0.600576,
      vscale: 1.45,
    });
    assert.match(
      await page.locator(".reference-track__heading").innerText(),
      /NATELBORO.*chr06/,
    );
    await page.getByRole("button", { name: "Source" }).click();
    const sourceDialog = page.getByRole("dialog", { name: "Archive source" });
    assert.equal(
      await sourceDialog.getByLabel("Demo archive").inputValue(),
      "rice",
    );
    assert.match(
      await sourceDialog.innerText(),
      /anonymous weighted tile-local/i,
    );
    await page.screenshot({
      path: screenshotPath("rice-xa7"),
      fullPage: true,
    });
    assert.deepEqual(errors, []);
    return {
      source: "rice",
      locus: "Xa7",
      nodes: await page.locator("[data-node-key]").count(),
      screenshot: screenshotPath("rice-xa7"),
    };
  } finally {
    await page.close();
  }
}

async function exerciseChickenArchive(browser, baseUrl) {
  const page = await browser.newPage({
    viewport: { width: 1600, height: 1000 },
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto(
      `${baseUrl}/demo?archive=chicken&locus=IGLL1&zoom=0.35&center=0.5&vscale=1.2`,
    );
    await waitForReady(page, "chicken IGLL1 view", 45_000);
    assert.equal(
      await page
        .getByRole("textbox", { name: "Search gene or coordinate" })
        .inputValue(),
      "IGLL1",
    );
    await assertViewportState(page, {
      zoom: 0.35,
      center: 0.5,
      vscale: 1.2,
    });
    assert.match(
      await page.locator(".reference-track__heading").innerText(),
      /bGalGal1b.*chr15/,
    );
    assert((await page.locator("[data-node-key]").count()) > 0);
    await page.getByRole("button", { name: "Source" }).click();
    const sourceDialog = page.getByRole("dialog", { name: "Archive source" });
    assert.equal(
      await sourceDialog.getByLabel("Demo archive").inputValue(),
      "chicken",
    );
    assert.match(
      await sourceDialog.innerText(),
      /exact named GBWT source-path/i,
    );
    await sourceDialog
      .getByRole("button", { name: "Close archive source" })
      .click();
    const pattern = page.locator("[data-pattern-id]").first();
    await pattern.focus();
    await pattern.press("Enter");
    const inspector = page.getByRole("complementary", {
      name: "Pattern inspector",
    });
    await inspector
      .getByRole("heading", { name: "Named source paths" })
      .waitFor();
    await inspector.getByText("Unique named paths").waitFor();
    assert((await inspector.locator("li").count()) > 0);
    assert.match(
      await inspector.locator("li").first().innerText(),
      /multiplicity/,
    );
    await page.screenshot({
      path: screenshotPath("chicken-igll1"),
      fullPage: true,
    });
    assert.deepEqual(errors, []);
    return {
      source: "chicken",
      locus: "IGLL1",
      nodes: await page.locator("[data-node-key]").count(),
      screenshot: screenshotPath("chicken-igll1"),
    };
  } finally {
    await page.close();
  }
}

async function assertViewportState(page, expected) {
  const graph = page.locator(".tube-map-view");
  const actual = {
    zoom: Number(await graph.getAttribute("data-zoom")),
    center: Number(await graph.getAttribute("data-center")),
    vscale: Number(await graph.getAttribute("data-vertical-scale")),
  };
  assert(Math.abs(actual.zoom - expected.zoom) < 0.001, JSON.stringify(actual));
  assert(
    Math.abs(actual.center - expected.center) < 0.001,
    JSON.stringify(actual),
  );
  assert(
    Math.abs(actual.vscale - expected.vscale) < 0.001,
    JSON.stringify(actual),
  );
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

  const fittedVerticalSpread = await graphVerticalSpread(page);
  for (let step = 0; step < 3; step += 1)
    await page
      .getByRole("button", { name: "Decrease vertical spacing" })
      .click();
  await page.waitForFunction((fitted) => {
    const centers = [...document.querySelectorAll(".pngr-node")].map(
      (element) => {
        const rect = element.getBoundingClientRect();
        return rect.top + rect.height / 2;
      },
    );
    return Math.max(...centers) - Math.min(...centers) < fitted * 0.98;
  }, fittedVerticalSpread);
  const compactVerticalSpread = await graphVerticalSpread(page);
  await page.getByRole("button", { name: "Fit" }).click();
  await page.waitForFunction((fitted) => {
    const centers = [...document.querySelectorAll(".pngr-node")].map(
      (element) => {
        const rect = element.getBoundingClientRect();
        return rect.top + rect.height / 2;
      },
    );
    const spread = Math.max(...centers) - Math.min(...centers);
    return Math.abs(spread - fitted) < 0.75;
  }, fittedVerticalSpread);

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
    fittedVerticalSpread,
    compactVerticalSpread,
    singleClickPreservedCollapse,
    expandedNodeCount,
  };
}

async function graphVerticalSpread(page) {
  return page.locator("svg.pngr-tube-map-svg").evaluate((svg) => {
    const centers = [...svg.querySelectorAll(".pngr-node")].map((element) => {
      const rect = element.getBoundingClientRect();
      return rect.top + rect.height / 2;
    });
    return Math.max(...centers) - Math.min(...centers);
  });
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

async function referenceLabelOverlapCount(page) {
  return page.evaluate(() => {
    const locus = document
      .querySelector(".reference-ruler__locus")
      ?.getBoundingClientRect();
    if (locus === undefined) return 0;
    return [
      ...document.querySelectorAll(".reference-ruler__tick small"),
    ].filter((element) => {
      const label = element.getBoundingClientRect();
      const overlapX =
        Math.min(label.right, locus.right) - Math.max(label.left, locus.left);
      const overlapY =
        Math.min(label.bottom, locus.bottom) - Math.max(label.top, locus.top);
      return overlapX > 0.5 && overlapY > 0.5;
    }).length;
  });
}

async function patternPortMismatchCount(page) {
  return page.locator("svg.pngr-tube-map-svg").evaluate((svg) => {
    const numbers = (value) =>
      (value.match(/-?\d+(?:\.\d+)?/g) ?? []).map(Number);
    const nodes = [...svg.querySelectorAll(".pngr-node")].map((group) => {
      const [left, top] = numbers(group.getAttribute("transform") ?? "");
      const shape = group.querySelector(".pngr-node-shape");
      const path = shape?.getAttribute("d") ?? "";
      const reverse = path.match(/^M 7 0 H (-?\d+(?:\.\d+)?) V/);
      const forward = path.match(
        /^M 0 0 H -?\d+(?:\.\d+)? L (-?\d+(?:\.\d+)?) /,
      );
      const width = Number(reverse?.[1] ?? forward?.[1]);
      return { left, right: left + width, top, bottom: top + 34 };
    });
    let mismatches = 0;
    for (const path of svg.querySelectorAll("[data-pattern-port-id]")) {
      const values = numbers(path.getAttribute("d") ?? "");
      for (let index = 0; index + 3 < values.length; index += 4) {
        const x = values[index];
        const y = values[index + 1];
        const attached = nodes.some(
          (node) =>
            (Math.abs(node.left - x) < 0.11 ||
              Math.abs(node.right - x) < 0.11) &&
            y >= node.top &&
            y <= node.bottom,
        );
        if (!attached) mismatches += 1;
      }
    }
    return mismatches;
  });
}

async function assertHealthyBrowser(page) {
  await page.getByRole("img", { name: /Tube map for GRCh38 chr1/ }).waitFor();
  assert.equal(await page.locator(".pangenome-browser").count(), 1);
  assert.equal(
    await page.locator(".showcase-stage, .overview-stage, .tool-rail").count(),
    0,
  );
  assert.equal(await page.getByRole("button", { name: "Source" }).count(), 1);
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

async function waitForViewportUrl(page, label, timeout = 15_000) {
  try {
    await page.waitForFunction(
      () => {
        const parameters = new URL(location.href).searchParams;
        return ["zoom", "center", "vscale"].every((name) =>
          parameters.has(name),
        );
      },
      undefined,
      { timeout },
    );
  } catch (error) {
    throw new Error(
      `${label} did not publish its complete viewport URL (${page.url()}): ${error instanceof Error ? error.message : error}`,
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
