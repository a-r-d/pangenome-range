import { copyFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageEntry = fileURLToPath(import.meta.resolve("@bokuweb/zstd-wasm"));
const source = resolve(dirname(packageEntry), "zstd.wasm");
const destination = resolve("dist/browser/zstd.wasm");
await mkdir(dirname(destination), { recursive: true });
await copyFile(source, destination);
