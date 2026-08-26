import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFile,
  mkdir,
  readdir,
  readFile,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  parseArguments,
  releaseTargets,
  requiredArgument,
} from "./release-config.mjs";

const argumentsMap = parseArguments(process.argv.slice(2));
const nativeDirectory = path.resolve(
  requiredArgument(argumentsMap, "native-dir"),
);
const outputDirectory = path.resolve(requiredArgument(argumentsMap, "out-dir"));
const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const mainDirectory = path.join(outputDirectory, "main");
const platformsDirectory = path.join(outputDirectory, "platforms");
const nativeAssetsDirectory = path.join(outputDirectory, "native");

await mkdir(outputDirectory, { recursive: false });
await Promise.all([
  mkdir(platformsDirectory, { recursive: true }),
  mkdir(nativeAssetsDirectory, { recursive: true }),
]);

function runScript(name, args) {
  const result = spawnSync(
    process.execPath,
    [path.join(scriptDirectory, name), ...args],
    { encoding: "utf8" },
  );
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  process.stdout.write(result.stdout);
}

runScript("stage-main-package.mjs", ["--out-dir", mainDirectory]);
const checksums = [];
for (const [targetName, target] of Object.entries(releaseTargets)) {
  const artifactDirectory = path.join(nativeDirectory, `native-${targetName}`);
  runScript("stage-platform-package.mjs", [
    "--target",
    targetName,
    "--binary",
    path.join(artifactDirectory, "bin", target.binary),
    "--out-dir",
    path.join(platformsDirectory, targetName),
  ]);
  for (const entry of await readdir(artifactDirectory)) {
    if (!entry.endsWith(".tar.gz") && !entry.endsWith(".zip")) continue;
    const source = path.join(artifactDirectory, entry);
    const destination = path.join(nativeAssetsDirectory, entry);
    await copyFile(source, destination);
    const bytes = await readFile(destination);
    checksums.push({
      filename: entry,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    });
  }
}
checksums.sort(({ filename: left }, { filename: right }) =>
  left.localeCompare(right),
);
await writeFile(
  path.join(nativeAssetsDirectory, "SHA256SUMS"),
  `${checksums.map(({ sha256, filename }) => `${sha256}  ${filename}`).join("\n")}\n`,
);
console.log(
  `staged ${Object.keys(releaseTargets).length} platform packages and ${checksums.length} native archives`,
);
