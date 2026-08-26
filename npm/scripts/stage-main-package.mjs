import { cp, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { parseArguments, requiredArgument } from "./release-config.mjs";

const argumentsMap = parseArguments(process.argv.slice(2));
const outputDirectory = path.resolve(requiredArgument(argumentsMap, "out-dir"));
const repositoryRoot = fileURLToPath(new URL("../../", import.meta.url));
const sourceDirectory = path.join(repositoryRoot, "packages/browser");

await mkdir(outputDirectory, { recursive: false });
for (const name of ["package.json", "README.md", "LICENSE"]) {
  await cp(path.join(sourceDirectory, name), path.join(outputDirectory, name));
}
await cp(path.join(sourceDirectory, "bin"), path.join(outputDirectory, "bin"), {
  recursive: true,
});
await cp(
  path.join(sourceDirectory, "dist"),
  path.join(outputDirectory, "dist"),
  {
    recursive: true,
  },
);

const manifest = JSON.parse(
  await readFile(path.join(outputDirectory, "package.json"), "utf8"),
);
delete manifest.scripts;
delete manifest.devDependencies;
await writeFile(
  path.join(outputDirectory, "package.json"),
  `${JSON.stringify(manifest, null, 2)}\n`,
);
console.log(
  `staged ${manifest.name}@${manifest.version} at ${outputDirectory}`,
);
