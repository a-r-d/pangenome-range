import { defineConfig } from "tsup";

export default defineConfig({
  entry: {
    "reader/index": "src/reader/index.ts",
    "viewer/index": "src/viewer/index.ts",
    "node/index": "src/node/index.ts",
  },
  format: ["esm"],
  target: "es2022",
  platform: "neutral",
  dts: true,
  sourcemap: true,
  clean: true,
  splitting: false,
  treeshake: true,
  noExternal: ["fzstd", "@noble/hashes"],
});
