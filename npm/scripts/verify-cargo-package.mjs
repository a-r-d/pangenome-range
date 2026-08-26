import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";

const result = spawnSync(
  "cargo",
  [
    "package",
    "-p",
    "pangenome-range-cli",
    "--allow-dirty",
    "--locked",
    "--list",
  ],
  { encoding: "utf8" },
);
assert.ifError(result.error);
assert.equal(result.status, 0, result.stderr || result.stdout);
const files = result.stdout.trim().split("\n").sort();
assert.deepEqual(files, [
  ".cargo_vcs_info.json",
  "Cargo.lock",
  "Cargo.toml",
  "Cargo.toml.orig",
  "README.md",
  "src/main.rs",
  "tests/cli.rs",
]);
console.log(
  `Cargo package contents verified for pangenome-range-cli (${files.length} files)`,
);
