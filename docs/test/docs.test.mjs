import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import test from "node:test";
import {
  DEFAULT_DEMO_1000G_ARCHIVE_URL,
  DEFAULT_DEMO_ARCHIVE_URL,
  DEFAULT_DEMO_CHICKEN_ARCHIVE_URL,
  DEFAULT_DEMO_RICE_ARCHIVE_URL,
  docsDevEnvironment,
} from "../../scripts/docs-dev.mjs";

const browserUrl = new URL(
  "../components/browser/PangenomeBrowser.vue",
  import.meta.url,
);
const shellUrl = new URL("../components/PangenomeDemo.vue", import.meta.url);
const styleUrl = new URL("../components/browser/browser.css", import.meta.url);
const configUrl = new URL("../.vitepress/config.ts", import.meta.url);
const pagesWorkflowUrl = new URL(
  "../../.github/workflows/pages.yml",
  import.meta.url,
);
const readmeUrl = new URL("../../README.md", import.meta.url);
const packageUrl = new URL("../../package.json", import.meta.url);
const animationUrl = new URL(
  "../components/RangeReadAnimation.vue",
  import.meta.url,
);
const walkthroughUrl = new URL("../how-range-reads-work.md", import.meta.url);

test("the demo is a single browser shell using public reader and viewer exports", async () => {
  const [shell, browser] = await Promise.all([
    readFile(shellUrl, "utf8"),
    readFile(browserUrl, "utf8"),
  ]);
  assert.match(shell, /PangenomeBrowser/);
  assert.doesNotMatch(
    shell,
    /ShowcaseStage|OverviewStage|EvidenceSheet|ToolRail/,
  );
  assert.match(browser, /from "pangenome-range\/reader"/);
  assert.match(browser, /from "pangenome-range\/viewer"/);
  assert.match(browser, /VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL/);
  assert.match(browser, /VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL/);
  assert.match(browser, /VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL/);
  assert.match(browser, /VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL/);
  assert.doesNotMatch(browser, /VITE_PANGENOME_RANGE_DEMO_POPLAR_ARCHIVE_URL/);
  assert.match(browser, /archive=hprc|"hprc"/);
  assert.match(browser, /"1000g"/);
  assert.match(browser, /"rice"/);
  assert.match(browser, /"chicken"/);
  assert.doesNotMatch(browser, /"poplar"/);
  assert.match(
    browser,
    /Chicken pangenome, 30 assemblies \(whole reference genome\)/,
  );
  assert.match(browser, /namedPathHintVisible/);
  assert.match(browser, /openPublishedExample/);
  assert.match(
    browser,
    /URLSearchParams\(window\.location\.search\)[\s\S]*?\.get\("preset"\)/,
  );
  assert.match(
    browser,
    /activeArchiveSha256\.value !== publishedPreset\?\.archiveSha256/,
  );
  assert.match(browser, /viewportFromUrl/);
  assert.match(browser, /opened\.searchLoci/);
  assert.match(browser, /opened\.planRegion/);
  assert.match(browser, /opened\.summary/);
  assert.match(browser, /opened\.queryTiles/);
  assert.match(browser, /decideGraphRegion/);
  assert.match(browser, /configuredDefaultLocus[\s\S]+"HLA-B"/);
  assert.doesNotMatch(browser, /packages\/browser\/src/);
});

test("the desktop shell owns the viewport with the specified four rows", async () => {
  const source = await readFile(styleUrl, "utf8");
  assert.match(source, /position: fixed;[\s\S]+inset: 0;/);
  assert.match(source, /grid-template-rows: 52px 112px minmax\(0, 1fr\) 30px;/);
  assert.match(source, /overflow: hidden;/);
});

test("the VitePress base path matches the repository Pages path", async () => {
  const source = await readFile(configUrl, "utf8");
  assert.match(source, /base: "\/pangenome-range\/"/);
  assert.doesNotMatch(source, /tube-map-lab/);
});

test("range-read animation teaches the three query rounds", async () => {
  const [animation, walkthrough, config] = await Promise.all([
    readFile(animationUrl, "utf8"),
    readFile(walkthroughUrl, "utf8"),
    readFile(configUrl, "utf8"),
  ]);
  assert.match(animation, /PNGRNG01/);
  assert.match(animation, /16 KiB bootstrap/);
  assert.match(animation, /4 KiB/);
  assert.match(animation, /bucket_span/);
  assert.match(animation, /__pngrRangeReadAnimation/);
  assert.match(animation, /Round 3 · payloads/);
  assert.match(walkthrough, /RangeReadAnimation/);
  assert.match(walkthrough, /page_offset = first_page_offset/);
  assert.match(config, /how-range-reads-work/);
  const gif = new URL("../public/format-range-read.gif", import.meta.url);
  const info = await stat(gif);
  assert.ok(info.size > 10_000, "README GIF should be a non-empty animation");
});

test("Pages defaults to the content-addressed whole-genome demo archive", async () => {
  const source = await readFile(pagesWorkflowUrl, "utf8");
  assert.match(
    source,
    /sha256\/82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a\/hprc-v2\.1-gencode-v50-named-membership-82585cb612effbf4\.pngr/,
  );
  assert.match(source, /vars\.PANGENOME_RANGE_DEMO_ARCHIVE_URL/);
  assert.match(
    source,
    /sha256\/71730fab7aad0dbbef81cf7c74b4fa8dbacbb3aad5bab0a797349120b18f6afb\/1000gplons-hs38d1-na19239-h0-v1-t8-zstd3\.pngr/,
  );
  assert.match(source, /vars\.PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL/);
  assert.match(
    source,
    /sha256\/c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c\/rice-chr06-mc-xa7-anonymous\.pngr/,
  );
  assert.match(source, /vars\.PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL/);
  assert.match(
    source,
    /sha256\/93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e\/chicken-whole-named\.pngr/,
  );
  assert.match(source, /vars\.PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL/);
  assert.doesNotMatch(source, /PANGENOME_RANGE_DEMO_POPLAR_ARCHIVE_URL/);
});

test("README leads with the four project parts and useful demo links", async () => {
  const source = await readFile(readmeUrl, "utf8");
  assert.match(source, /## What is in this repository/);
  assert.match(source, /\*\*The `\.pngr` file format\*\*/);
  assert.match(source, /\*\*A Rust encoder\*\*/);
  assert.match(source, /\*\*A TypeScript reader\*\*/);
  assert.match(source, /\*\*A browser viewer\*\*/);
  assert.match(source, /## Why I built this/);
  assert.match(source, /## How a range query works/);
  assert.match(source, /docs\/public\/format-range-read\.gif/);
  assert.match(source, /PNGRNG01/);
  assert.match(source, /https:\/\/github\.com\/a-r-d\/PureJsImage/);
  assert.match(source, /https:\/\/purejsimage\.com\//);
  assert.match(source, /## Live demos/);
  assert.match(source, /archive=chicken&preset=chicken-igll1-ucd312-deletion/);
  assert.match(source, /archive=hprc&locus=HLA-B/);
  assert.match(source, /archive=hprc&locus=KIR3DL1/);
  assert.match(source, /archive=1000g&sample=NA19239&contig=1/);
  assert.match(source, /archive=rice&locus=Xa7/);
  assert.match(source, /A published deletion and the named source paths/);
  assert.doesNotMatch(source, /archive=poplar/);
  assert.match(source, /zoom=[^&]+&center=[^&]+&vscale=/);
  assert.match(source, /Local files still require you to choose the file/);

  const demos = source.indexOf("## Live demos");
  const rawPathId = source.indexOf("UCD312#2#h2tg000050l#fragment=1251366");
  assert.ok(demos >= 0);
  assert.ok(
    rawPathId > demos,
    "the raw GBWT path ID should follow the demo table",
  );
});

test("npm docs:dev injects all public archives while preserving overrides", async () => {
  const packageJson = JSON.parse(await readFile(packageUrl, "utf8"));
  assert.equal(packageJson.scripts["docs:dev"], "node scripts/docs-dev.mjs");
  const defaults = docsDevEnvironment({ PATH: "/example" });
  assert.equal(
    defaults.VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL,
    DEFAULT_DEMO_ARCHIVE_URL,
  );
  assert.equal(
    defaults.VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL,
    DEFAULT_DEMO_1000G_ARCHIVE_URL,
  );
  assert.equal(
    defaults.VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL,
    DEFAULT_DEMO_RICE_ARCHIVE_URL,
  );
  assert.equal(
    defaults.VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL,
    DEFAULT_DEMO_CHICKEN_ARCHIVE_URL,
  );
  const overridden = docsDevEnvironment({
    VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL: "https://example.test/custom.pngr",
    VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL:
      "https://example.test/custom-1000g.pngr",
    VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL:
      "https://example.test/custom-rice.pngr",
    VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL:
      "https://example.test/custom-chicken.pngr",
  });
  assert.equal(
    overridden.VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL,
    "https://example.test/custom.pngr",
  );
  assert.equal(
    overridden.VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL,
    "https://example.test/custom-1000g.pngr",
  );
  assert.equal(
    overridden.VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL,
    "https://example.test/custom-rice.pngr",
  );
  assert.equal(
    overridden.VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL,
    "https://example.test/custom-chicken.pngr",
  );
});
