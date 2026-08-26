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
  "f703e99934e52c906d9f10971328dd947a6196da0c38c1aed31e24ba31c98f89";

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
