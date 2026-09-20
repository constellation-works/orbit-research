.DEFAULT_GOAL := help
.PHONY: help build install check test crate-test fmt fmt-check clippy check-dependency-direction ci clean

CARGO ?= cargo
CARGO_TARGET_DIR ?= target
export CARGO_TARGET_DIR

# Override with a build-budget wrapper accepting `-- COMMAND ...` if needed.
BUILD_BUDGET ?= env
BINARY := orbit-research
BIN_CRATE := orbit-research-cli
INSTALL_PROFILE ?= release
INSTALL_BIN_DIR ?= $(HOME)/.local/bin

ifeq ($(INSTALL_PROFILE),release)
INSTALL_CARGO_PROFILE := --release
INSTALL_TARGET_DIR := $(CARGO_TARGET_DIR)/release
else ifeq ($(INSTALL_PROFILE),debug)
INSTALL_CARGO_PROFILE :=
INSTALL_TARGET_DIR := $(CARGO_TARGET_DIR)/debug
else
$(error INSTALL_PROFILE must be release or debug)
endif

help:
	@echo "Orbit Research Make Targets"
	@echo ""
	@echo "  make build                       Build the Rust workspace"
	@echo "  make install                     Install the CLI (release; INSTALL_BIN_DIR overrides destination)"
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

install:
	$(BUILD_BUDGET) -- $(CARGO) build -p $(BIN_CRATE) $(INSTALL_CARGO_PROFILE)
	install -d "$(INSTALL_BIN_DIR)"
	install -m 755 "$(INSTALL_TARGET_DIR)/$(BINARY)" "$(INSTALL_BIN_DIR)/$(BINARY)"

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
