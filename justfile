# adr recipes — `just <recipe>` from the repo root.
#
# Install with: `cargo install just` (or brew / apt equivalent).
# Requires Docker for the `db` recipe.

# Print recipe list when `just` is run alone.
default:
    @just --list

# ─── Day-to-day dev ──────────────────────────────────────────────
# Start Postgres in Docker, build the server, run the Rust backend,
# and start the Vite dev server — all in parallel. One Ctrl-C kills
# everything. Good for `just dev` as your morning command.
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    just db-up
    # Run server + web in parallel; `wait -n` exits on first failure
    # so a crashed server kills the web dev server too.
    cargo build -p floe-server
    (cargo run -q -p floe-server) &
    SERVER_PID=$!
    (cd apps/web && npm run dev) &
    WEB_PID=$!
    trap "kill $SERVER_PID $WEB_PID 2>/dev/null || true" EXIT
    wait -n

# Start the server alone (foreground).
server:
    cargo run -q -p floe-server

# Start the web dev server alone (foreground).
web:
    cd apps/web && npm run dev

# ─── Database ────────────────────────────────────────────────────
# Bring up Postgres via docker compose. Idempotent.
db-up:
    docker compose up -d postgres

# Stop Postgres.
db-down:
    docker compose down

# Open a psql shell against the dev database.
db-shell:
    docker compose exec postgres psql -U postgres -d adr

# ─── Validation ──────────────────────────────────────────────────
# Run the full test + lint + typecheck suite the way CI runs it.
check:
    cargo test --workspace --no-fail-fast
    cargo clippy --all-targets -- -D warnings
    cd apps/web && npx tsc --noEmit
    cd apps/web && npm run build

# Regenerate JSON schema + frontend types from floe-core.
regen-schema:
    cargo run -q -p floe-cli -- schema > schema.json
    cd apps/web && npm run gen:types

# ─── Samples ─────────────────────────────────────────────────────
# Pre-warm cache with every fixture in `fixtures/`. Handy after a
# PIPELINE_VERSION bump so the first reviewer visit isn't slow.
warm-samples:
    ls fixtures/pr-*/ | xargs -I{} curl -X POST http://localhost:8787/samples/{}/analyze
