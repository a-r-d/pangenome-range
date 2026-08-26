import assert from "node:assert/strict";
import * as root from "pangenome-range";
import * as node from "pangenome-range/node";
import * as reader from "pangenome-range/reader";
import * as viewer from "pangenome-range/viewer";

assert.equal(root.PANGENOME_RANGE_API_VERSION, "0.1.0");
assert.equal(reader.PANGENOME_RANGE_API_VERSION, "0.1.0");
assert.equal(typeof viewer.createPangenomeViewer, "function");
assert.equal("ViewerModelBuilder" in viewer, false);
assert.equal("ProgressiveTileQuery" in viewer, false);
assert.equal(typeof node.FileRangeSource, "function");
assert.equal(typeof reader.HttpRangeSource, "function");
assert.equal(typeof reader.openPangenome, "function");
assert.equal("createPangenomeViewer" in root, false);
assert.equal("FileRangeSource" in root, false);

console.log("public package exports resolve and remain isolated");
