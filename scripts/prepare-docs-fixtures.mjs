import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repository = dirname(dirname(fileURLToPath(import.meta.url)));
const source = join(repository, "test-data", "conformance", "format-v1.pngr");
const destination = join(
  repository,
  "docs",
  "public",
  "fixtures",
  "format-v1.pngr",
);
const expectedSha256 =
  "b952efedc2274d91f0ff3e8979203a9221ef5f4d81d7eb3465afd90e7fbb0984";

const bytes = await readFile(source);
const sha256 = createHash("sha256").update(bytes).digest("hex");
if (sha256 !== expectedSha256) {
  throw new Error(
    `refusing to publish an unexpected demo fixture: expected ${expectedSha256}, got ${sha256}`,
  );
}
await mkdir(dirname(destination), { recursive: true });
await copyFile(source, destination);
console.log(
  `prepared deterministic synthetic Pages fixture (${bytes.byteLength} bytes, sha256 ${sha256})`,
);
