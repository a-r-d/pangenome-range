# pangenome-range

`pangenome-range` provides a browser-safe TypeScript reader, a framework-neutral
Canvas viewer, a Node file source, and a launcher for the native Rust encoder
CLI.

```bash
npm install pangenome-range
npx pangenome-range encode input.gbz output.pngr
```

```ts
import { openPangenome } from "pangenome-range";
import { createPangenomeViewer } from "pangenome-range/viewer";
```

The native executable is supplied by an exact-version optional package selected
for the current operating system, CPU, and Linux libc. Installing with
`--omit=optional` leaves every JavaScript entry point usable; invoking the CLI
then reports how to install the missing native package.

See the [repository README](https://github.com/a-r-d/pangenome-range#readme) and
[distribution guide](https://github.com/a-r-d/pangenome-range/blob/main/docs/DISTRIBUTION.md)
for supported targets and release details.
