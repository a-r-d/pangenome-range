import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const componentUrl = new URL(
  "../components/PangenomeDemo.vue",
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
  assert.match(
    source,
    /configuredArchiveUrl\.length === 0 \? "fixture" : "configured"/,
  );
  assert.match(source, /choice === "fixture"/);
  assert.doesNotMatch(source, /packages\/browser\/src/);
  assert.match(source, /local evidence within each source tile/);
  assert.match(source, /not named people or globally/);
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
});
