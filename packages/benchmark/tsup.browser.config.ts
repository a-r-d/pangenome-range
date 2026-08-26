import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/browser-runtime.ts"],
  format: ["esm"],
  target: "es2022",
  platform: "browser",
  outDir: "dist/browser",
  splitting: true,
  sourcemap: true,
  clean: false,
  dts: false,
  external: ["pangenome-range/reader"],
  noExternal: ["@bokuweb/zstd-wasm"],
});
