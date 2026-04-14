# AGENTS.md

## Project Overview

SNIP-36 virtual block proving tooling for Starknet. Two-phase pipeline:
1. **Execute** — Run virtual OS against RPC node → produces Cairo PIE
2. **Prove** — Feed PIE through bootloader into stwo prover → produces stwo proof

## Architecture

The project is a Rust workspace centered on the web/backend crates:

- `crates/snip36-core/` — Shared library: typed config, Starknet RPC client, SNIP-36 signing, proof encoding
- `crates/snip36-server/` — Axum web backend (replaces FastAPI) for the proving playground
- `extractor/` — Rust crate that extracts the compiled virtual OS program (excluded from default workspace, requires `deps/sequencer/`)
- `web/frontend/` — React + TypeScript playground UI (unchanged)
- `tests/contracts/` — Cairo test contracts (Counter, Messenger, CoinFlip) for E2E tests
- `scripts/` — Shell scripts for external binary orchestration (setup, prove, run-virtual-os)
- `sample-input/` — Template inputs for the prover and bootloader
- `deps/` — (generated, gitignored) Cloned repos: `proving-utils`, `sequencer`

## Key Conventions

- All Rust code targets stable toolchain (workspace crates)
- External dependencies (`stwo`, `sequencer`) require `nightly-2025-07-14`
- Config loaded from `.env` via `snip36_core::Config`
- Structured logging via `tracing` crate
- Error handling via `color-eyre` and typed errors (`thiserror`) in core
- All proof output is base64-encoded (runner outputs directly as base64)
- Proofs and build artifacts go in `output/` (gitignored)

## Building

```bash
cargo build --workspace                 # Build all crates
cargo build --release -p snip36-server  # Build the backend library
cargo build --release -p snip36-playground  # Build the backend binary

# External dependencies (stwo prover, starknet_os_runner) are managed outside this trimmed copy
```

## Web Playground

```bash
# Backend (Rust):
cargo run --release -p snip36-playground

# Frontend (unchanged):
cd web/frontend && npm install && npm run dev
```

## Testing

```bash
cargo test --workspace           # Unit tests
```

## Environment

- `.env` contains secrets (RPC URL, private key) — never commit
- `.env.example` shows required variables
- Target network: Starknet Sepolia

## Working with Proofs

- PIE files: `.pie.zip` — Cairo Program Independent Execution artifacts
- Proof files: `.proof` — stwo proofs as base64 strings
- Proof facts: `.proof_facts` — JSON array of hex felt strings identifying the proven execution
- L2→L1 messages: `.raw_messages.json` — saved when the virtual tx emits messages (only data channel from virtual tx to real tx)
- The `proof_facts` field in INVOKE_TXN_V3 must be included in Poseidon tx hash computation (non-standard — see `crates/snip36-core/src/signing.rs`)

## Common Pitfalls

- Runner must use `--prefetch-state false` (prefetch has a bug with missing storage keys)
- Tx signing must include `proof_facts` in the hash chain — standard Starknet SDKs do NOT do this
- L2 gas for proof verification is ~75M — set max to ≥117M
- The `extractor` crate requires `deps/sequencer/` to exist
