import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";

export const DEFAULT_DEMO_ARCHIVE_URL =
  "https://archives.ard.ninja/pangenome-range/sha256/82585cb612effbf414b1c8f38b049bc415876866168ccc929f9a885f06d97b0a/hprc-v2.1-gencode-v50-named-membership-82585cb612effbf4.pngr";
export const DEFAULT_DEMO_1000G_ARCHIVE_URL =
  "https://archives.ard.ninja/pangenome-range/sha256/71730fab7aad0dbbef81cf7c74b4fa8dbacbb3aad5bab0a797349120b18f6afb/1000gplons-hs38d1-na19239-h0-v1-t8-zstd3.pngr";
export const DEFAULT_DEMO_RICE_ARCHIVE_URL =
  "https://archives.ard.ninja/pangenome-range/sha256/c91768e6e98d32ff6467732a26e32def5058f4c15d247a0ac6a252a4403e134c/rice-chr06-mc-xa7-anonymous.pngr";
export const DEFAULT_DEMO_CHICKEN_ARCHIVE_URL =
  "https://archives.ard.ninja/pangenome-range/sha256/93bcd713ccda14bf4e650c1c8d56751e5ed5db7624aecbf76769fa1909d25e4e/chicken-whole-named.pngr";

export function docsDevEnvironment(environment = process.env) {
  return {
    ...environment,
    VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL:
      environment.VITE_PANGENOME_RANGE_DEMO_ARCHIVE_URL?.trim() ||
      DEFAULT_DEMO_ARCHIVE_URL,
    VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL:
      environment.VITE_PANGENOME_RANGE_DEMO_1000G_ARCHIVE_URL?.trim() ||
      DEFAULT_DEMO_1000G_ARCHIVE_URL,
    VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL:
      environment.VITE_PANGENOME_RANGE_DEMO_RICE_ARCHIVE_URL?.trim() ||
      DEFAULT_DEMO_RICE_ARCHIVE_URL,
    VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL:
      environment.VITE_PANGENOME_RANGE_DEMO_CHICKEN_ARCHIVE_URL?.trim() ||
      DEFAULT_DEMO_CHICKEN_ARCHIVE_URL,
  };
}

async function run() {
  const child = spawn(
    "pnpm",
    ["--filter", "@pangenome-range/docs", "dev", ...process.argv.slice(2)],
    {
      env: docsDevEnvironment(),
      stdio: "inherit",
      shell: process.platform === "win32",
    },
  );
  const result = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
  if (result.signal !== null) {
    console.error(`Docs development server stopped by ${result.signal}.`);
    process.exitCode = 1;
  } else {
    process.exitCode = result.code ?? 1;
  }
}

if (
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  await run();
}
