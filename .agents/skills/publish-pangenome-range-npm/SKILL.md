---
name: publish-pangenome-range-npm
description: Prepare, publish, and verify a pangenome-range npm release, including its exact-version @pangenome-range native CLI packages. Use for version bumps, release-artifact checks, manual npm publication, registry verification, or diagnosing a partially published pangenome-range release.
---

# Publish pangenome-range to npm

Publish one user-facing package backed by five platform packages. Treat publication as irreversible: never publish without explicit user authorization.

## 1. Prepare the release

1. Read `docs/DISTRIBUTION.md`, inspect `git status`, and confirm the intended version and commit.
2. Use Node.js 24 and run:

   ```bash
   pnpm install --frozen-lockfile
   pnpm check
   pnpm check:rust
   pnpm build
   pnpm package:cargo
   ```

3. Confirm version synchronization with `pnpm check:versions`.
4. Confirm `npm whoami` and `npm org ls pangenome-range` identify the expected owner.
5. Check `npm view pangenome-range@VERSION version`. If that version exists, do not attempt to replace it.
6. Run the `Release artifacts` GitHub Actions workflow from the exact release commit. Require all five native jobs and the package job to pass.
7. Download the workflow's `npm-release-set` artifact into a new temporary directory. Run `sha256sum -c SHA256SUMS` there.

Do not construct platform tarballs locally or reuse artifacts from a different commit.

## 2. Publish in dependency order

Publish the five platform packages first. Replace `VERSION` with the release version and run from the verified artifact directory:

```bash
npm publish ./pangenome-range-cli-darwin-arm64-VERSION.tgz --access public --provenance=false
npm publish ./pangenome-range-cli-darwin-x64-VERSION.tgz --access public --provenance=false
npm publish ./pangenome-range-cli-linux-arm64-gnu-VERSION.tgz --access public --provenance=false
npm publish ./pangenome-range-cli-linux-x64-gnu-VERSION.tgz --access public --provenance=false
npm publish ./pangenome-range-cli-linux-x64-musl-VERSION.tgz --access public --provenance=false
```

Stop if any platform publication fails. After all five succeed, publish the main package last:

```bash
npm publish ./pangenome-range-VERSION.tgz --access public --provenance=false
```

Use `--provenance=false` only for a manual command-line release. A future protected trusted-publishing workflow should emit provenance instead.

If npm requests two-factor authentication, rerun only the affected command with `--otp=CODE`. Never republish packages that already succeeded.

## 3. Verify the registry

Use `npm view`, not npm search indexing, to verify the exact versions:

```bash
npm view pangenome-range@VERSION version dist.integrity
npm view @pangenome-range/cli-darwin-arm64@VERSION version dist.integrity
npm view @pangenome-range/cli-darwin-x64@VERSION version dist.integrity
npm view @pangenome-range/cli-linux-arm64-gnu@VERSION version dist.integrity
npm view @pangenome-range/cli-linux-x64-gnu@VERSION version dist.integrity
npm view @pangenome-range/cli-linux-x64-musl@VERSION version dist.integrity
```

New scoped packages can briefly return `E404` from one registry endpoint while their npm pages are already public. Retry exact `npm view` checks for several minutes before declaring publication missing. Do not use search results as immediate evidence.

## 4. Test a clean consumer install

Create a new temporary directory, install from the registry, and verify both JavaScript and the host CLI:

```bash
npm install --no-audit --no-fund pangenome-range@VERSION
node --input-type=module -e "import('pangenome-range').then(m => console.log(m.PANGENOME_RANGE_API_VERSION))"
node --input-type=module -e "import('pangenome-range/viewer').then(m => console.log(typeof m.buildTubeMapModel))"
npx pangenome-range --version
npx pangenome-range --help
```

Report the exact version, source commit, workflow run, published package list, checksum result, and clean-install results. Distinguish a working JavaScript install from proof that the native CLI package installed and executed.

## Partial publication recovery

- If the main package exists but a native package appears missing, wait and retry `npm view` before taking action.
- If a native package is genuinely absent, publish only that verified tarball; do not republish the main package.
- If npm reports that a version already exists, inspect that exact version and stop. npm versions are immutable.
