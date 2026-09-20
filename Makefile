.DEFAULT_GOAL := help
.PHONY: help build check test crate-test fmt fmt-check clippy check-dependency-direction ci clean

CARGO ?= cargo
CARGO_TARGET_DIR ?= target
export CARGO_TARGET_DIR

help:
	@echo "Orbit Research Make Targets"
	@echo ""
	@echo "  make build                       Build the Rust workspace"
	@echo "  make check                       Type-check the Rust workspace"
	@echo "  make test                        Run the complete required drop-in gate"
	@echo "  make crate-test                  Run Rust crate and CLI tests"
	@echo "  make fmt                         Format Rust code"
	@echo "  make fmt-check                   Check Rust formatting"
	@echo "  make clippy                      Lint all CLI targets with warnings denied"
	@echo "  make check-dependency-direction  Enforce the accepted crate graph"
	@echo "  make ci                          Alias for the complete required gate"
	@echo "  make clean                       Clean Cargo build artifacts"

build:
	$(CARGO) build --workspace --locked --target-dir "$(CARGO_TARGET_DIR)"

check:
	$(CARGO) check --workspace --locked --target-dir "$(CARGO_TARGET_DIR)"

# Keep the gate sequential, including when a caller supplies make -j.
test:
	$(MAKE) fmt-check
	$(MAKE) clippy
	$(MAKE) crate-test
	$(MAKE) check-dependency-direction

crate-test:
	$(CARGO) test --workspace --locked --target-dir "$(CARGO_TARGET_DIR)"

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy -p orbit-research-cli --all-targets --locked --no-deps --target-dir "$(CARGO_TARGET_DIR)" -- -D warnings

check-dependency-direction:
	./scripts/check-dependency-direction.sh --self-test

ci: test
	git diff --check

clean:
	$(CARGO) clean --target-dir "$(CARGO_TARGET_DIR)"

# Dashboard formatting uses a pinned formatter; Node/npm are development-only.
.PHONY: fmt-dashboard fmt-check-dashboard
fmt-dashboard:
	./scripts/format-dashboard.sh --write

fmt-check-dashboard:
	./scripts/format-dashboard.sh --check
