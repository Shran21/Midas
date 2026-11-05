set windows-shell := ["cmd.exe", "/d", "/c"]

default:
    @just --list

setup:
    pnpm install --frozen-lockfile

dev:
    pnpm dev

dev-auth *args:
    python .github/scripts/dev-auth.py {{args}}

build:
    cargo build --workspace --locked
    pnpm build

test:
    cargo test --workspace --locked
    pnpm typecheck

test-database:
    python .github/scripts/test-database.py

lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    pnpm lint

fmt:
    cargo fmt --all
    pnpm format

release:
    cargo build --workspace --release --locked
    pnpm build

security:
    python .github/scripts/check-secret-detection.py
    gitleaks git --redact --no-banner --log-opts=HEAD
    cargo audit --file Cargo.lock --deny warnings
    cargo deny --locked --all-features check
    pnpm audit --audit-level low
