import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import { releaseTargets } from "./release-config.mjs";

const repositoryRoot = new URL("../../", import.meta.url);
const readJson = async (relativePath) =>
  JSON.parse(await readFile(new URL(relativePath, repositoryRoot), "utf8"));

const [
  workspaceManifest,
  mainManifest,
  cargoManifest,
  buildManifest,
  cliManifest,
  queryManifest,
  readerSource,
] = await Promise.all([
  readJson("package.json"),
  readJson("packages/browser/package.json"),
  readFile(new URL("Cargo.toml", repositoryRoot), "utf8"),
  readFile(
    new URL("crates/pangenome-range-build/Cargo.toml", repositoryRoot),
    "utf8",
  ),
  readFile(
    new URL("crates/pangenome-range-cli/Cargo.toml", repositoryRoot),
    "utf8",
  ),
  readFile(
    new URL("crates/pangenome-range-query/Cargo.toml", repositoryRoot),
    "utf8",
  ),
  readFile(
    new URL("packages/browser/src/reader/index.ts", repositoryRoot),
    "utf8",
  ),
]);

const cargoVersion = cargoManifest.match(
  /\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
)?.[1];
assert(cargoVersion, "Cargo workspace version was not found");
assert.equal(
  workspaceManifest.version,
  mainManifest.version,
  "workspace and main npm versions differ",
);
assert.equal(
  cargoVersion,
  mainManifest.version,
  "Cargo workspace and main npm versions differ",
);
assert.equal(
  readerSource.match(/PANGENOME_RANGE_API_VERSION\s*=\s*"([^"]+)"/)?.[1],
  mainManifest.version,
  "exported TypeScript API version differs from the npm package",
);
assert.match(
  cliManifest,
  /^version\.workspace\s*=\s*true$/m,
  "native CLI must inherit the Cargo workspace version",
);
for (const manifest of [buildManifest, cliManifest, queryManifest]) {
  for (const match of manifest.matchAll(
    /pangenome-range-[a-z-]+\s*=\s*\{\s*version\s*=\s*"([^"]+)"/g,
  )) {
    assert.equal(
      match[1],
      cargoVersion,
      "internal Cargo path dependency version differs from the workspace",
    );
  }
}

const expectedOptionalDependencies = Object.fromEntries(
  Object.values(releaseTargets)
    .map(({ packageName }) => [packageName, mainManifest.version])
    .sort(([left], [right]) => left.localeCompare(right)),
);
assert.deepEqual(
  mainManifest.optionalDependencies,
  expectedOptionalDependencies,
  "platform optional dependencies must be complete and exact-versioned",
);

console.log(
  `release versions synchronized at ${mainManifest.version} across npm, Cargo, and ${Object.keys(releaseTargets).length} platform packages`,
);
