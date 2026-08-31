import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import {
  parseArguments,
  releaseTargets,
  requiredArgument,
} from "./release-config.mjs";

const argumentsMap = parseArguments(process.argv.slice(2));
const releaseDirectory = path.resolve(
  requiredArgument(argumentsMap, "release-dir"),
);
const releaseManifest = JSON.parse(
  await readFile(path.join(releaseDirectory, "release-manifest.json"), "utf8"),
);

function detectHostTarget() {
  const libc =
    process.platform === "linux"
      ? typeof process.report?.getReport()?.header?.glibcVersionRuntime ===
        "string"
        ? "glibc"
        : "musl"
      : undefined;
  return Object.entries(releaseTargets).find(
    ([, target]) =>
      target.os === process.platform &&
      target.cpu === process.arch &&
      target.libc === libc,
  );
}

const [hostTargetName, hostTarget] =
  detectHostTarget() ??
  (() => {
    throw new Error(
      `packed release smoke does not support ${process.platform}-${process.arch}`,
    );
  })();
const mainPackage = releaseManifest.packages.find(
  ({ name }) => name === "pangenome-range",
);
const nativePackage = releaseManifest.packages.find(
  ({ name }) => name === hostTarget.packageName,
);
assert(mainPackage, "main package tarball is missing from release manifest");
assert(nativePackage, `${hostTarget.packageName} tarball is missing`);

const temporaryRoot = await mkdtemp(path.join(os.tmpdir(), "pngr-pack-smoke-"));
const npmCache = path.join(temporaryRoot, "npm-cache");

function run(command, args, cwd, expectedStatus = 0) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, npm_config_cache: npmCache },
  });
  assert.ifError(result.error);
  assert.equal(
    result.status,
    expectedStatus,
    `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`,
  );
  return result;
}

async function createProject(name) {
  const directory = path.join(temporaryRoot, name);
  await mkdir(directory);
  await writeFile(
    path.join(directory, "package.json"),
    `${JSON.stringify({ name, private: true, type: "module" }, null, 2)}\n`,
  );
  return directory;
}

try {
  const mainTarball = path.join(releaseDirectory, mainPackage.filename);
  const nativeTarball = path.join(releaseDirectory, nativePackage.filename);

  const omittedProject = await createProject("omitted-optional");
  run(
    "npm",
    [
      "install",
      "--omit=optional",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      mainTarball,
    ],
    omittedProject,
  );
  const missing = run(
    "npx",
    ["--no-install", "pangenome-range", "--version"],
    omittedProject,
    1,
  );
  assert.match(missing.stderr, /optional native package .* is missing/i);
  assert.match(missing.stderr, /without --omit=optional/i);

  const installedProject = await createProject("installed-native");
  run(
    "npm",
    [
      "install",
      "--omit=optional",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      mainTarball,
      nativeTarball,
    ],
    installedProject,
  );
  const version = run(
    "npx",
    ["--no-install", "pangenome-range", "--version"],
    installedProject,
  );
  assert.match(
    version.stdout,
    new RegExp(mainPackage.version.replaceAll(".", "\\.")),
  );
  assert.match(
    run("npx", ["--no-install", "pangenome-range", "--help"], installedProject)
      .stdout,
    /Usage:/,
  );
  assert.match(
    run(
      "npx",
      ["--no-install", "pangenome-range", "encode", "--help"],
      installedProject,
    ).stdout,
    /pangenome-range encode/,
  );
  run(
    process.execPath,
    [
      "--input-type=module",
      "--eval",
      'import { openPangenome } from "pangenome-range"; import { buildTubeMapModel } from "pangenome-range/viewer"; if (typeof openPangenome !== "function" || typeof buildTubeMapModel !== "function") process.exit(1);',
    ],
    installedProject,
  );
  console.log(
    `packed ${hostTargetName} install, CLI, reader, and viewer smoke passed`,
  );
} finally {
  await rm(temporaryRoot, { recursive: true, force: true });
}
