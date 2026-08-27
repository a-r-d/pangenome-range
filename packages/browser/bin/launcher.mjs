import { spawn } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import { access, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import process from "node:process";

export const PLATFORM_PACKAGES = Object.freeze({
  "darwin-arm64": Object.freeze({
    packageName: "@pangenome-range/cli-darwin-arm64",
    binary: "bin/pangenome-range",
  }),
  "darwin-x64": Object.freeze({
    packageName: "@pangenome-range/cli-darwin-x64",
    binary: "bin/pangenome-range",
  }),
  "linux-arm64-gnu": Object.freeze({
    packageName: "@pangenome-range/cli-linux-arm64-gnu",
    binary: "bin/pangenome-range",
  }),
  "linux-x64-gnu": Object.freeze({
    packageName: "@pangenome-range/cli-linux-x64-gnu",
    binary: "bin/pangenome-range",
  }),
  "linux-x64-musl": Object.freeze({
    packageName: "@pangenome-range/cli-linux-x64-musl",
    binary: "bin/pangenome-range",
  }),
  "win32-x64": Object.freeze({
    packageName: "@pangenome-range/cli-win32-x64",
    binary: "bin/pangenome-range.exe",
  }),
});

const supportedTargets = Object.keys(PLATFORM_PACKAGES).join(", ");

export class NativeCliError extends Error {
  constructor(message, options) {
    super(message, options);
    this.name = "NativeCliError";
  }
}

export function detectLinuxLibc(report = process.report) {
  let header;
  try {
    header = report?.getReport()?.header;
  } catch {
    header = undefined;
  }
  return typeof header?.glibcVersionRuntime === "string" &&
    header.glibcVersionRuntime.length > 0
    ? "gnu"
    : undefined;
}

function explicitLinuxLibc(libc, env = process.env) {
  const value = libc ?? env.PANGENOME_RANGE_LIBC;
  if (value === undefined || value === "") return undefined;
  if (value !== "gnu" && value !== "musl") {
    throw new NativeCliError(
      `Invalid Linux libc override ${JSON.stringify(value)}; PANGENOME_RANGE_LIBC must be gnu or musl.`,
    );
  }
  return value;
}

export function selectPlatformPackage({
  platform = process.platform,
  arch = process.arch,
  libc,
  report = process.report,
  env = process.env,
} = {}) {
  const linuxLibc =
    platform === "linux"
      ? (explicitLinuxLibc(libc, env) ?? detectLinuxLibc(report))
      : undefined;
  if (platform === "linux" && linuxLibc === undefined) {
    throw new NativeCliError(
      `Could not determine Linux libc for ${platform}-${arch}. Set PANGENOME_RANGE_LIBC=gnu or PANGENOME_RANGE_LIBC=musl explicitly.`,
    );
  }
  const target =
    platform === "linux"
      ? `${platform}-${arch}-${linuxLibc}`
      : `${platform}-${arch}`;
  const selected = PLATFORM_PACKAGES[target];
  if (!selected) {
    throw new NativeCliError(
      `The native pangenome-range CLI does not support ${target}. Supported targets: ${supportedTargets}. The JavaScript reader and viewer remain available.`,
    );
  }
  return { ...selected, target };
}

export async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, "utf8"));
}

export async function resolveNativeCli({
  mainPackageJson = new URL("../package.json", import.meta.url),
  platform,
  arch,
  libc,
  report,
  env = process.env,
  resolve = createRequire(import.meta.url).resolve,
} = {}) {
  const mainManifest = await readJson(mainPackageJson);
  const actualPlatform = platform ?? process.platform;
  const actualArch = arch ?? process.arch;
  let selected;
  const override =
    actualPlatform === "linux" ? explicitLinuxLibc(libc, env) : undefined;
  const detected =
    actualPlatform === "linux" && override === undefined
      ? detectLinuxLibc(report)
      : undefined;
  if (
    actualPlatform === "linux" &&
    override === undefined &&
    detected === undefined
  ) {
    const candidates = ["gnu", "musl"].map((candidate) =>
      selectPlatformPackage({
        platform: actualPlatform,
        arch: actualArch,
        libc: candidate,
        env,
      }),
    );
    const installed = [];
    for (const candidate of candidates) {
      try {
        installed.push({
          selected: candidate,
          manifestPath: resolve(`${candidate.packageName}/package.json`),
        });
      } catch {
        // Optional packages for other libc families are normally absent.
      }
    }
    if (installed.length !== 1) {
      throw new NativeCliError(
        installed.length === 0
          ? `Could not determine Linux libc and neither exact-version optional package (${candidates.map(({ packageName }) => packageName).join(", ")}) is installed. Set PANGENOME_RANGE_LIBC=gnu or musl, then reinstall pangenome-range without --omit=optional.`
          : `Could not determine Linux libc and both GNU and musl optional packages are installed. Set PANGENOME_RANGE_LIBC=gnu or musl explicitly.`,
      );
    }
    selected = installed[0].selected;
  } else {
    selected = selectPlatformPackage({
      platform: actualPlatform,
      arch: actualArch,
      libc: override ?? detected,
      report,
      env,
    });
  }
  let nativeManifestPath;
  try {
    nativeManifestPath = resolve(`${selected.packageName}/package.json`);
  } catch (error) {
    throw new NativeCliError(
      `The optional native package ${selected.packageName}@${mainManifest.version} is missing. Reinstall pangenome-range without --omit=optional, or install that exact package explicitly. The JavaScript reader and viewer remain available.`,
      { cause: error },
    );
  }

  const nativeManifest = await readJson(nativeManifestPath);
  if (nativeManifest.version !== mainManifest.version) {
    throw new NativeCliError(
      `Version mismatch: pangenome-range@${mainManifest.version} resolved ${selected.packageName}@${nativeManifest.version}. Reinstall both packages at ${mainManifest.version}.`,
    );
  }

  const declaredBinary = nativeManifest.pangenomeRange?.binary;
  if (declaredBinary !== selected.binary) {
    throw new NativeCliError(
      `${selected.packageName}@${nativeManifest.version} has invalid native binary metadata. Reinstall the package.`,
    );
  }

  const packageRoot = path.dirname(nativeManifestPath);
  const binaryPath = path.resolve(packageRoot, declaredBinary);
  const relativeBinaryPath = path.relative(packageRoot, binaryPath);
  if (
    relativeBinaryPath.startsWith("..") ||
    path.isAbsolute(relativeBinaryPath)
  ) {
    throw new NativeCliError(
      `${selected.packageName}@${nativeManifest.version} points outside its package directory.`,
    );
  }
  try {
    await access(
      binaryPath,
      process.platform === "win32" ? fsConstants.F_OK : fsConstants.X_OK,
    );
  } catch (error) {
    throw new NativeCliError(
      `The native executable is missing or not executable at ${binaryPath}. Reinstall ${selected.packageName}@${mainManifest.version}.`,
      { cause: error },
    );
  }

  return {
    binaryPath,
    mainVersion: mainManifest.version,
    nativeVersion: nativeManifest.version,
    packageName: selected.packageName,
    target: selected.target,
  };
}

const forwardedSignals =
  process.platform === "win32"
    ? ["SIGINT", "SIGTERM"]
    : ["SIGHUP", "SIGINT", "SIGTERM"];

export function spawnNativeCli(
  binaryPath,
  args,
  { spawnImplementation = spawn, parentProcess = process } = {},
) {
  return new Promise((resolve, reject) => {
    const child = spawnImplementation(binaryPath, args, { stdio: "inherit" });
    const handlers = new Map();
    let settled = false;

    const cleanup = () => {
      for (const [signal, handler] of handlers) {
        parentProcess.removeListener(signal, handler);
      }
    };
    const settle = (callback) => {
      if (settled) return;
      settled = true;
      cleanup();
      callback();
    };

    for (const signal of forwardedSignals) {
      const handler = () => {
        if (child.exitCode === null && child.signalCode === null) {
          child.kill(signal);
        }
      };
      handlers.set(signal, handler);
      parentProcess.on(signal, handler);
    }

    child.once("error", (error) => {
      settle(() =>
        reject(
          new NativeCliError(
            `Failed to start the native pangenome-range CLI: ${error.message}`,
            { cause: error },
          ),
        ),
      );
    });
    child.once("close", (code, signal) => {
      settle(() => resolve({ code, signal }));
    });
  });
}

export async function launchNativeCli(args, options = {}) {
  const resolved = await resolveNativeCli(options);
  return spawnNativeCli(resolved.binaryPath, args, options);
}
