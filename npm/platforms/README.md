# Native npm platform packages

Platform package directories are generated for release staging; compiled
binaries and generated package contents are never committed here.

`npm/scripts/stage-platform-package.mjs` creates one minimal package containing
only `package.json`, `README.md`, `LICENSE`, and the native executable. The
package name, `os`, `cpu`, Linux `libc`, and version come from the checked-in
release configuration and the primary `pangenome-range` manifest.

`npm/scripts/verify-release-set.mjs` checks the complete platform set, exact
optional-dependency versions, absence of lifecycle scripts, file allowlists,
and every `npm pack --dry-run` result before tarballs are created.
