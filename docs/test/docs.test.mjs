import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const componentUrl = new URL(
  "../components/PangenomeDemo.vue",
  import.meta.url,
);
const evidenceComponentUrl = new URL(
  "../components/explorer/EvidenceSheet.vue",
  import.meta.url,
);
const configUrl = new URL("../.vitepress/config.ts", import.meta.url);
const pagesWorkflowUrl = new URL(
  "../../.github/workflows/pages.yml",
  import.meta.url,
);

test("the demo uses only public package exports and a configurable archive", async () => {
  const source = await readFile(componentUrl, "utf8");
  assert.match(source, /from "pangenome-range\/reader"/);
  assert.match(source, /from "pangenome-range\/viewer"/);
  assert.match(source, /VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL/);
  assert.match(source, /VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL/);
  assert.match(
    source,
    /configuredArchiveUrl\.length > 0[\s\S]+\? "configured"[\s\S]+populationArchiveUrl\.length > 0[\s\S]+\? "population"/,
  );
  assert.match(source, /archiveChoice\.value === "fixture"/);
  assert.match(source, /archiveChoice\.value === "population"/);
  assert.match(source, /NA19239 haplotype-0 paths/);
  assert.match(source, /not GRCh38/);
  assert.doesNotMatch(source, /packages\/browser\/src/);
  assert.match(source, /opened\.searchLoci/);
  assert.match(source, /opened\.summary/);
  assert.match(source, /chooseViewerLod/);
  assert.match(source, /remain local to each source tile/);
  assert.match(source, /not people, alleles, frequencies/);
  assert.match(source, /pangenome-explorer-active/);
  assert.match(source, /maxRenderedNodes: 8_000/);
  assert.match(source, /maxRenderedEdges: 12_000/);
  assert.match(
    source,
    /phase\.value === "graph" \|\| phase\.value === "ready"/,
  );
});

test("the bottom evidence bar reports progressive graph loading", async () => {
  const source = await readFile(evidenceComponentUrl, "utf8");
  assert.match(source, /props\.phase === "graph"/);
  assert.match(
    source,
    /\$\{completedTiles\}\/\$\{props\.expectedTiles\} tiles/,
  );
  assert.match(source, /counts\.renderedNodes\.toLocaleString\(\)/);
  assert.match(source, /data-loading="props\.phase === 'graph'"/);
});

test("the VitePress base path matches the repository Pages path", async () => {
  const source = await readFile(configUrl, "utf8");
  assert.match(source, /base: "\/pangenome-range\/"/);
});

test("Pages defaults to the content-addressed whole-genome demo archive", async () => {
  const source = await readFile(pagesWorkflowUrl, "utf8");
  assert.match(
    source,
    /sha256\/ecf5ae4fa8c784a80307507f58bed894311b8560724b57de0fcc35237c324b63\/hprc-v1-gencode-v50-disk-t8\.pngr/,
  );
  assert.match(source, /vars\.PANGENOME_RANGE_DEMO_ARCHIVE_URL/);
  assert.match(
    source,
    /sha256\/71730fab7aad0dbbef81cf7c74b4fa8dbacbb3aad5bab0a797349120b18f6afb\/1000gplons-hs38d1-na19239-h0-v1-t8-zstd3\.pngr/,
  );
  assert.match(source, /vars\.PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL/);
});
