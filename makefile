# Rust development commands for Toss Backend.
# Running `make` repairs common issues, formats the code, and validates it.

CARGO ?= cargo

.DEFAULT_GOAL := all

.PHONY: all fix fmt fmt-check check clippy test build release run migrate ci help

all: fix fmt check clippy test

# Apply compiler and Clippy suggestions. The allow flags make this usable while
# developing with staged or uncommitted files.
fix:
	$(CARGO) fix --workspace --all-targets --all-features --allow-dirty --allow-staged
	$(CARGO) clippy --fix --workspace --all-targets --all-features --allow-dirty --allow-staged

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

check:
	$(CARGO) check --workspace --all-targets --all-features

clippy:
	$(CARGO) clippy --workspace --all-targets --all-features -- -D warnings

test:
	$(CARGO) test --workspace --all-targets --all-features

build:
	$(CARGO) build --workspace --all-targets --all-features

release:
	$(CARGO) build --release --workspace --all-features

run:
	$(CARGO) run --bin toss-backend

# Pass migration arguments with ARGS, for example:
# make migrate ARGS="migration apply"
migrate:
	$(CARGO) run --bin migrate -- $(ARGS)

# Read-only validation suitable for CI; unlike `all`, this changes no sources.
ci: fmt-check check clippy test

help:
	@echo "make              Fix, format, check, lint, and test"
	@echo "make fix         Apply compiler and Clippy suggestions"
	@echo "make fmt         Format all Rust code"
	@echo "make fmt-check   Verify formatting without changing files"
	@echo "make check       Type-check all targets"
	@echo "make clippy      Run Clippy and reject warnings"
	@echo "make test        Run all tests"
	@echo "make build       Build all debug targets"
	@echo "make release     Build the release binary"
	@echo "make run         Run the backend server"
	@echo "make migrate ARGS=\"migration apply\""
	@echo "make ci          Run all non-mutating validation checks"
