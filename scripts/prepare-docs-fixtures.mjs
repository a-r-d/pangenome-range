import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repository = dirname(dirname(fileURLToPath(import.meta.url)));
const archiveSource = join(
  repository,
  "test-data",
  "conformance",
  "format-v1.pngr",
);
const archiveDestination = join(
  repository,
  "docs",
  "public",
  "fixtures",
  "format-v1.pngr",
);
const expectedArchiveSha256 =
  "f703e99934e52c906d9f10971328dd947a6196da0c38c1aed31e24ba31c98f89";

const bytes = await readFile(archiveSource);
const sha256 = createHash("sha256").update(bytes).digest("hex");
if (sha256 !== expectedArchiveSha256) {
  throw new Error(
    `refusing to publish an unexpected demo fixture: expected ${expectedArchiveSha256}, got ${sha256}`,
  );
}
await mkdir(dirname(archiveDestination), { recursive: true });
await copyFile(archiveSource, archiveDestination);
console.log(
  `prepared deterministic synthetic Pages fixture (${bytes.byteLength} bytes, sha256 ${sha256})`,
);

const tubeMapSource = join(
  repository,
  "packages",
  "browser",
  "test-data",
  "tube-map-golden.json",
);
const tubeMapDestination = join(
  repository,
  "docs",
  "public",
  "fixtures",
  "tube-map-golden.json",
);
const expectedTubeMapSha256 =
  "72a99c09255850694718dfbd5f299379370fad4bc6f37c6f2edb0d86a0df1647";
const tubeMapBytes = await readFile(tubeMapSource);
const tubeMapSha256 = createHash("sha256").update(tubeMapBytes).digest("hex");
if (tubeMapSha256 !== expectedTubeMapSha256) {
  throw new Error(
    `refusing to publish an unexpected tube-map fixture: expected ${expectedTubeMapSha256}, got ${tubeMapSha256}`,
  );
}
await copyFile(tubeMapSource, tubeMapDestination);
console.log(
  `prepared deterministic tube-map fixture (${tubeMapBytes.byteLength} bytes, sha256 ${tubeMapSha256})`,
);
