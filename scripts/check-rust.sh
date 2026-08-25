#!/usr/bin/env bash

set -euo pipefail

cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
