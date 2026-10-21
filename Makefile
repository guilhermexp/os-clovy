# Local command layer mirroring CI (see .github/workflows/). `make verify`
# runs the same gates as the desktop + clovy-api workflows, so a green
# `make verify` locally should mean green CI. Use `make dev` to run clovy-api
# and the desktop app together locally; production builds use `pnpm tauri:build`.
.PHONY: help install \
	dev dev-api \
	check format typecheck test-web \
	tauri-fmt tauri-fmt-check tauri-lint tauri-test \
	companion-fmt companion-fmt-check companion-lint companion-test \
	clovy-api-fmt clovy-api-fmt-check clovy-api-lint clovy-api-test \
	fmt fmt-check lint test verify org-references-check \
	local-ci signoff-pr signoff-frontend signoff-rust-macos

.DEFAULT_GOAL := help

help:  ## Show this help
	@grep -hE '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| sort \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# --- Install ---
install:  ## Install frontend deps (Rust builds via cargo)
	pnpm install --frozen-lockfile

# --- Run (local dev) ---
# `pnpm tauri:dev` selects free worktree-local ports, boots clovy-api and Vite
# alongside the native app, and tears them all down on exit. The
# desktop app reads CLOVY_API_URL from .env and clovy-api reads its keys from
# clovy-api/.env (both auto-load their .env), so this is the whole local stack in
# one command.
dev:  ## Run the desktop app + clovy-api together (Ctrl-C stops both)
	pnpm tauri:dev

dev-api:  ## Run only clovy-api locally on :8080 (loads clovy-api/.env)
	cd clovy-api && cargo run -p clovy-api-server -- serve

# --- Frontend (src/, scripts/) ---
check:  ## Biome check (format + lint, incl. the lucide ban)
	pnpm check

format:  ## Biome format (write) + biome safe fixes
	pnpm check:write

typecheck:  ## tsc --noEmit
	pnpm typecheck

test-web:  ## Vitest
	pnpm test

# --- Tauri shell (src-tauri/) ---
tauri-fmt:  ## rustfmt (write)
	cargo fmt --manifest-path src-tauri/Cargo.toml --all

tauri-fmt-check:  ## rustfmt (check only)
	cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check

tauri-lint:  ## clippy (warnings = errors)
	cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings

tauri-test:  ## cargo test
	cargo test --manifest-path src-tauri/Cargo.toml --locked

# --- Companion protocol and crypto crates (crates/clovy-companion-*) ---
companion-fmt:  ## rustfmt companion crates (write)
	cargo fmt --manifest-path crates/clovy-companion-protocol/Cargo.toml --all
	cargo fmt --manifest-path crates/clovy-companion-crypto/Cargo.toml --all
	cargo fmt --manifest-path crates/june-companion-protocol/Cargo.toml --all
	cargo fmt --manifest-path crates/june-companion-crypto/Cargo.toml --all

companion-fmt-check:  ## rustfmt companion crates (check only)
	cargo fmt --manifest-path crates/clovy-companion-protocol/Cargo.toml --all -- --check
	cargo fmt --manifest-path crates/clovy-companion-crypto/Cargo.toml --all -- --check
	cargo fmt --manifest-path crates/june-companion-protocol/Cargo.toml --all -- --check
	cargo fmt --manifest-path crates/june-companion-crypto/Cargo.toml --all -- --check

companion-lint:  ## clippy companion crates (warnings = errors)
	cargo clippy --manifest-path crates/clovy-companion-protocol/Cargo.toml --all-targets --locked -- -D warnings
	cargo clippy --manifest-path crates/clovy-companion-crypto/Cargo.toml --all-targets --locked -- -D warnings
	cargo clippy --manifest-path crates/june-companion-protocol/Cargo.toml --all-targets --locked -- -D warnings
	cargo clippy --manifest-path crates/june-companion-crypto/Cargo.toml --all-targets --locked -- -D warnings

companion-test:  ## cargo test companion crates
	cargo test --manifest-path crates/clovy-companion-protocol/Cargo.toml --locked
	cargo test --manifest-path crates/clovy-companion-crypto/Cargo.toml --locked
	cargo test --manifest-path crates/june-companion-protocol/Cargo.toml --locked
	cargo test --manifest-path crates/june-companion-crypto/Cargo.toml --locked

.PHONY: benchmark-note-transcription-latency benchmark-calendar-account-poll benchmark-share-rate-limiter
benchmark-note-transcription-latency:
	cargo test --manifest-path src-tauri/Cargo.toml --locked --release commands::note_transcription_benchmark::benchmark_post_finalization_note_transcription_latency -- --ignored --exact --nocapture --test-threads=1

benchmark-calendar-account-poll:
	cargo test --manifest-path src-tauri/Cargo.toml --locked --release connectors::triggers::tests::benchmark_calendar_account_poll_consolidation -- --ignored --exact --nocapture --test-threads=1

benchmark-share-rate-limiter:  ## Benchmark share limiter latency and concurrent throughput
	cd clovy-api && cargo bench -p clovy-api --bench share_rate_limiter --features benchmark --locked

# --- Clovy API backend (clovy-api/) ---
clovy-api-fmt:  ## rustfmt (write)
	cd clovy-api && cargo fmt --all

clovy-api-fmt-check:  ## rustfmt (check only)
	cd clovy-api && cargo fmt --all -- --check

clovy-api-lint:  ## clippy (warnings = errors)
	cd clovy-api && cargo clippy --all-targets --all-features --locked -- -D warnings

clovy-api-test:  ## cargo test
	cd clovy-api && cargo test --all-targets --all-features --locked

# --- Aggregates ---
fmt: format tauri-fmt companion-fmt clovy-api-fmt  ## Format everything

fmt-check: tauri-fmt-check companion-fmt-check clovy-api-fmt-check  ## Check Rust formatting

lint: check tauri-lint companion-lint clovy-api-lint  ## Lint everything

test: test-web tauri-test companion-test clovy-api-test  ## Run all test suites

org-references-check:  ## Fail on Open Software organization references in repo, CI and app source
	node scripts/check-org-references.mjs

verify: org-references-check check typecheck test-web tauri-fmt-check tauri-lint tauri-test companion-fmt-check companion-lint companion-test clovy-api-fmt-check clovy-api-lint clovy-api-test  ## Full CI-parity gate

local-ci:  ## Run path-aware local PR checks and post required signoff/* statuses
	./scripts/local-ci.sh

signoff-pr: local-ci

signoff-frontend:  ## Run local frontend typecheck/tests and post signoff/frontend
	./scripts/signoff-frontend.sh

signoff-rust-macos:  ## Run local macOS Tauri Rust checks and post signoff/rust-macos
	./scripts/signoff-rust-macos.sh
