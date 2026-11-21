#!/usr/bin/env bash
# Zero-install contributor environment: honor rust-toolchain.toml exactly and
# install the Stellar CLI (which replaces the old soroban-cli) once, at create.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# rustup reads rust-toolchain.toml, so this installs the pinned channel,
# components and the wasm target declared there.
rustup show

if ! command -v stellar >/dev/null 2>&1; then
  cargo install --locked stellar-cli
fi
stellar --version
