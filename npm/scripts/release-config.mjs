export const releaseTargets = Object.freeze({
  "darwin-arm64": Object.freeze({
    packageName: "@pangenome-range/cli-darwin-arm64",
    os: "darwin",
    cpu: "arm64",
    binary: "pangenome-range",
  }),
  "darwin-x64": Object.freeze({
    packageName: "@pangenome-range/cli-darwin-x64",
    os: "darwin",
    cpu: "x64",
    binary: "pangenome-range",
  }),
  "linux-arm64-gnu": Object.freeze({
    packageName: "@pangenome-range/cli-linux-arm64-gnu",
    os: "linux",
    cpu: "arm64",
    libc: "glibc",
    binary: "pangenome-range",
  }),
  "linux-x64-gnu": Object.freeze({
    packageName: "@pangenome-range/cli-linux-x64-gnu",
    os: "linux",
    cpu: "x64",
    libc: "glibc",
    binary: "pangenome-range",
  }),
  "linux-x64-musl": Object.freeze({
    packageName: "@pangenome-range/cli-linux-x64-musl",
    os: "linux",
    cpu: "x64",
    libc: "musl",
    binary: "pangenome-range",
  }),
  "win32-x64": Object.freeze({
    packageName: "@pangenome-range/cli-win32-x64",
    os: "win32",
    cpu: "x64",
    binary: "pangenome-range.exe",
  }),
});

export function parseArguments(argv) {
  if (argv[0] === "--") argv = argv.slice(1);
  const values = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      throw new Error(
        `expected --name value arguments, received: ${argv.join(" ")}`,
      );
    }
    values.set(key.slice(2), value);
  }
  return values;
}

export function requiredArgument(argumentsMap, name) {
  const value = argumentsMap.get(name);
  if (!value) throw new Error(`missing required --${name} argument`);
  return value;
}
