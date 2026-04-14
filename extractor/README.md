# Virtual OS Extractor

A small Rust utility that extracts the compiled virtual OS program from the `apollo_starknet_os_program` crate into a standalone JSON file.

## Purpose

The SNIP-36 proving pipeline requires the virtual OS program as a JSON file. This program is embedded as a byte constant (`VIRTUAL_OS_PROGRAM_BYTES`) inside the `apollo_starknet_os_program` crate. The extractor pulls it out so it can be fed to the prover tooling.

## Prerequisites

- Rust (edition 2021)
- The `deps/sequencer` dependency must be cloned and available (run `scripts/setup.sh` first)

## Build

```bash
cargo build --release -p virtual-os-extractor
```

## Usage

```bash
virtual-os-extractor <output-path>
```

Example:

```bash
./target/release/virtual-os-extractor output/virtual_os_program.json
```

This writes the virtual OS program JSON to the specified path, creating parent directories if needed.
