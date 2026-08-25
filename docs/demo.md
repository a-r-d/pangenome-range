# Demo

<script setup>
import PackageVersion from './components/PackageVersion.vue'
</script>

<PackageVersion />

The interactive range-query demo is intentionally a placeholder. The public
package exports resolve, but the archive decoder and viewer do not yet exist.
No production archive URL is hardcoded here.

A later tranche will load a configured immutable archive, issue real HTTP
`Range` requests, expose the request trace, and render decoded regional tiles.
Until then this page makes no browser-performance or decoding claim.
