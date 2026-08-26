import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readdir, readFile, rm } from "node:fs/promises";
import os from "node:os";
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
const cacheDirectory = await mkdtemp(path.join(os.tmpdir(), "pngr-npm-cache-"));

const readJson = async (filePath) =>
  JSON.parse(await readFile(filePath, "utf8"));

async function listFiles(directory, prefix = "") {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const relativePath = path.posix.join(prefix, entry.name);
    if (entry.isDirectory()) {
      files.push(
        ...(await listFiles(path.join(directory, entry.name), relativePath)),
      );
    } else {
      files.push(relativePath);
    }
  }
  return files.sort();
}

function dryRunPack(directory) {
  const result = spawnSync(
    "npm",
    [
      "pack",
      "--dry-run",
      "--json",
      "--ignore-scripts",
      "--cache",
      cacheDirectory,
    ],
    { cwd: directory, encoding: "utf8" },
  );
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const parsed = JSON.parse(result.stdout);
  assert.equal(parsed.length, 1);
  return parsed[0];
}

try {
  const mainManifest = await readJson(path.join(mainDirectory, "package.json"));
  assert.equal(mainManifest.private, false);
  assert.deepEqual(mainManifest.scripts, undefined);
  assert.deepEqual(await listFiles(path.join(mainDirectory, "bin")), [
    "launcher.mjs",
    "pangenome-range.mjs",
  ]);
  const mainPack = dryRunPack(mainDirectory);
  for (const file of mainPack.files) {
    assert(
      /^(LICENSE|README\.md|package\.json|bin\/(launcher|pangenome-range)\.mjs|dist\/)/.test(
        file.path,
      ),
      `unexpected main package file: ${file.path}`,
    );
  }

  for (const [targetName, target] of Object.entries(releaseTargets)) {
    const directory = path.join(platformsDirectory, targetName);
    const manifest = await readJson(path.join(directory, "package.json"));
    assert.equal(manifest.name, target.packageName);
    assert.equal(manifest.version, mainManifest.version);
    assert.deepEqual(manifest.os, [target.os]);
    assert.deepEqual(manifest.cpu, [target.cpu]);
    assert.deepEqual(manifest.libc, target.libc ? [target.libc] : undefined);
    assert.deepEqual(manifest.scripts, undefined);
    assert.equal(
      mainManifest.optionalDependencies[target.packageName],
      mainManifest.version,
    );
    assert.deepEqual(await listFiles(directory), [
      "LICENSE",
      "README.md",
      `bin/${target.binary}`,
      "package.json",
    ]);
    const packed = dryRunPack(directory);
    assert.deepEqual(
      packed.files.map(({ path: filePath }) => filePath).sort(),
      ["LICENSE", "README.md", `bin/${target.binary}`, "package.json"],
    );
  }
  console.log(
    `verified ${mainManifest.name}@${mainManifest.version} and ${Object.keys(releaseTargets).length} native package dry-runs`,
  );
} finally {
  await rm(cacheDirectory, { recursive: true, force: true });
}
