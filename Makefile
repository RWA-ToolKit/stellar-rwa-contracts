.PHONY: build test fmt fmt-check clean deploy

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

# Remove build artifacts.
clean:
	cargo clean

# Deploy + initialize everything on Testnet.
deploy:
	NETWORK=$(or $(NETWORK),testnet) IDENTITY=$(or $(IDENTITY),rwa-admin) ./scripts/deploy.sh
