# Distribution

`pangenome-range` has two independently distributed products under one project
name.

## Native Rust CLI

The native binary remains `pangenome-range`. It owns GBZ inspection, archive
encoding, verification, and research benchmarking. Eventual native installation
paths are prebuilt binaries attached to GitHub releases and/or:

```bash
cargo install pangenome-range-cli
```

The Rust crate and release process are not yet declared stable by this document.

## TypeScript package

The npm working name is also `pangenome-range`. The package contains only the
portable browser/Node range reader and the framework-neutral viewer. Its public
entry points are:

```text
pangenome-range
pangenome-range/reader
pangenome-range/viewer
pangenome-range/node
```

The root and `/reader` entries are browser-safe and reader-focused. Node
built-ins are isolated to `/node`; viewer and DOM types are isolated to
`/viewer`.

The package remains `private: true` until npm ownership, naming, release policy,
and provenance publishing are explicitly confirmed. The initial release will
not contain an npm wrapper around the Rust binary, and the native binary will
not be bundled into npm.
