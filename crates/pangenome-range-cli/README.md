# pangenome-range CLI

The native `pangenome-range` executable encodes GBZ inputs into static
range-addressable `.pngr` archives and provides inspection, validation,
source-oracle verification, fixture generation, and research benchmark
commands.

The planned crates.io package name is `pangenome-range-cli`; the installed
binary remains `pangenome-range`:

```bash
cargo install pangenome-range-cli
pangenome-range --help
```

The same binary is also staged for standalone GitHub Release archives and for
the platform-specific optional packages used by the primary npm package.

This crate is pre-release. See the
[repository](https://github.com/a-r-d/pangenome-range) for the current format,
semantics, build constraints, and distribution policy.
