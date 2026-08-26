import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

import {
  parseArguments,
  releaseTargets,
  requiredArgument,
} from "./release-config.mjs";

const argumentsMap = parseArguments(process.argv.slice(2));
const mainDirectory = path.resolve(requiredArgument(argumentsMap, "main-dir"));
const platformsDirectory = path.resolve(
  requiredArgument(argumentsMap, "platforms-dir"),
);
const outputDirectory = path.resolve(requiredArgument(argumentsMap, "out-dir"));
await mkdir(outputDirectory, { recursive: false });

const packageDirectories = [
  mainDirectory,
  ...Object.keys(releaseTargets).map((targetName) =>
    path.join(platformsDirectory, targetName),
  ),
];
const packages = [];
for (const directory of packageDirectories) {
  const result = spawnSync(
    "npm",
    [
      "pack",
      directory,
      "--json",
      "--ignore-scripts",
      "--pack-destination",
      outputDirectory,
    ],
    { encoding: "utf8" },
  );
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const [packed] = JSON.parse(result.stdout);
  const tarballPath = path.join(outputDirectory, packed.filename);
  const tarballBytes = await readFile(tarballPath);
  packages.push({
    name: packed.name,
    version: packed.version,
    filename: packed.filename,
    size: packed.size,
    unpackedSize: packed.unpackedSize,
    integrity: packed.integrity,
    sha256: createHash("sha256").update(tarballBytes).digest("hex"),
    files: packed.files.map(({ path: filePath, size }) => ({
      path: filePath,
      size,
    })),
  });
}

packages.sort(({ name: left }, { name: right }) => left.localeCompare(right));
const releaseManifest = { schemaVersion: 1, packages };
await writeFile(
  path.join(outputDirectory, "release-manifest.json"),
  `${JSON.stringify(releaseManifest, null, 2)}\n`,
);
await writeFile(
  path.join(outputDirectory, "SHA256SUMS"),
  `${packages.map(({ sha256, filename }) => `${sha256}  ${filename}`).join("\n")}\n`,
);
console.log(JSON.stringify(releaseManifest, null, 2));
