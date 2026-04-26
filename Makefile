# Liaozhai MUX — project Makefile.

.PHONY: help build release test lint format check check-all docs run run-config clean coverage push

CARGO  := cargo
PORT   ?= 4444
CONFIG ?= liaozhai.toml

help: ## Show this help message.
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

build: ## Build all workspace crates (debug).
	$(CARGO) build --workspace

release: ## Build all workspace crates (release).
	$(CARGO) build --workspace --release

test: ## Run all tests.
	$(CARGO) test --workspace

lint: ## Run clippy and check formatting.
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace -- -D warnings

format: ## Auto-format all source files.
	$(CARGO) fmt --all

check: build lint test ## Build, lint, and test.

check-all: check coverage ## Build, lint, test, and coverage.

docs: ## Generate API documentation.
	$(CARGO) doc --workspace --no-deps --document-private-items

run: ## Run the server with default port.
	$(CARGO) run --bin liaozhai-server -- run --port $(PORT)

run-config: ## Run the server with a config file.
	$(CARGO) run --bin liaozhai-server -- run --config $(CONFIG)

clean: ## Remove build artifacts.
	$(CARGO) clean

coverage: ## Generate text coverage report (requires cargo-llvm-cov).
	$(CARGO) llvm-cov --workspace

push: ## Push to all remotes.
	git push macpro
	git push github
	git push codeberg
