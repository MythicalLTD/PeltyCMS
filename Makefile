# PeltyCMS developer commands. Run `make help`.
# Every target is a thin wrapper around `cargo xtask <target>` (the `xtask`
# crate), which also works on Windows/macOS without make or bash:
#   cargo xtask help    cargo dev    cargo xtask build ...
.DEFAULT_GOAL := help
XTASK := cargo xtask
ADDONS_TARGET ?= $(CURDIR)/target/addons
export ADDONS_TARGET

help: ## Show this help
	@$(XTASK) help

dev: ## Postgres + migrations + backend (cargo-watch if installed) + Vite; stops prior PeltyCMS dev first
	$(XTASK) dev

stop: ## Stop leftover PeltyCMS backend/Vite from a previous make dev
	$(XTASK) stop

backend: ## Backend only (cargo run, or cargo watch if installed)
	$(XTASK) backend

frontend: ## Frontend only (Vite dev server, proxies /api to the backend)
	$(XTASK) frontend

build: ## Release build: frontend, server binary, addon zips, generated docs
	$(XTASK) build

docs: ## Generate the user docs (target/docs) and developer docs (target/docs/developers), with coverage report
	$(XTASK) docs

addons: ## Build every WASM addon into extensions/addons/dist/*.zip
	$(XTASK) addons

addon: ## Scaffold a new WASM addon (NEW=<id> KIND=gateway|provisioning|registrar|captcha|sso|integration)
	$(XTASK) addon $(NEW) --kind $(or $(KIND),integration)

test: ## Rust workspace tests (needs TEST_DATABASE_URL) + frontend tests
	$(XTASK) test

lint: ## clippy + TypeScript check
	$(XTASK) lint

db: ## Start the dev Postgres if needed, create and migrate the DATABASE_URL database
	$(XTASK) db

db-reset: ## Drop and recreate the DATABASE_URL database, then migrate
	$(XTASK) db-reset

seed: ## Load demo data into a running server (ADMIN_EMAIL, ADMIN_PASSWORD, CLIENT_PASSWORD)
	$(XTASK) seed

seed-extra: ## More demo data for newer features (after make seed, same env)
	$(XTASK) seed-extra

docker: ## Build the Docker image (peltycms:latest)
	$(XTASK) docker

# Rust equivalent of frontend `ncu` / `ncu -u` (needs: cargo install cargo-edit).
# `cargo update` alone only moves the lockfile within existing Cargo.toml ranges.
deps-rust: ## Like ncu (compatible): bump Cargo.toml within semver, refresh lockfile
	@command -v cargo-upgrade >/dev/null || { echo "install: cargo install cargo-edit"; exit 1; }
	cargo upgrade
	cargo update

deps-rust-latest: ## Like ncu -u: bump Cargo.toml to latest including majors (often needs code fixes)
	@command -v cargo-upgrade >/dev/null || { echo "install: cargo install cargo-edit"; exit 1; }
	cargo upgrade --incompatible
	cargo update

clean: ## Remove build outputs (target/, frontend/dist, addon zips)
	$(XTASK) clean

.PHONY: help dev stop backend frontend build docs addons addon test lint db db-reset seed seed-extra docker deps-rust deps-rust-latest clean
