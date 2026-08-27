import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  detectLinuxLibc,
  NativeCliError,
  resolveNativeCli,
  selectPlatformPackage,
} from "../bin/launcher.mjs";

const temporaryDirectories = [];
afterEach(async () => {
  await Promise.all(
    temporaryDirectories
      .splice(0)
      .map((directory) => rm(directory, { recursive: true, force: true })),
  );
});

test("selects every supported platform package", () => {
  const cases = [
    ["darwin", "arm64", undefined, "@pangenome-range/cli-darwin-arm64"],
    ["darwin", "x64", undefined, "@pangenome-range/cli-darwin-x64"],
    ["linux", "arm64", "gnu", "@pangenome-range/cli-linux-arm64-gnu"],
    ["linux", "x64", "gnu", "@pangenome-range/cli-linux-x64-gnu"],
    ["linux", "x64", "musl", "@pangenome-range/cli-linux-x64-musl"],
    ["win32", "x64", undefined, "@pangenome-range/cli-win32-x64"],
  ];
  for (const [platform, arch, libc, packageName] of cases) {
    assert.equal(
      selectPlatformPackage({ platform, arch, libc }).packageName,
      packageName,
    );
  }
  assert.equal(
    detectLinuxLibc({
      getReport: () => ({ header: { glibcVersionRuntime: "2.39" } }),
    }),
    "gnu",
  );
  assert.equal(
    detectLinuxLibc({ getReport: () => ({ header: {} }) }),
    undefined,
  );
  assert.throws(
    () =>
      selectPlatformPackage({
        platform: "linux",
        arch: "x64",
        report: { getReport: () => ({ header: {} }) },
        env: {},
      }),
    /Could not determine Linux libc/,
  );
  assert.equal(
    selectPlatformPackage({
      platform: "linux",
      arch: "x64",
      report: { getReport: () => ({ header: {} }) },
      env: { PANGENOME_RANGE_LIBC: "gnu" },
    }).packageName,
    "@pangenome-range/cli-linux-x64-gnu",
  );
});

test("rejects unsupported targets with an actionable error", () => {
  assert.throws(
    () => selectPlatformPackage({ platform: "freebsd", arch: "x64" }),
    (error) =>
      error instanceof NativeCliError &&
      /does not support freebsd-x64/.test(error.message) &&
      /JavaScript reader and viewer remain available/.test(error.message),
  );
});

async function createResolutionFixture(nativeVersion) {
  const directory = await mkdtemp(path.join(os.tmpdir(), "pngr-resolve-"));
  temporaryDirectories.push(directory);
  const mainManifest = path.join(directory, "main-package.json");
  const nativeManifest = path.join(directory, "native-package.json");
  const nativeBinary = path.join(directory, "bin", "pangenome-range");
  await mkdir(path.dirname(nativeBinary), { recursive: true });
  await Promise.all([
    writeFile(mainManifest, '{"name":"pangenome-range","version":"0.1.0"}\n'),
    writeFile(
      nativeManifest,
      `${JSON.stringify({
        name: "fixture",
        version: nativeVersion,
        pangenomeRange: { binary: "bin/pangenome-range" },
      })}\n`,
    ),
    writeFile(nativeBinary, "#!/bin/sh\nexit 0\n"),
  ]);
  await chmod(nativeBinary, 0o755);
  return { mainManifest, nativeManifest };
}

test("reports a missing optional native dependency", async () => {
  const { mainManifest } = await createResolutionFixture("0.1.0");
  await assert.rejects(
    resolveNativeCli({
      mainPackageJson: mainManifest,
      platform: "linux",
      arch: "x64",
      libc: "gnu",
      resolve: () => {
        const error = new Error("missing");
        error.code = "MODULE_NOT_FOUND";
        throw error;
      },
    }),
    /Reinstall pangenome-range without --omit=optional/,
  );
});

test("resolves the only installed libc package when process.report is ambiguous", async () => {
  const { mainManifest, nativeManifest } =
    await createResolutionFixture("0.1.0");
  const resolved = await resolveNativeCli({
    mainPackageJson: mainManifest,
    platform: "linux",
    arch: "x64",
    report: { getReport: () => ({ header: {} }) },
    env: {},
    resolve: (specifier) => {
      if (specifier === "@pangenome-range/cli-linux-x64-gnu/package.json") {
        return nativeManifest;
      }
      const error = new Error("missing");
      error.code = "MODULE_NOT_FOUND";
      throw error;
    },
  });
  assert.equal(resolved.packageName, "@pangenome-range/cli-linux-x64-gnu");
});

test("rejects a platform package version mismatch before launch", async () => {
  const { mainManifest, nativeManifest } =
    await createResolutionFixture("0.1.1");
  await assert.rejects(
    resolveNativeCli({
      mainPackageJson: mainManifest,
      platform: "linux",
      arch: "x64",
      libc: "gnu",
      resolve: () => nativeManifest,
    }),
    /Version mismatch: pangenome-range@0.1.0 resolved .*@0.1.1/,
  );
});

async function createInstalledFixture(nativeSource) {
  if (process.platform === "win32") {
    throw new Error("executable fixture is Unix-only");
  }
  const directory = await mkdtemp(path.join(os.tmpdir(), "pngr-launch-"));
  temporaryDirectories.push(directory);
  const binDirectory = path.join(directory, "bin");
  const selected = selectPlatformPackage();
  const nativeRoot = path.join(
    directory,
    "node_modules",
    ...selected.packageName.split("/"),
  );
  const nativeBinary = path.join(nativeRoot, selected.binary);
  await Promise.all([
    mkdir(binDirectory, { recursive: true }),
    mkdir(path.dirname(nativeBinary), { recursive: true }),
  ]);
  await Promise.all([
    copyFile(
      fileURLToPath(new URL("../bin/launcher.mjs", import.meta.url)),
      path.join(binDirectory, "launcher.mjs"),
    ),
    copyFile(
      fileURLToPath(new URL("../bin/pangenome-range.mjs", import.meta.url)),
      path.join(binDirectory, "pangenome-range.mjs"),
    ),
    writeFile(
      path.join(directory, "package.json"),
      '{"name":"pangenome-range","version":"0.1.0","type":"module"}\n',
    ),
    writeFile(
      path.join(nativeRoot, "package.json"),
      `${JSON.stringify({
        name: selected.packageName,
        version: "0.1.0",
        pangenomeRange: { binary: selected.binary },
      })}\n`,
    ),
    writeFile(nativeBinary, nativeSource),
  ]);
  await Promise.all([
    chmod(path.join(binDirectory, "pangenome-range.mjs"), 0o755),
    chmod(nativeBinary, 0o755),
  ]);
  return {
    directory,
    shim: path.join(binDirectory, "pangenome-range.mjs"),
  };
}

test("executes a real fixture executable and forwards arguments and exit status", {
  skip: process.platform === "win32",
}, async () => {
  const fixture = await createInstalledFixture(`#!/usr/bin/env node
import { writeFileSync } from "node:fs";
writeFileSync(process.env.PNG_RANGE_ARGS_FILE, JSON.stringify(process.argv.slice(2)));
process.exit(Number(process.env.PNG_RANGE_EXIT_CODE));
`);
  const argumentsFile = path.join(fixture.directory, "arguments.json");
  const result = spawnSync(fixture.shim, ["encode", "a b.gbz", "out.pngr"], {
    encoding: "utf8",
    env: {
      ...process.env,
      PNG_RANGE_ARGS_FILE: argumentsFile,
      PNG_RANGE_EXIT_CODE: "23",
    },
  });
  assert.equal(result.status, 23, result.stderr);
  assert.deepEqual(JSON.parse(await readFile(argumentsFile, "utf8")), [
    "encode",
    "a b.gbz",
    "out.pngr",
  ]);
});

test("forwards termination signals and preserves signal exit", {
  skip: process.platform === "win32",
}, async () => {
  const fixture = await createInstalledFixture(`#!/usr/bin/env node
import { writeFileSync } from "node:fs";
writeFileSync(process.env.PNG_RANGE_READY_FILE, "ready");
setInterval(() => {}, 1000);
`);
  const readyFile = path.join(fixture.directory, "ready");
  const child = spawn(fixture.shim, [], {
    env: { ...process.env, PNG_RANGE_READY_FILE: readyFile },
    stdio: "ignore",
  });
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      await readFile(readyFile);
      break;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }
  assert.equal(await readFile(readyFile, "utf8"), "ready");
  child.kill("SIGTERM");
  const [code, signal] = await new Promise((resolve) =>
    child.once("close", (...result) => resolve(result)),
  );
  assert.equal(code, null);
  assert.equal(signal, "SIGTERM");
});
