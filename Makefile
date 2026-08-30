.PHONY: build build-cli test fmt fmt-check clippy verify clean deploy

# Build all contracts to wasm the same way CI does: plain cargo against the
# wasm32v1-none target. This avoids requiring the Stellar CLI just to build.
build:
	cargo build --workspace --release --target wasm32v1-none

# Build via the Stellar CLI instead (needed for `stellar contract deploy`
# workflows, e.g. `make deploy`). Requires the CLI to be installed.
build-cli:
	stellar contract build

# Run the full workspace test suite.
test:
	cargo test

# Format all code.
fmt:
	cargo fmt --all

# Check formatting without writing.
fmt-check:
	cargo fmt --all -- --check

# Lint with clippy, denying warnings.
clippy:
	cargo clippy --all-targets -- -D warnings

# Run the full local verification suite: formatting, lints, and tests.
verify: fmt-check clippy test

# Remove build artifacts.
clean:
	cargo clean

# Deploy + initialize everything on Testnet.
deploy:
	NETWORK=$(or $(NETWORK),testnet) IDENTITY=$(or $(IDENTITY),rwa-admin) ./scripts/deploy.sh

update-doc-addresses:
	python3 scripts/generate_addresses.py
