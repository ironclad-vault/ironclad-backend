// src/ironclad_vault_backend/src/lib.rs

use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::api::{canister_self, msg_caller, time};
use ic_cdk_macros::{init, query, update};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;

// =======================
// Constants
// =======================

// ckBTC / ckTESTBTC ledger canister IDs (from official ICP docs)
// Mainnet ckBTC ledger:      mxzaz-hqaaa-aaaar-qaada-cai
// Testnet4 ckTESTBTC ledger: g4xu7-jiaaa-aaaan-aaaaq-cai
const CKBTC_LEDGER_CANISTER_ID: &str = "g4xu7-jiaaa-aaaan-aaaaq-cai";
// NOTE: swap to mxzaz-hqaaa-aaaar-qaada-cai when pointing to mainnet ckBTC.

// Local dfx mock ledger (will be set during init)
thread_local! {
    static LEDGER_CANISTER_ID: RefCell<Option<String>> = RefCell::new(None);
}

// ECDSA key IDs for threshold signing
// Local dfx replica: "dfx_test_key"
// Testnet/Mainnet testing: "test_key_1"
// Production (DO NOT USE in hackathon): "key_1"
const ECDSA_KEY_NAME: &str = "test_key_1";

// =======================
// Types
// =======================

// Helper type for ICRC-1 account (used for ledger calls)
#[derive(CandidType, Deserialize, Clone)]
struct Icrc1Account {
    owner: Principal,
    subaccount: Option<Vec<u8>>,
}

// ECDSA types (matching management canister interface)
#[derive(CandidType, Deserialize)]
struct EcdsaKeyId {
    curve: EcdsaCurve,
    name: String,
}

#[derive(CandidType, Deserialize)]
enum EcdsaCurve {
    #[serde(rename = "secp256k1")]
    Secp256k1,
}

#[derive(CandidType, Deserialize)]
struct SignWithEcdsaArgument {
    message_hash: Vec<u8>,
    derivation_path: Vec<Vec<u8>>,
    key_id: EcdsaKeyId,
}

#[derive(CandidType, Deserialize)]
struct SignWithEcdsaResponse {
    signature: Vec<u8>,
}

#[derive(Clone, CandidType, Deserialize)]
pub enum NetworkMode {
    Mock,
    CkBTCMainnet,
}

#[derive(Clone, CandidType, Deserialize)]
pub enum VaultStatus {
    PendingDeposit,
    ActiveLocked,
    Unlockable,
    Withdrawn,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct Vault {
    pub id: u64,
    pub owner: Principal,

    // BTC / ckBTC routing
    pub btc_address: String,
    pub ckbtc_subaccount: Option<Vec<u8>>, // NEW: ckBTC subaccount for this vault
    pub expected_deposit: u64,             // in satoshis (can be 0 for now)
    pub btc_deposit_txid: Option<String>,
    pub btc_withdraw_txid: Option<String>,

    // Timelock & balance
    pub lock_until: u64, // unix timestamp (seconds)
    pub status: VaultStatus,
    pub balance: u64, // current balance (dummy / real later)

    // === INHERITANCE PROTOCOL (Dead Man Switch) ===
    pub beneficiary: Option<Principal>, // designated heir
    pub last_keep_alive: u64,           // timestamp of last owner activity
    pub inheritance_timeout: u64,       // seconds of inactivity before claim (default: 180 days)

    // === DIGITAL WILL (Encrypted Message) ===
    pub encrypted_note: Option<String>, // Hex-encoded ciphertext for Digital Will
    pub secure_key: Option<String>,     // Decryption key (PRIVATE - DO NOT EXPOSE)

    // Metadata
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct VaultEvent {
    pub vault_id: u64,
    pub action: String, // e.g. "VAULT_CREATED", "MOCK_DEPOSIT"
    pub timestamp: u64,
    pub notes: String,
}

#[derive(Clone, CandidType, Deserialize)]
pub enum AutoReinvestPlanStatus {
    Active,
    Cancelled,
    Error,
    Paused,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct AutoReinvestConfig {
    pub vault_id: u64,
    pub owner: Principal,
    pub new_lock_duration: u64,
    pub enabled: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub plan_status: AutoReinvestPlanStatus,
    pub error_message: Option<String>,
    pub next_cycle_timestamp: u64,
    pub execution_count: u64,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct PlanStatusResponse {
    pub plan_status: AutoReinvestPlanStatus,
    pub error_message: Option<String>,
    pub next_cycle_timestamp: u64,
    pub execution_count: u64,
}

#[derive(Clone, CandidType, Deserialize)]
pub enum ListingStatus {
    Active,
    Cancelled,
    Filled,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct MarketListing {
    pub id: u64,
    pub vault_id: u64,
    pub seller: Principal,
    pub buyer: Option<Principal>,
    pub price_sats: u64,
    pub status: ListingStatus,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct CkbtcSyncResult {
    pub vault: Vault,
    pub synced_balance: u64,
    pub mode: NetworkMode,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct BitcoinTxProof {
    pub txid: String,
    pub confirmed: bool,
    pub confirmations: u32,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct SignatureResponse {
    pub message: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Default)]
pub struct State {
    pub next_id: u64,
    pub user_vaults: BTreeMap<Principal, Vec<Vault>>, // Stores vaults by Owner
    pub vault_index: BTreeMap<u64, Principal>,        // Maps VaultID -> Owner (for quick lookup)
    pub history: Vec<VaultEvent>,
    pub auto_reinvest: Vec<AutoReinvestConfig>,
    pub listings: Vec<MarketListing>,
    pub next_listing_id: u64,
}

// =======================
// Global state (in-memory)
// =======================

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static MODE: RefCell<NetworkMode> = RefCell::new(NetworkMode::Mock);
}

fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| {
        let state = s.borrow();
        f(&state)
    })
}

fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        f(&mut state)
    })
}

fn now_sec() -> u64 {
    time() / 1_000_000_000
}

fn get_mode() -> NetworkMode {
    MODE.with(|m| m.borrow().clone())
}

fn ecdsa_key_id() -> EcdsaKeyId {
    EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: ECDSA_KEY_NAME.to_string(),
    }
}

fn record_event(vault_id: u64, action: &str, notes: &str) {
    let ts = now_sec();
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.history.push(VaultEvent {
            vault_id,
            action: action.to_string(),
            timestamp: ts,
            notes: notes.to_string(),
        });
    });
}

// =======================
// Helper Methods (Internal Logic)
// =======================

/// Internal helper: Get mutable reference to a vault by ID
/// Returns None if vault doesn't exist or caller doesn't own it
fn _get_vault_mut<'a>(
    state: &'a mut State,
    vault_id: u64,
    caller: Principal,
) -> Option<&'a mut Vault> {
    // Find owner from vault_index
    let owner = state.vault_index.get(&vault_id)?;

    // Verify caller owns the vault
    if *owner != caller {
        return None;
    }

    // Get mutable reference to user's vault list
    let vaults = state.user_vaults.get_mut(owner)?;

    // Find the specific vault in the vector by ID
    vaults.iter_mut().find(|v| v.id == vault_id)
}

/// Internal helper: Get immutable reference to a vault by ID
/// Returns None if vault doesn't exist or caller doesn't own it
fn _get_vault<'a>(state: &'a State, vault_id: u64, caller: Principal) -> Option<&'a Vault> {
    // Find owner from vault_index
    let owner = state.vault_index.get(&vault_id)?;

    // Verify caller owns the vault
    if *owner != caller {
        return None;
    }

    // Get reference to user's vault list
    let vaults = state.user_vaults.get(owner)?;

    // Find the specific vault in the vector by ID
    vaults.iter().find(|v| v.id == vault_id)
}

/// Internal helper: Transfer vault ownership to a new owner
/// This safely moves the vault from old owner's list to new owner's list
fn _transfer_vault(state: &mut State, vault_id: u64, new_owner: Principal) -> Result<(), String> {
    // Find old owner from vault_index
    let old_owner = state
        .vault_index
        .get(&vault_id)
        .cloned()
        .ok_or("Vault not found")?;

    // Remove vault from old owner's list
    let mut vault = {
        let old_vaults = state
            .user_vaults
            .get_mut(&old_owner)
            .ok_or("Old owner's vault list not found")?;

        let vault_pos = old_vaults
            .iter()
            .position(|v| v.id == vault_id)
            .ok_or("Vault not found in old owner's list")?;

        old_vaults.remove(vault_pos)
    };

    // Update vault data
    let ts = now_sec();
    vault.owner = new_owner;
    vault.beneficiary = None; // Security: reset beneficiary on ownership transfer
    vault.last_keep_alive = ts; // Reset keep-alive timer for new owner
    vault.updated_at = ts;

    // Add vault to new owner's list
    state.user_vaults.entry(new_owner).or_default().push(vault);

    // Update vault_index to point vault_id -> new_owner
    state.vault_index.insert(vault_id, new_owner);

    Ok(())
}

// =======================
// Lifecycle
// =======================

#[init]
fn init() {
    // For local dfx testing, default to mock_ledger if it exists.
    // For production/testnet, this will be overridden by the explicit canister ID.
    // The mock ledger will be deployed as the first canister in dfx.json
    LEDGER_CANISTER_ID.with(|id| {
        *id.borrow_mut() = Some(CKBTC_LEDGER_CANISTER_ID.to_string());
    });
}

// =======================
// Public methods
// =======================

/// Create a new vault with a lock_until time, optional expected_deposit, optional beneficiary, optional encrypted Digital Will note, and optional decryption key.
/// For now btc_address is a placeholder string; later we'll plug real BTC.
#[update]
fn create_vault(
    lock_until: u64,
    expected_deposit: u64,
    beneficiary: Option<Principal>,
    encrypted_note: Option<String>,
    secure_key: Option<String>,
) -> Vault {
    let caller = msg_caller();
    let ts = now_sec();

    let vault = with_state_mut(|state| {
        let id = state.next_id;
        state.next_id += 1;

        // TODO: replace placeholder with real BTC address derivation
        let btc_address = format!("IRONCLAD-VAULT-{}", id);

        // Generate deterministic ckBTC subaccount based on vault id
        let mut sub = vec![0u8; 32];
        sub[0..8].copy_from_slice(&id.to_be_bytes());

        let vault = Vault {
            id,
            owner: caller,
            btc_address,
            ckbtc_subaccount: Some(sub),
            expected_deposit,
            btc_deposit_txid: None,
            btc_withdraw_txid: None,
            lock_until,
            status: VaultStatus::PendingDeposit,
            balance: 0,
            beneficiary,                     // Set from argument
            last_keep_alive: ts,             // Initialize to now
            inheritance_timeout: 15_552_000, // Default 180 days (6 months) in seconds
            encrypted_note,                  // Digital Will encrypted message
            secure_key,                      // Decryption key (stored privately)
            created_at: ts,
            updated_at: ts,
        };

        // Add vault to user's vault list
        state
            .user_vaults
            .entry(caller)
            .or_default()
            .push(vault.clone());

        // Add vault ID to index
        state.vault_index.insert(id, caller);

        vault
    });

    record_event(
        vault.id,
        "VAULT_CREATED",
        "Vault created in PendingDeposit state",
    );
    vault
}

/// Get all vaults owned by the caller.
/// SECURITY: secure_key is sanitized (set to None) before returning.
#[query]
fn get_my_vaults() -> Vec<Vault> {
    let caller = msg_caller();
    with_state(|state| {
        state
            .user_vaults
            .get(&caller)
            .map(|vaults| {
                vaults
                    .iter()
                    .map(|v| {
                        let mut sanitized = v.clone();
                        sanitized.secure_key = None; // CRITICAL: Never expose secure_key
                        sanitized
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Get a single vault by id, only if owned by caller.
/// SECURITY: secure_key is sanitized (set to None) before returning.
#[query]
fn get_vault(id: u64) -> Option<Vault> {
    let caller = msg_caller();
    with_state(|state| {
        _get_vault(state, id, caller).map(|v| {
            let mut sanitized = v.clone();
            sanitized.secure_key = None; // CRITICAL: Never expose secure_key
            sanitized
        })
    })
}

/// Get history events for a given vault id (owned by caller).
#[query]
fn get_vault_events(id: u64) -> Vec<VaultEvent> {
    let caller = msg_caller();
    with_state(|state| {
        // Ensure caller owns the vault before showing history
        let owns = state
            .vault_index
            .get(&id)
            .map(|owner| *owner == caller)
            .unwrap_or(false);

        if !owns {
            return Vec::new();
        }

        state
            .history
            .iter()
            .filter(|e| e.vault_id == id)
            .cloned()
            .collect()
    })
}

/// Dummy deposit function: simulates a deposit and activates the lock.
///
/// This is TEMPORARY for UI + flow testing. Later we will replace this
/// with real BTC / ckBTC integration.
#[update]
fn mock_deposit_vault(id: u64, amount: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        let vault = _get_vault_mut(state, id, caller).ok_or("Vault not found or unauthorized")?;

        if amount == 0 {
            return Err("Deposit amount must be greater than 0".to_string());
        }

        vault.balance = amount;
        vault.status = VaultStatus::ActiveLocked;
        vault.updated_at = ts;

        Ok(vault.clone())
    });

    if let Ok(ref _v) = result {
        record_event(
            id,
            "MOCK_DEPOSIT",
            &format!("Mock deposit of {} satoshis", amount),
        );
        record_event(id, "LOCK_STARTED", "Vault moved to ActiveLocked");
    }

    result
}

/// Check if a vault's timelock has expired and can be unlocked.
#[query]
fn is_vault_unlockable(id: u64) -> Result<bool, String> {
    let caller = msg_caller();
    let now = now_sec();

    with_state(|state| {
        let vault = _get_vault(state, id, caller).ok_or("Vault not found or unauthorized")?;

        match vault.status {
            VaultStatus::ActiveLocked if now >= vault.lock_until => Ok(true),
            _ => Ok(false),
        }
    })
}

/// Unlock a vault after the timelock has expired.
#[update]
fn unlock_vault(id: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        let vault = _get_vault_mut(state, id, caller).ok_or("Vault not found or unauthorized")?;

        match vault.status {
            VaultStatus::ActiveLocked => {
                if ts < vault.lock_until {
                    return Err(format!(
                        "Vault still locked until {}. Current time: {}",
                        vault.lock_until, ts
                    ));
                }
                // Timelock expired, can unlock
                vault.status = VaultStatus::Unlockable;
                vault.updated_at = ts;
                Ok(vault.clone())
            }
            VaultStatus::Unlockable => Err("Vault is already unlocked".to_string()),
            VaultStatus::PendingDeposit => Err("Vault must be locked before unlocking".to_string()),
            VaultStatus::Withdrawn => Err("Vault has already been withdrawn".to_string()),
        }
    });

    if let Ok(ref _v) = result {
        record_event(
            id,
            "UNLOCK_READY",
            &format!("Vault unlocked after timelock expired at {}", ts),
        );
    }

    result
}

/// Get all unlockable vaults for the caller (timelock expired).
/// SECURITY: secure_key is sanitized (set to None) before returning.
#[query]
fn get_unlockable_vaults() -> Vec<Vault> {
    let caller = msg_caller();
    let now = now_sec();

    with_state(|state| {
        state
            .user_vaults
            .get(&caller)
            .map(|vaults| {
                vaults
                    .iter()
                    .filter(|v| {
                        matches!(v.status, VaultStatus::ActiveLocked) && now >= v.lock_until
                    })
                    .map(|v| {
                        let mut sanitized = v.clone();
                        sanitized.secure_key = None; // CRITICAL: Never expose secure_key
                        sanitized
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Preview withdraw amount available for a vault (no mutation).
#[query]
fn preview_withdraw(id: u64) -> Result<u64, String> {
    let caller = msg_caller();
    with_state(|state| {
        let vault = _get_vault(state, id, caller).ok_or("Vault not found or unauthorized")?;

        if !matches!(vault.status, VaultStatus::Unlockable) {
            return Err("Vault is not unlockable".to_string());
        }

        if vault.balance == 0 {
            return Err("No balance to withdraw".to_string());
        }

        Ok(vault.balance)
    })
}

/// Withdraw from a vault (mock flow), updates state and logs events.
#[update]
fn withdraw_vault(id: u64, amount: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        let vault = _get_vault_mut(state, id, caller).ok_or("Vault not found or unauthorized")?;

        if !matches!(vault.status, VaultStatus::Unlockable) {
            return Err("Vault is not unlockable".to_string());
        }

        if amount == 0 {
            return Err("Withdraw amount must be greater than 0".to_string());
        }

        if amount > vault.balance {
            return Err("Withdraw amount exceeds balance".to_string());
        }

        // Apply withdrawal
        vault.balance -= amount;
        if vault.balance == 0 {
            vault.status = VaultStatus::Withdrawn;
        }
        let txid = format!("MOCK-TXID-{}", id);
        vault.btc_withdraw_txid = Some(txid.clone());
        vault.updated_at = ts;

        Ok((vault.clone(), txid))
    });

    if let Ok((ref _v, ref txid)) = result {
        record_event(id, "WITHDRAW_REQUESTED", &format!("Requested withdraw"));
        record_event(id, "WITHDRAW_COMPLETED", &format!("Withdraw txid {}", txid));
    }

    result.map(|(v, _)| v)
}

/// Get all withdrawable vaults for the caller (Unlockable and balance > 0).
/// SECURITY: secure_key is sanitized (set to None) before returning.
#[query]
fn get_withdrawable_vaults() -> Vec<Vault> {
    let caller = msg_caller();
    with_state(|state| {
        state
            .user_vaults
            .get(&caller)
            .map(|vaults| {
                vaults
                    .iter()
                    .filter(|v| matches!(v.status, VaultStatus::Unlockable) && v.balance > 0)
                    .map(|v| {
                        let mut sanitized = v.clone();
                        sanitized.secure_key = None; // CRITICAL: Never expose secure_key
                        sanitized
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

// =======================
// Inheritance Protocol (Dead Man Switch)
// =======================

/// Ping alive to reset the dead man switch timer.
/// Owner must call this periodically to prevent beneficiary from claiming.
#[update]
fn ping_alive(vault_id: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        let vault =
            _get_vault_mut(state, vault_id, caller).ok_or("Vault not found or unauthorized")?;

        vault.last_keep_alive = ts; // Reset timer
        vault.updated_at = ts;
        Ok(vault.clone())
    });

    if let Ok(ref _v) = result {
        record_event(
            vault_id,
            "PING_ALIVE",
            &format!("Owner pinged alive, reset dead man switch timer"),
        );
    }

    result
}

/// Claim inheritance after owner has been inactive for the timeout period.
/// Only the designated beneficiary can call this.
#[update]
fn claim_inheritance(vault_id: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        // Step 1: Look up the vault owner from the index
        let old_owner = state
            .vault_index
            .get(&vault_id)
            .cloned()
            .ok_or("Vault not found")?;

        // Step 2: Get the vault from the owner's list
        let vaults = state
            .user_vaults
            .get(&old_owner)
            .ok_or("Vault owner not found")?;

        let vault = vaults
            .iter()
            .find(|v| v.id == vault_id)
            .ok_or("Vault not found in owner's list")?;

        // Step 3: Validate beneficiary - MUST be the caller
        if vault.beneficiary != Some(caller) {
            return Err("Not the beneficiary".to_string());
        }

        // Step 4: Validate timeout (Dead Man Switch)
        if ts < vault.last_keep_alive + vault.inheritance_timeout {
            return Err("Owner is still considered active".to_string());
        }

        // Step 5: All validations passed - transfer ownership
        _transfer_vault(state, vault_id, caller)?;

        // Step 6: Get the vault after transfer to return it
        // Now we can safely use _get_vault since caller is the new owner
        let vault = _get_vault(state, vault_id, caller).ok_or("Vault not found after transfer")?;

        Ok((vault.clone(), old_owner))
    });

    if let Ok((ref _v, old_owner)) = result {
        record_event(
            vault_id,
            "INHERITANCE_CLAIMED",
            &format!(
                "Vault ownership transferred from {} to beneficiary via inheritance",
                old_owner
            ),
        );
    }

    result.map(|(v, _)| v)
}

// =======================
// Auto-Reinvest System
// =======================

/// Schedule auto-reinvest for a vault with a new lock duration.
#[update]
fn schedule_auto_reinvest(
    vault_id: u64,
    new_lock_duration: u64,
) -> Result<AutoReinvestConfig, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        // Find and validate vault using helper
        let vault = _get_vault(state, vault_id, caller).ok_or("Vault not found or unauthorized")?;

        // Reject if vault is already withdrawn
        if matches!(vault.status, VaultStatus::Withdrawn) {
            return Err("Cannot schedule auto-reinvest for withdrawn vault".to_string());
        }

        // Reject if new_lock_duration is 0
        if new_lock_duration == 0 {
            return Err("Lock duration must be greater than 0".to_string());
        }

        // Check if config already exists
        if let Some(existing) = state
            .auto_reinvest
            .iter_mut()
            .find(|c| c.vault_id == vault_id && c.owner == caller)
        {
            // Update existing config
            existing.new_lock_duration = new_lock_duration;
            existing.enabled = true;
            existing.updated_at = ts;
            existing.plan_status = AutoReinvestPlanStatus::Active;
            existing.error_message = None;
            existing.next_cycle_timestamp = ts + new_lock_duration;
            Ok(existing.clone())
        } else {
            // Create new config
            let config = AutoReinvestConfig {
                vault_id,
                owner: caller,
                new_lock_duration,
                enabled: true,
                created_at: ts,
                updated_at: ts,
                plan_status: AutoReinvestPlanStatus::Active,
                error_message: None,
                next_cycle_timestamp: ts + new_lock_duration,
                execution_count: 0,
            };
            state.auto_reinvest.push(config.clone());
            Ok(config)
        }
    });

    if let Ok(ref _config) = result {
        record_event(
            vault_id,
            "AUTO_REINVEST_SCHEDULED",
            &format!(
                "Auto-reinvest scheduled with lock duration {} seconds",
                new_lock_duration
            ),
        );
    }

    result
}

/// Cancel auto-reinvest for a vault.
#[update]
fn cancel_auto_reinvest(vault_id: u64) -> Result<(), String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        // Find active config
        let config = match state
            .auto_reinvest
            .iter_mut()
            .find(|c| c.vault_id == vault_id && c.owner == caller && c.enabled)
        {
            Some(c) => c,
            None => {
                return Err(
                    "No active auto-reinvest config for this vault or unauthorized".to_string(),
                )
            }
        };

        // Disable config and update status
        config.enabled = false;
        config.updated_at = ts;
        config.plan_status = AutoReinvestPlanStatus::Cancelled;
        config.error_message = None;
        config.next_cycle_timestamp = 0;
        Ok(())
    });

    if result.is_ok() {
        record_event(
            vault_id,
            "AUTO_REINVEST_CANCELLED",
            "Auto-reinvest configuration cancelled",
        );
    }

    result
}

/// Get auto-reinvest config for a specific vault.
#[query]
fn get_auto_reinvest_config(vault_id: u64) -> Option<AutoReinvestConfig> {
    let caller = msg_caller();
    with_state(|state| {
        state
            .auto_reinvest
            .iter()
            .find(|c| c.vault_id == vault_id && c.owner == caller)
            .cloned()
    })
}

/// Get all auto-reinvest configs for the caller.
#[query]
fn get_my_auto_reinvest_configs() -> Vec<AutoReinvestConfig> {
    let caller = msg_caller();
    with_state(|state| {
        state
            .auto_reinvest
            .iter()
            .filter(|c| c.owner == caller)
            .cloned()
            .collect()
    })
}

/// Execute auto-reinvest: withdraw from source vault and create new locked vault.
#[update]
fn execute_auto_reinvest(vault_id: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    // First validate that config exists
    let config_exists = with_state(|state| {
        state
            .auto_reinvest
            .iter()
            .any(|c| c.vault_id == vault_id && c.owner == caller && c.enabled)
    });

    if !config_exists {
        return Err("No active auto-reinvest config for this vault or unauthorized".to_string());
    }

    let result = with_state_mut(|state| {
        // Find active auto-reinvest config
        let config = match state
            .auto_reinvest
            .iter()
            .find(|c| c.vault_id == vault_id && c.owner == caller && c.enabled)
        {
            Some(c) => c.clone(),
            None => {
                return Err(
                    "No active auto-reinvest config for this vault or unauthorized".to_string(),
                )
            }
        };

        // Find source vault using helper
        let source_vault =
            _get_vault_mut(state, vault_id, caller).ok_or("Vault not found or unauthorized")?;

        // Validate source vault status
        if !matches!(source_vault.status, VaultStatus::Unlockable) {
            return Err("Source vault must be unlocked before executing auto-reinvest".to_string());
        }

        // Validate source vault has balance
        if source_vault.balance == 0 {
            return Err("Source vault has no balance to reinvest".to_string());
        }

        let old_balance = source_vault.balance;

        // Update source vault: withdraw all funds
        source_vault.balance = 0;
        source_vault.status = VaultStatus::Withdrawn;
        source_vault.btc_withdraw_txid = Some(format!("REINVEST-INTERNAL-{}", vault_id));
        source_vault.updated_at = ts;

        // Create new vault with reinvested funds
        let new_id = state.next_id;
        state.next_id += 1;

        // Generate ckBTC subaccount for new vault
        let mut sub = vec![0u8; 32];
        sub[0..8].copy_from_slice(&new_id.to_be_bytes());

        let new_vault = Vault {
            id: new_id,
            owner: caller,
            btc_address: format!("IRONCLAD-VAULT-{}", new_id),
            ckbtc_subaccount: Some(sub),
            expected_deposit: old_balance,
            btc_deposit_txid: None,
            btc_withdraw_txid: None,
            lock_until: ts + config.new_lock_duration,
            status: VaultStatus::ActiveLocked,
            balance: old_balance,
            beneficiary: None,   // No beneficiary for auto-reinvested vaults
            last_keep_alive: ts, // Initialize to now
            inheritance_timeout: 15_552_000, // Default 180 days
            encrypted_note: None, // No Digital Will for auto-created vaults
            secure_key: None,    // No decryption key for auto-created vaults
            created_at: ts,
            updated_at: ts,
        };

        // Add new vault to user's vault list
        state
            .user_vaults
            .entry(caller)
            .or_default()
            .push(new_vault.clone());

        // Add new vault ID to index
        state.vault_index.insert(new_id, caller);

        // Update the auto-reinvest config - keep it Active and increment counter
        if let Some(cfg) = state
            .auto_reinvest
            .iter_mut()
            .find(|c| c.vault_id == vault_id && c.owner == caller)
        {
            cfg.execution_count += 1;
            cfg.plan_status = AutoReinvestPlanStatus::Active;
            cfg.error_message = None;
            cfg.next_cycle_timestamp = ts + cfg.new_lock_duration;
            cfg.updated_at = ts;
        }

        Ok(new_vault)
    });

    match &result {
        Ok(new_vault) => {
            record_event(
                vault_id,
                "AUTO_REINVEST_EXECUTED_SOURCE",
                &format!("Source vault withdrawn for reinvestment"),
            );
            record_event(
                new_vault.id,
                "AUTO_REINVEST_EXECUTED_TARGET",
                &format!(
                    "New vault created from auto-reinvest with balance {}",
                    new_vault.balance
                ),
            );
        }
        Err(error_msg) => {
            // Set error status on the config
            with_state_mut(|state| {
                if let Some(cfg) = state
                    .auto_reinvest
                    .iter_mut()
                    .find(|c| c.vault_id == vault_id && c.owner == caller)
                {
                    cfg.plan_status = AutoReinvestPlanStatus::Error;
                    cfg.error_message = Some(error_msg.clone());
                    cfg.updated_at = ts;
                }
            });
            record_event(
                vault_id,
                "AUTO_REINVEST_ERROR",
                &format!("Auto-reinvest failed: {}", error_msg),
            );
        }
    }

    result
}

/// Get plan status for a vault's auto-reinvest configuration.
#[query]
fn get_plan_status(vault_id: u64) -> Result<PlanStatusResponse, String> {
    let caller = msg_caller();
    with_state(|state| {
        let config = match state
            .auto_reinvest
            .iter()
            .find(|c| c.vault_id == vault_id && c.owner == caller)
        {
            Some(c) => c,
            None => {
                return Err(
                    "No auto-reinvest config found for this vault or unauthorized".to_string(),
                )
            }
        };

        Ok(PlanStatusResponse {
            plan_status: config.plan_status.clone(),
            error_message: config.error_message.clone(),
            next_cycle_timestamp: config.next_cycle_timestamp,
            execution_count: config.execution_count,
        })
    })
}

/// Retry a failed auto-reinvest plan.
#[update]
fn retry_failed_plan(vault_id: u64) -> Result<AutoReinvestConfig, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        let config = match state
            .auto_reinvest
            .iter_mut()
            .find(|c| c.vault_id == vault_id && c.owner == caller)
        {
            Some(c) => c,
            None => {
                return Err(
                    "No auto-reinvest config found for this vault or unauthorized".to_string(),
                )
            }
        };

        // Validate plan is in Error state
        if !matches!(config.plan_status, AutoReinvestPlanStatus::Error) {
            return Err("Plan must be in Error state to retry".to_string());
        }

        // Reset to Active status
        config.plan_status = AutoReinvestPlanStatus::Active;
        config.error_message = None;
        config.next_cycle_timestamp = ts + config.new_lock_duration;
        config.updated_at = ts;

        Ok(config.clone())
    });

    if let Ok(ref _config) = result {
        record_event(
            vault_id,
            "AUTO_REINVEST_RETRY",
            "Auto-reinvest plan reset to Active after error",
        );
    }

    result
}

// =======================
// Marketplace System
// =======================

/// Create a listing to sell a vault on the marketplace.
#[update]
fn create_listing(vault_id: u64, price_sats: u64) -> Result<MarketListing, String> {
    let caller = msg_caller();
    let ts = now_sec();

    if price_sats == 0 {
        return Err("Price must be greater than 0".to_string());
    }

    let result = with_state_mut(|state| {
        // Find and validate vault using helper
        let vault = _get_vault(state, vault_id, caller).ok_or("Vault not found or unauthorized")?;

        // Validate vault status
        if matches!(vault.status, VaultStatus::PendingDeposit) {
            return Err("Cannot list vault in PendingDeposit status".to_string());
        }
        if matches!(vault.status, VaultStatus::Withdrawn) {
            return Err("Cannot list withdrawn vault".to_string());
        }

        // Bond validation: price must be lower than balance to ensure positive yield
        if price_sats >= vault.balance {
            return Err("Price must be lower than balance (Bond Yield required)".to_string());
        }

        // Check for existing active listing
        if state
            .listings
            .iter()
            .any(|l| l.vault_id == vault_id && matches!(l.status, ListingStatus::Active))
        {
            return Err("Vault already has an active listing".to_string());
        }

        // Create new listing
        let listing_id = state.next_listing_id;
        state.next_listing_id += 1;

        let listing = MarketListing {
            id: listing_id,
            vault_id,
            seller: caller,
            buyer: None,
            price_sats,
            status: ListingStatus::Active,
            created_at: ts,
            updated_at: ts,
        };

        state.listings.push(listing.clone());
        Ok(listing)
    });

    if let Ok(ref listing) = result {
        record_event(
            vault_id,
            "VAULT_LISTED",
            &format!(
                "Vault listed for {} satoshis with listing ID {}",
                price_sats, listing.id
            ),
        );
    }

    result
}

/// Cancel an active listing.
#[update]
fn cancel_listing(listing_id: u64) -> Result<MarketListing, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        let listing = match state.listings.iter_mut().find(|l| l.id == listing_id) {
            Some(l) if l.seller == caller => l,
            Some(_) => return Err("Unauthorized: You don't own this listing".to_string()),
            None => return Err("Listing not found".to_string()),
        };

        if !matches!(listing.status, ListingStatus::Active) {
            return Err("Listing is not active".to_string());
        }

        listing.status = ListingStatus::Cancelled;
        listing.updated_at = ts;

        Ok(listing.clone())
    });

    if let Ok(ref listing) = result {
        record_event(
            listing.vault_id,
            "VAULT_LISTING_CANCELLED",
            &format!("Listing {} cancelled", listing_id),
        );
    }

    result
}

/// Get all active listings.
#[query]
fn get_active_listings() -> Vec<MarketListing> {
    with_state(|state| {
        state
            .listings
            .iter()
            .filter(|l| matches!(l.status, ListingStatus::Active))
            .cloned()
            .collect()
    })
}

/// Get all listings owned by the caller (as seller).
#[query]
fn get_my_listings() -> Vec<MarketListing> {
    let caller = msg_caller();
    with_state(|state| {
        state
            .listings
            .iter()
            .filter(|l| l.seller == caller)
            .cloned()
            .collect()
    })
}

/// Buy a listing and transfer vault ownership.
#[update]
fn buy_listing(listing_id: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        // Find and validate listing (get vault_id then release borrow)
        let vault_id = {
            let listing = match state.listings.iter().find(|l| l.id == listing_id) {
                Some(l) => l,
                None => return Err("Listing not found".to_string()),
            };

            if !matches!(listing.status, ListingStatus::Active) {
                return Err("Listing is not active".to_string());
            }

            if listing.seller == caller {
                return Err("Cannot buy your own listing".to_string());
            }

            listing.vault_id
        };

        // Validate vault exists and check status (read-only check)
        {
            let owner = state.vault_index.get(&vault_id).ok_or("Vault not found")?;

            let vaults = state
                .user_vaults
                .get(owner)
                .ok_or("Vault owner not found")?;

            let vault = vaults
                .iter()
                .find(|v| v.id == vault_id)
                .ok_or("Vault not found in owner's list")?;

            if matches!(vault.status, VaultStatus::PendingDeposit) {
                return Err("Cannot buy vault in PendingDeposit status".to_string());
            }
            if matches!(vault.status, VaultStatus::Withdrawn) {
                return Err("Cannot buy withdrawn vault".to_string());
            }
        }

        // Transfer vault ownership using helper
        _transfer_vault(state, vault_id, caller)?;

        // Update listing (now we can get mutable borrow)
        if let Some(listing) = state.listings.iter_mut().find(|l| l.id == listing_id) {
            listing.status = ListingStatus::Filled;
            listing.buyer = Some(caller);
            listing.updated_at = ts;
        }

        // Disable any active auto-reinvest config for this vault
        if let Some(config) = state
            .auto_reinvest
            .iter_mut()
            .find(|c| c.vault_id == vault_id && c.enabled)
        {
            config.enabled = false;
            config.updated_at = ts;
        }

        // Get the vault after transfer to return it
        let vault = _get_vault(state, vault_id, caller).ok_or("Vault not found after transfer")?;

        Ok(vault.clone())
    });

    if let Ok(ref vault) = result {
        record_event(
            vault.id,
            "VAULT_SOLD",
            &format!("Vault sold via listing {}", listing_id),
        );
    }

    result
}

// =======================
// Network Mode Control
// =======================

/// Set runtime mode to Mock (for development/testing)
#[update]
fn set_mode_mock() {
    MODE.with(|m| *m.borrow_mut() = NetworkMode::Mock);
}

/// Set runtime mode to ckBTC Mainnet (for production)
#[update]
fn set_mode_ckbtc_mainnet() {
    MODE.with(|m| *m.borrow_mut() = NetworkMode::CkBTCMainnet);
}

/// Get current runtime mode
#[query]
fn get_mode_query() -> NetworkMode {
    get_mode()
}

/// Set the ledger canister ID (for local testing with mock ledger)
#[update]
fn set_ledger_canister_id(canister_id: String) {
    LEDGER_CANISTER_ID.with(|id| {
        *id.borrow_mut() = Some(canister_id);
    });
}

/// Get current ledger canister ID
#[query]
fn get_ledger_canister_id() -> String {
    LEDGER_CANISTER_ID.with(|id| {
        id.borrow()
            .clone()
            .unwrap_or_else(|| CKBTC_LEDGER_CANISTER_ID.to_string())
    })
}

// =======================
// ckBTC Integration (Placeholder)
// =======================

/// Sync vault balance from ckBTC ledger (real integration)
#[update]
async fn sync_vault_balance_from_ckbtc(vault_id: u64) -> Result<CkbtcSyncResult, String> {
    let caller = msg_caller();
    let mode = get_mode();

    // Only allow in CkBTCMainnet mode
    if !matches!(mode, NetworkMode::CkBTCMainnet) {
        return Err("ckBTC sync is only available in CkBTCMainnet mode".to_string());
    }

    // Find vault and ensure ownership using helper
    let maybe_vault = with_state(|state| _get_vault(state, vault_id, caller).cloned());

    let vault = match maybe_vault {
        Some(v) => v,
        None => return Err("Vault not found or unauthorized".to_string()),
    };

    // Ensure vault has ckBTC subaccount
    if vault.ckbtc_subaccount.is_none() {
        return Err("Vault has no ckBTC subaccount configured".to_string());
    }

    // Call ckBTC ledger to get balance
    let ledger_canister_id = LEDGER_CANISTER_ID.with(|id| {
        id.borrow()
            .clone()
            .unwrap_or_else(|| CKBTC_LEDGER_CANISTER_ID.to_string())
    });
    let ledger_id = Principal::from_text(&ledger_canister_id)
        .map_err(|e| format!("Invalid ckBTC ledger canister id: {}", e))?;

    // CRITICAL FIX: Check the CANISTER's balance (self-custody), not the user's wallet.
    // The user must transfer funds TO this canister for them to be locked.
    let account = Icrc1Account {
        owner: canister_self(), // Canister's own Principal (self-custody)
        subaccount: vault.ckbtc_subaccount.clone(),
    };

    // Call icrc1_balance_of : (record { owner; subaccount }) -> (nat)
    let balance_result = ic_cdk::call(ledger_id, "icrc1_balance_of", (account,)).await;

    // Handle the call result with helpful error messages for local development
    let (balance_nat,): (Nat,) = match balance_result {
        Ok(result) => result,
        Err((_code, msg)) => {
            // Check if this is the "canister not found" error from local dfx
            if msg.contains("not found") || msg.contains("Canister not found") {
                return Err(
                    format!(
                        "ckBTC ledger canister '{}' not found. \
                        For local development, switch to Mock mode in settings or deploy a mock ledger. \
                        Error: {}", 
                        ledger_canister_id, msg
                    )
                );
            }
            return Err(format!("Failed to call ckBTC ledger: {}", msg));
        }
    };

    // Convert Nat to u64 safely
    let synced_balance: u64 = balance_nat
        .0
        .try_into()
        .map_err(|_| "ckBTC balance is too large to fit in u64".to_string())?;

    // Update vault balance in state using helper
    let updated_vault = with_state_mut(|state| {
        if let Some(v) = _get_vault_mut(state, vault_id, caller) {
            v.balance = synced_balance;
            v.updated_at = now_sec();
            Some(v.clone())
        } else {
            None
        }
    })
    .ok_or_else(|| "Vault not found or unauthorized".to_string())?;

    Ok(CkbtcSyncResult {
        vault: updated_vault,
        synced_balance,
        mode,
    })
}

// =======================
// Bitcoin Proof Endpoints (Placeholder)
// =======================

/// Get proof of deposit transaction (placeholder for Bitcoin API integration)
#[query]
async fn get_deposit_proof(vault_id: u64) -> Result<BitcoinTxProof, String> {
    let caller = msg_caller();

    let vault = with_state(|state| _get_vault(state, vault_id, caller).cloned());

    let vault = match vault {
        Some(v) => v,
        None => return Err("Vault not found or unauthorized".to_string()),
    };

    let txid = match &vault.btc_deposit_txid {
        Some(t) => t.clone(),
        None => return Err("No deposit txid recorded for this vault".to_string()),
    };

    // TODO: integrate with ICP Bitcoin canister.
    // For now, return a dummy "unconfirmed" proof.
    Ok(BitcoinTxProof {
        txid,
        confirmed: false,
        confirmations: 0,
    })
}

/// Get proof of withdrawal transaction (placeholder for Bitcoin API integration)
#[query]
async fn get_withdraw_proof(vault_id: u64) -> Result<BitcoinTxProof, String> {
    let caller = msg_caller();

    let vault = with_state(|state| _get_vault(state, vault_id, caller).cloned());

    let vault = match vault {
        Some(v) => v,
        None => return Err("Vault not found or unauthorized".to_string()),
    };

    let txid = match &vault.btc_withdraw_txid {
        Some(t) => t.clone(),
        None => return Err("No withdraw txid recorded for this vault".to_string()),
    };

    // TODO: integrate with ICP Bitcoin canister.
    Ok(BitcoinTxProof {
        txid,
        confirmed: false,
        confirmations: 0,
    })
}

// =======================
// Threshold Signing (Placeholder)
// =======================

// =======================
// Digital Will Access Control
// =======================

/// Endpoint to get the decryption key for Digital Will.
/// Access Control (Dead Man Switch Logic):
/// - Owner: Always has access
/// - Beneficiary: Only has access after inheritance timeout expires
#[update]
fn get_digital_will_key(vault_id: u64) -> Result<String, String> {
    let caller = msg_caller();
    let now = now_sec();

    // Check access permissions and retrieve the secure_key
    let (secure_key, access_type) = with_state(|state| {
        // Find vault owner from index
        let owner = state.vault_index.get(&vault_id).ok_or("Vault not found")?;

        // Get vault from owner's list
        let vaults = state
            .user_vaults
            .get(owner)
            .ok_or("Vault owner not found")?;

        let vault = vaults
            .iter()
            .find(|v| v.id == vault_id)
            .ok_or("Vault not found")?;

        // Check if Digital Will note exists
        if vault.encrypted_note.is_none() {
            return Err("Digital Will note not found for this vault.".to_string());
        }

        // Check if secure_key exists
        let key = vault
            .secure_key
            .clone()
            .ok_or("Decryption key not found for this vault.".to_string())?;

        // --- Conditional Access Logic (Dead Man Switch) ---
        let is_owner = vault.owner == caller;
        let is_beneficiary = vault.beneficiary == Some(caller);
        let is_time_expired = now > (vault.last_keep_alive + vault.inheritance_timeout);

        if is_owner {
            Ok((key, "owner"))
        } else if is_beneficiary && is_time_expired {
            Ok((key, "beneficiary"))
        } else {
            Err("Access Denied: The inheritance conditions are not met.".to_string())
        }
    })?;

    // Record event after releasing borrow
    if access_type == "owner" {
        record_event(
            vault_id,
            "DIGITAL_WILL_KEY_ACCESS",
            "Owner accessed Digital Will key",
        );
    } else {
        record_event(
            vault_id,
            "DIGITAL_WILL_KEY_ACCESS",
            "Beneficiary accessed Digital Will key after inheritance timeout",
        );
    }

    Ok(secure_key)
}

// =======================
// Bitcoin Signing
// =======================

/// Request BTC signature using threshold ECDSA (real integration)
#[update]
async fn request_btc_signature(
    vault_id: u64,
    message: Vec<u8>,
) -> Result<SignatureResponse, String> {
    let caller = msg_caller();

    let owns = with_state(|state| {
        state
            .vault_index
            .get(&vault_id)
            .map(|owner| *owner == caller)
            .unwrap_or(false)
    });

    if !owns {
        return Err("Vault not found or unauthorized".to_string());
    }

    // Validate message is not empty
    if message.is_empty() {
        return Err("Message must not be empty".to_string());
    }

    // Hash the message with SHA-256
    let mut hasher = Sha256::new();
    hasher.update(&message);
    let hash = hasher.finalize();
    let message_hash = hash.to_vec();

    // Build ECDSA signing argument
    let arg = SignWithEcdsaArgument {
        message_hash,
        derivation_path: vec![], // single key for now; can extend later
        key_id: ecdsa_key_id(),
    };

    // Call management canister to sign (canister ID aaaaa-aa)
    // ECDSA signing requires ~26.2B cycles per signature
    let mgmt_canister = Principal::from_text("aaaaa-aa").unwrap();
    let cycles: u128 = 30_000_000_000; // 30 billion cycles (buffer for safety)

    let (resp,): (SignWithEcdsaResponse,) =
        ic_cdk::api::call::call_with_payment128(mgmt_canister, "sign_with_ecdsa", (arg,), cycles)
            .await
            .map_err(|e| format!("Failed to sign with ECDSA: {}", e.1))?;

    // Extract signature from response
    let signature = resp.signature;

    Ok(SignatureResponse { message, signature })
}

// Export Candid interface
ic_cdk::export_candid!();
