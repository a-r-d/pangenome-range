# pangenome-range

`pangenome-range` provides a browser-safe TypeScript reader, a framework-neutral
SVG tube-map viewer, a Node file source, and a launcher for the native Rust encoder
CLI.

```bash
npm install pangenome-range
npx pangenome-range encode input.gbz output.pngr
```

```ts
import { openPangenome } from "pangenome-range";
import {
  buildTubeMapModel,
  layoutTubeMap,
  renderTubeMapSvg,
} from "pangenome-range/viewer";
```

The viewer adapter performs deterministic local-pattern selection and structural
collapse. The layout is reference-anchored, and the renderer draws into a
caller-owned SVG. It never opens an archive or issues range requests itself.

The native executable is supplied by an exact-version optional package selected
for the current operating system, CPU, and Linux libc. Installing with
`--omit=optional` leaves every JavaScript entry point usable; invoking the CLI
then reports how to install the missing native package.

Native CLI packages are available for macOS arm64/x64 and Linux arm64 glibc or
x64 glibc/musl. The native CLI is not currently available on Windows because
the upstream `simple-sds` dependency requires Unix file descriptors and `mmap`;
the JavaScript reader and viewer remain portable.

See the [repository README](https://github.com/a-r-d/pangenome-range#readme) and
[distribution guide](https://github.com/a-r-d/pangenome-range/blob/main/docs/DISTRIBUTION.md)
for supported targets and release details.
