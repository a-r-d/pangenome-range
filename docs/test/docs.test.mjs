import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const componentUrl = new URL(
  "../components/PangenomeDemo.vue",
  import.meta.url,
);
const configUrl = new URL("../.vitepress/config.ts", import.meta.url);

test("the demo uses only public package exports and a configurable archive", async () => {
  const source = await readFile(componentUrl, "utf8");
  assert.match(source, /from "pangenome-range\/reader"/);
  assert.match(source, /from "pangenome-range\/viewer"/);
  assert.match(source, /VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL/);
  assert.doesNotMatch(source, /packages\/browser\/src/);
  assert.match(source, /local evidence within each source tile/);
  assert.match(source, /not named people or globally/);
});

test("the VitePress base path matches the repository Pages path", async () => {
  const source = await readFile(configUrl, "utf8");
  assert.match(source, /base: "\/pangenome-range\/"/);
});
