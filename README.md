# IRONCLAD VAULT (Backend)

## Project Title & Overview

**IRONCLAD VAULT** is a decentralized, non-custodial inheritance and vesting protocol for Bitcoin, built on the Internet Computer (ICP). It enables users to create time-locked Bitcoin vaults (via ckBTC) with an integrated **Dead Man Switch** for automated inheritance, **Digital Wills** for encrypted message passing, and a **Zero-Coupon Bond Marketplace** for liquidity.

**Key Technologies:**
*   **Internet Computer (ICP)**: Infinite scaling and chain-key cryptography.
*   **Rust**: High-performance system programming for canister logic.
*   **ckBTC**: 1:1 Bitcoin twin on ICP for fast, low-cost transactions.
*   **Threshold ECDSA**: Decentralized signing for Bitcoin transactions.
*   **VetKeys (Planned)**: For advanced encryption of Digital Wills.

## Architecture

### Core Canisters / Smart Contracts

*   **`src/ironclad_vault_backend/src/lib.rs`**
    *   **Responsibility:** The primary entry point and logic core of the canister. It manages the global state, handles all query and update calls, implements the vault state machine (Pending -> Locked -> Unlockable/Withdrawn), executes the Dead Man Switch logic, and interfaces with the ckBTC ledger.

### Types & Helpers

*   **`src/ironclad_vault_backend/src/lib.rs`** (Inline definitions)
    *   **`Vault`**: Struct defining the vault properties (owner, balance, lock time, beneficiary).
    *   **`NetworkMode`**: Enum (`Mock`, `CkBTCTestnet`, `CkBTCMainnet`) controlling the environment behavior.
    *   **`VaultStatus`**: Enum tracking the lifecycle of a vault.
    *   **`AutoReinvestConfig`**: Configuration struct for the auto-roll strategy.
    *   **`MarketListing`**: Struct for the zero-coupon bond marketplace listings.

### Interfaces

*   **`src/ironclad_vault_backend/ironclad_vault_backend.did`**
    *   **Responsibility:** Defines the Candid interface (IDL) for the canister, specifying all public types, queries, and update methods available to frontend clients and other canisters.

## Operations & Logic Methods

### Queries (Read)

These methods fetch data without modifying the state. All implemented in `src/ironclad_vault_backend/src/lib.rs`.

*   **`get_my_vaults`**: Returns all vaults owned by the caller.
*   **`get_vault(id)`**: Returns details of a specific vault.
*   **`get_vault_events(id)`**: Retrieves the audit trail/history for a vault.
*   **`is_vault_unlockable(id)`**: Checks if the time-lock has expired.
*   **`get_unlockable_vaults`**: Lists all vaults ready for unlock.
*   **`preview_withdraw(id)`**: Simulates a withdrawal to check available balance.
*   **`get_mode_query`**: Returns the current network mode (Mock/Testnet/Mainnet).
*   **`get_deposit_proof(id)`**, **`get_withdraw_proof(id)`**: Fetches transaction proofs (placeholders).

### Updates (Write)

These methods modify the state. All implemented in `src/ironclad_vault_backend/src/lib.rs`.

*   **`create_vault`**: Initializes a new vault.
*   **`mock_deposit_vault`**: Simulates a deposit (Mock mode only).
*   **`sync_vault_balance_from_ckbtc`**: Syncs real on-chain balance from ckBTC ledger (Testnet/Mainnet).
*   **`withdraw_vault`**: Executes a real ICRC-1 transfer to withdraw funds.
*   **`unlock_vault`**: Transitions a vault from `ActiveLocked` to `Unlockable` if time valid.
*   **`ping_alive`**: Resets the Dead Man Switch timer.
*   **`claim_inheritance`**: Transfers ownership to the beneficiary if timeout exceeded.
*   **`set_mode_mock`, `set_mode_ckbtc_testnet`, `set_mode_ckbtc_mainnet`**: Admin functions to switch network environment.
*   **`create_listing`, `buy_listing`, `cancel_listing`**: Marketplace operations.
*   **`schedule_auto_reinvest`, `cancel_auto_reinvest`**: Auto-roll strategy management.

## Application Flow & Implementation

### How It Works

#### 1. Vault Creation
User initializes a vault with a lock duration and optional beneficiary.
*   **Step**: Generate ID, derive ckBTC subaccount, initialize state.
*   **Implemented in**: `create_vault` -> `src/ironclad_vault_backend/src/lib.rs`

#### 2. Deposit Process (3-Tier Network Mode)
Depending on the selected mode, the deposit flow varies:
*   **Mock Mode**:
    *   **Step**: User manually sets a fake balance.
    *   **Implemented in**: `mock_deposit_vault` -> `src/ironclad_vault_backend/src/lib.rs`
*   **Real Mode (Testnet/Mainnet)**:
    *   **Step**: User transfers ckBTC to the specific canister subaccount.
    *   **Step**: User triggers sync; Canister queries Ledger (`icrc1_balance_of`) and updates local state.
    *   **Implemented in**: `sync_vault_balance_from_ckbtc` -> `src/ironclad_vault_backend/src/lib.rs`

#### 3. Withdrawal
User retrieves funds after the lock period.
*   **Step**: Check lock status -> Execute `icrc1_transfer` on Ledger -> Update local balance -> Record Transaction ID.
*   **Implemented in**: `withdraw_vault` -> `src/ironclad_vault_backend/src/lib.rs`

#### 4. Dead Man Switch (Inheritance)
If the owner is inactive for 180 days (default).
*   **Step (Owner)**: Calls `ping_alive` to reset `last_keep_alive`.
*   **Step (Beneficiary)**: Calls `claim_inheritance` -> System checks `now > last_keep_alive + timeout` -> Transfers `owner` field to beneficiary.
*   **Implemented in**: `ping_alive`, `claim_inheritance` -> `src/ironclad_vault_backend/src/lib.rs`

#### 5. Digital Will Access
Encrypted notes are revealed only upon successful inheritance claim.
*   **Step**: Validate caller (Owner OR Beneficiary if timeout passed) -> Return `secure_key`.
*   **Implemented in**: `get_digital_will_key` -> `src/ironclad_vault_backend/src/lib.rs`

## Development & Usage

### Prerequisites
*   **DFX SDK**: `v0.15.0` or newer.
*   **Rust**: Stable toolchain.
*   **Node.js**: `v18+` (for frontend, if running full stack).

### Configuration
*   **`dfx.json`**: Configures the canister networks and dependencies (e.g., ckBTC ledger).

### CLI Commands

**1. Deploy Locally (Mock Mode)**
```bash
# Start local replica
dfx start --background --clean

# Deploy canisters (Backend + Frontend)
dfx deploy
```

**2. Run Tests**
Currently, testing is done via `cargo test` or manual interaction via Candid UI.
```bash
cargo test --package ironclad_vault_backend
```

## Project Structure

```text
ironclad_vault/
├── dfx.json                        # Network & Canister Config
├── Cargo.toml                      # Rust Workspace Config
└── src/
    └── ironclad_vault_backend/     # MAIN BACKEND CANISTER
        ├── Cargo.toml
        ├── ironclad_vault_backend.did  # Interface Definition
        └── src/
            └── lib.rs              # CORE LOGIC (State, API, Flows)
```

## Current Status & Integrations

*   **Status**: Alpha / Hackathon Ready.
*   **Integrations**:
    *   **Internet Identity**: For user authentication.
    *   **ICRC-1 Ledger**: Supports ckBTC (Mainnet) and ckTESTBTC (Testnet).
    *   **Threshold ECDSA**: Implemented for signature requests.
    *   **Network Modes**: Fully supported (Mock, Testnet, Mainnet).
