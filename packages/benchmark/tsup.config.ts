import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts", "src/cli.ts"],
  format: ["esm"],
  target: "node24",
  platform: "node",
  external: [
    "@bokuweb/zstd-wasm",
    "@playwright/test",
    "pangenome-range/node",
    "pangenome-range/reader",
  ],
  dts: true,
  sourcemap: true,
  clean: true,
});
