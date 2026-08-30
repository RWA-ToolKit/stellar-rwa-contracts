.PHONY: build test fmt fmt-check clippy verify clean deploy

# Build all contracts to wasm.
build:
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

# Lint the workspace, denying warnings.
clippy:
	cargo clippy --workspace --all-targets -- -D warnings

# Run the full local gate in the same order as CI: test, format check,
# lint, then the wasm build. Mirrors .github/workflows/ci.yml so
# contributors can catch a CI failure before pushing.
verify: test fmt-check clippy build

# Remove build artifacts.
clean:
	cargo clean

# Deploy + initialize everything on Testnet.
deploy:
	NETWORK=$(or $(NETWORK),testnet) IDENTITY=$(or $(IDENTITY),rwa-admin) ./scripts/deploy.sh

update-doc-addresses:
	python3 scripts/generate_addresses.py
