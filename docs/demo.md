# Demo

<script setup>
import PackageVersion from './components/PackageVersion.vue'
</script>

<PackageVersion />

The interactive range-query UI is intentionally a placeholder. The public
package now includes the archive-v4 reader, strict HTTP range source, zstd
decompression, and regional decoder; the viewer has not been implemented. No
production archive URL is hardcoded here.

A later tranche will connect this page to a configured immutable archive,
expose the request trace, and render decoded regional tiles. Real cross-origin
`206` loading is already covered by the browser integration gate, but this page
makes no public-origin performance claim.
