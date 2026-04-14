# SNIP-36 E2E Test Suite

End-to-end test notes for the full SNIP-36 virtual block pipeline against the Starknet Sepolia test environment.

## Test Flow

```
1. Import funded account into sncast
2. Compile + declare + deploy minimal Cairo counter contract (scarb/sncast)
3. Wait for deploy tx inclusion
4. For each SNOS block:
   a. Construct and sign an invoke transaction (increment)
   b. Prove via virtual OS (starknet_os_runner + stwo prover)
   c. Sign tx with proof_facts-inclusive hash and submit via RPC
   d. Wait for tx inclusion, verify counter state on-chain
5. Final counter verification
```

## Prerequisites

- `scarb` — contract compilation
- `sncast` — starknet-foundry (declare/deploy/invoke)
- backend built (`cargo build --release -p snip36-playground`)
- prover + runner dependencies prepared separately for this trimmed copy

## Environment Variables

| Variable | Default | Required |
|----------|---------|----------|
| `STARKNET_RPC_URL` | (see .env) | Yes |
| `STARKNET_ACCOUNT_ADDRESS` | — | Yes |
| `STARKNET_PRIVATE_KEY` | — | Yes |
| `STARKNET_CHAIN_ID` | `SN_SEPOLIA` | No |
## Files

| File | Description |
|------|-------------|
| `contracts/` | Minimal Cairo counter contract (Scarb project) |
| `contracts/src/lib.cairo` | Counter contract: `increment(amount)` + `get_counter()` |

Supporting logic for proving, signing, and submission lives in the backend/app crates:

| Crate | Description |
|-------|-------------|
| `crates/snip36-core/src/signing.rs` | Proof_facts-inclusive Poseidon tx hash + signing |
| `crates/snip36-core/src/rpc.rs` | Starknet RPC client (tx polling, calls) |

## Proof Format

The DEMO-19 runner + stwo prover outputs proofs in **binary format** (`ProofFormat::Binary`):

1. Prover: `CairoProofForRustVerifier` → `bincode::serialize` → bzip2 → file
2. Runner: decompresses → encodes to `Vec<u32>` (BE + padding prefix) → base64 string
3. The proof is returned as a base64 string in the JSON-RPC response

The `proof_facts` are a JSON array of hex felt values containing:
- `PROOF0` marker
- `VIRTUAL_SNOS` marker
- Virtual OS program hash
- `VIRTUAL_SNOS0` marker
- Block number, block hash, OS config hash
- L2→L1 message count and hashes

## Transaction Signing

Proof-bearing transactions require the `proof_facts` to be included in the Poseidon transaction hash chain. Standard Starknet SDKs (starknet-py, starknet.js) do **not** include this, producing an incorrect hash and "invalid signature" errors.

The project handles this via `snip36_core::signing`, which computes the correct hash.

## CI

A daily health check runs via GitHub Actions (`.github/workflows/daily-health.yml`).
