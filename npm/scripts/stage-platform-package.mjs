import { chmod, copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  parseArguments,
  releaseTargets,
  requiredArgument,
} from "./release-config.mjs";

const argumentsMap = parseArguments(process.argv.slice(2));
const targetName = requiredArgument(argumentsMap, "target");
const binarySource = path.resolve(requiredArgument(argumentsMap, "binary"));
const outputDirectory = path.resolve(requiredArgument(argumentsMap, "out-dir"));
const target = releaseTargets[targetName];
if (!target) {
  throw new Error(
    `unknown target ${targetName}; expected ${Object.keys(releaseTargets).join(", ")}`,
  );
}

const repositoryRoot = fileURLToPath(new URL("../../", import.meta.url));
const mainManifest = JSON.parse(
  await readFile(
    path.join(repositoryRoot, "packages/browser/package.json"),
    "utf8",
  ),
);
const binaryRelativePath = `bin/${target.binary}`;
const manifest = {
  name: target.packageName,
  version: mainManifest.version,
  description: `Native pangenome-range CLI for ${targetName}`,
  license: "MIT",
  repository: mainManifest.repository,
  homepage: mainManifest.homepage,
  os: [target.os],
  cpu: [target.cpu],
  ...(target.libc ? { libc: [target.libc] } : {}),
  files: ["bin", "LICENSE", "README.md"],
  pangenomeRange: { binary: binaryRelativePath },
  publishConfig: { access: "public", provenance: true },
};

await mkdir(path.join(outputDirectory, "bin"), { recursive: true });
await Promise.all([
  copyFile(binarySource, path.join(outputDirectory, binaryRelativePath)),
  copyFile(
    path.join(repositoryRoot, "LICENSE"),
    path.join(outputDirectory, "LICENSE"),
  ),
  writeFile(
    path.join(outputDirectory, "package.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
  ),
  writeFile(
    path.join(outputDirectory, "README.md"),
    `# ${target.packageName}\n\nNative ${targetName} executable for [pangenome-range](https://github.com/a-r-d/pangenome-range). This package is installed automatically as an optional dependency of the same-version primary package.\n`,
  ),
]);
if (target.os !== "win32") {
  await chmod(path.join(outputDirectory, binaryRelativePath), 0o755);
}
console.log(`staged ${target.packageName}@${mainManifest.version}`);
