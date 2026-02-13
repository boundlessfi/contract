# Boundless Platform Smart Contracts

A unified suite of Soroban smart contracts for the Boundless ecosystem, supporting bounties, grants, hackathons, and crowdfunding with integrated reputation and governance.

## Project Structure

- **Shared Infrastructure**: Core contracts providing escrow, reputation tracking, project registry, payment routing, and voting logic.
- **Module Registries**: Specific implementations for different work modules (Bounties, Grants, Hackathons, Crowdfunding) that utilize the shared infrastructure.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)
- [Stellar CLI](https://developers.stellar.org/docs/tools/stellar-cli/install)

### Development

#### Build Contracts

To build all contracts in the workspace:

```bash
stellar contract build
```

#### Build Optimized

To build optimized WASM for deployment:

```bash
stellar contract build --optimize
```

#### Run Tests

To run the full test suite:

```bash
cargo test
```

## Deployed Contracts (Testnet)

Public contract IDs for the latest Testnet deployment can be found in `deployed_contracts_testnet.txt`.
