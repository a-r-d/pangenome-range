# Distribution

`pangenome-range` has one primary npm product and two additional native
installation forms. No package or release is published by the repository's
ordinary CI.

## Primary npm package

The public package name is `pangenome-range`. It contains:

```text
pangenome-range          browser-safe reader root
pangenome-range/reader   explicit reader entry
pangenome-range/viewer   framework-neutral Canvas viewer
pangenome-range/node     Node positioned-file source
pangenome-range          Node executable shim
```

The executable and the JavaScript root intentionally share the package name:

```bash
npm install pangenome-range
npx pangenome-range encode input.gbz output.pngr
```

The root and `/reader` entries contain no DOM, Node built-ins, launcher code, or
native code. `/viewer` owns the framework-neutral tube-map model, layout, and
SVG behavior; `/node` owns Node file I/O; the
executable shim lives only under `bin/`. Bundle-size checks also reject native
launcher markers in reader and viewer output.

Named source-path identity is a pre-1.0 experimental reader capability in the same
browser-safe entry. It does not add a native or `gbwt-rs` dependency to JavaScript,
and ordinary `query()` calls do not fetch its optional descriptor or pages. The
archive format remains v1 with a skippable registered extension; this is not a stable
format promise and old research named-membership archives must be regenerated.

The public viewer export also includes pure formatting helpers for named-path TSV
and oriented tile-local FASTA. They consume already decoded typed arrays and do not
add transport, DOM, Node, or native dependencies to the reader/viewer bundle.

`npm install --omit=optional` remains a valid JavaScript-only installation. If
the CLI is invoked from that installation, the shim identifies the exact
missing optional package and tells the user to reinstall without
`--omit=optional` or install that package explicitly.

## Native backing packages

The main package pins all native packages as exact-version
`optionalDependencies`:

| Target | npm package | Restriction |
|---|---|---|
| macOS Apple Silicon | `@pangenome-range/cli-darwin-arm64` | `os=darwin`, `cpu=arm64` |
| macOS Intel | `@pangenome-range/cli-darwin-x64` | `os=darwin`, `cpu=x64` |
| Linux x64 glibc | `@pangenome-range/cli-linux-x64-gnu` | `os=linux`, `cpu=x64`, `libc=glibc` |
| Linux x64 musl | `@pangenome-range/cli-linux-x64-musl` | `os=linux`, `cpu=x64`, `libc=musl` |
| Linux arm64 glibc | `@pangenome-range/cli-linux-arm64-gnu` | `os=linux`, `cpu=arm64`, `libc=glibc` |

Each generated platform package contains only `package.json`, `README.md`,
`LICENSE`, and `bin/pangenome-range`. There is no `postinstall`,
runtime download, GitHub API fetch, or local Rust compilation. Linux libc is
selected from Node's runtime report when it identifies glibc. An explicit
`PANGENOME_RANGE_LIBC=gnu|musl` override takes precedence. If the report is
missing or ambiguous, the shim uses the only installed matching exact-version
optional package; it never guesses musl from missing glibc metadata. Before spawning, the shim verifies the
platform package version and declared executable path. It inherits all three
standard streams and forwards arguments, termination signals, and exit status.

The native CLI is not currently published for Windows because its upstream
`simple-sds` dependency requires Unix file descriptors and `mmap`. Linux arm64
musl and other operating-system/architecture pairs are also outside the current
build matrix. The launcher fails with a concise supported target list;
JavaScript imports continue to work on unsupported native targets. A new target
requires a compilable upstream dependency path, a reproducible native build
runner, and the same package/install smoke gates before it can be added.

## Standalone native binaries

The binary remains named `pangenome-range`. The manual release workflow builds
the same five-target matrix, runs target Rust tests, creates `.tar.gz`
archives, and emits `SHA256SUMS`. With an explicit boolean input and the
protected `release` environment, it can attach those files to a draft GitHub
release for an existing version tag.

The default workflow path only retains Actions artifacts. Pull requests,
ordinary `main` pushes, and the manual dry-run path cannot publish to npm or
create a GitHub release.

## Cargo installation path

The recommended eventual crates.io package name is
`pangenome-range-cli`; installing it will still create the
`pangenome-range` binary:

```bash
cargo install pangenome-range-cli
```

This preserves the existing crate and binary names while avoiding a second
package-name claim that looks like the TypeScript library. The CLI manifest has
description, README, homepage, repository, license, keywords, categories, a
tight include list, and versioned local dependencies.

`pnpm package:cargo` is the current passing dry-run gate. It uses
`cargo package --allow-dirty --list` and requires exactly the staged manifest,
lockfile, README, CLI source, and CLI integration test. A full
`cargo package --no-verify` cannot yet resolve the three internal
`pangenome-range-*` library crates from crates.io. Those crates will need a
deliberate publication or consolidation decision before Cargo publication; this
tranche does not publish or restructure research crates merely to bypass that
boundary.

## Release staging and version policy

The source of release truth is guarded rather than duplicated silently:

- the main npm version equals the workspace npm version;
- it equals `[workspace.package].version` in Cargo;
- the native binary prints Cargo's `CARGO_PKG_VERSION` for `--version`;
- every optional native dependency uses that exact version;
- every platform manifest is generated from the main manifest.

`npm/scripts/stage-release-set.mjs` consumes native build artifacts and creates
the main and platform staging directories. `verify-release-set.mjs` checks
metadata, exact versions, package restrictions, lifecycle-script absence, file
allowlists, and every `npm pack --dry-run`. `pack-release-set.mjs` emits all
seven tarballs, a machine-readable contents/size manifest, and checksums. The
host smoke then installs the packed main and native tarballs into a clean
project, runs `npx pangenome-range --help`,
`npx pangenome-range encode --help`, and `--version`, and imports the reader and
viewer.

Version `0.1.0` was published manually from the verified `npm-release-set`
artifact: all five exact-version platform packages first, then the main package.
The release workflow still stages and verifies packages without publishing them.
Future automated publication should use npm trusted publishing from a protected
release workflow; no `npm publish` command is currently present in Actions.

## Remaining automated-publishing setup

Ownership for `pangenome-range` and the `@pangenome-range` scope is established.
The remaining service-side steps for trusted automated publication cannot be
proved by a local package rehearsal:

1. Protect `main`, or add an equivalent ruleset, requiring the normal CI and
   CodeQL checks.
2. Bind npm trusted publishing to the protected release workflow.
3. Configure the protected GitHub `release` environment and its approval rules.
4. Run the release workflow and require all five real target artifacts before
   any publish step.

GitHub Pages is already configured to deploy through Actions. Remote run
`33036238701` built, smoke-tested, uploaded, and deployed the site successfully;
this setup step is complete.

Local staging verifies manifests, file allowlists, exact versions, lifecycle
script absence, tarball shape, and the host binary. Synthetic non-host package
shapes are not evidence that the corresponding native target compiled or ran.
