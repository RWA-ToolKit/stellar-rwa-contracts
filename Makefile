.PHONY: build test coverage fmt fmt-check clippy changelog-check check clean deploy interface-specs update-interface-specs check-interface-specs

BASE_REF ?= upstream/main

# Build all contracts to wasm.
build:
	stellar contract build

# Run the full workspace test suite.
test:
	cargo test

# Allow legacy API deprecations and fixed contract ABI argument counts.
clippy:
	cargo clippy --workspace --all-targets -- -D warnings -A deprecated -A clippy::too_many_arguments

# Check whether contract changes since BASE_REF have a CHANGELOG entry.
changelog-check:
	./scripts/check_changelog.sh "$(BASE_REF)"

# Run the core local checks used before opening a pull request.
check:
	$(MAKE) fmt-check
	$(MAKE) clippy
	$(MAKE) test
	$(MAKE) changelog-check BASE_REF="$(BASE_REF)"

# Print a per-crate coverage summary (requires cargo-llvm-cov).
coverage:
	cargo llvm-cov --workspace --summary-only

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

update-doc-addresses:
	python3 scripts/generate_addresses.py

# Emit the interface specification for each contract as a consumable build artifact.
# Specs land in target/interface-specs/<contract>.json and can be used to generate
# bindings for the web app and API (see CONTRIBUTING.md).
interface-specs: build
	mkdir -p target/interface-specs
	for contract in asset-token compliance dividend registry; do \
		wasm_contract=$$contract; \
		if [ "$$contract" = asset-token ]; then wasm_contract=asset_token; fi; \
		wasm=target/wasm32v1-none/release/$$wasm_contract.wasm; \
		stellar contract info interface --wasm "$$wasm" --output json > target/interface-specs/$$contract.json; \
	done

# Regenerate the committed interface specs in specs/interface/ after a public
# function is added, removed, or changed. Commit the result with the code change.
update-interface-specs: interface-specs
	rm -rf specs/interface
	mkdir -p specs/interface
	cp target/interface-specs/*.json specs/interface/

# Fail if the committed interface specs no longer match the built contracts.
check-interface-specs: interface-specs
	diff -ru specs/interface target/interface-specs
