// src/ironclad_vault_backend/src/lib.rs

use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::{msg_caller, time};
use ic_cdk_macros::{init, query, update};
use std::cell::RefCell;

// =======================
// Types
// =======================

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

    // BTC-related fields (future-ready)
    pub btc_address: String,
    pub expected_deposit: u64,          // in satoshis (can be 0 for now)
    pub btc_deposit_txid: Option<String>,
    pub btc_withdraw_txid: Option<String>,

    // Timelock & balance
    pub lock_until: u64,                // unix timestamp (seconds)
    pub status: VaultStatus,
    pub balance: u64,                   // current balance (dummy / real later)

    // Metadata
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct VaultEvent {
    pub vault_id: u64,
    pub action: String,                 // e.g. "VAULT_CREATED", "MOCK_DEPOSIT"
    pub timestamp: u64,
    pub notes: String,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct AutoReinvestConfig {
    pub vault_id: u64,
    pub owner: Principal,
    pub new_lock_duration: u64,
    pub enabled: bool,
    pub created_at: u64,
    pub updated_at: u64,
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

#[derive(Default)]
pub struct State {
    pub next_id: u64,
    pub vaults: Vec<Vault>,
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
// Lifecycle
// =======================

#[init]
fn init() {
    // nothing special for now
}

// =======================
// Public methods
// =======================

/// Create a new vault with a lock_until time and optional expected_deposit.
/// For now btc_address is a placeholder string; later we'll plug real BTC.
#[update]
fn create_vault(lock_until: u64, expected_deposit: u64) -> Vault {
    let caller = msg_caller();
    let ts = now_sec();

    let vault = with_state_mut(|state| {
        let id = state.next_id;
        state.next_id += 1;

        // TODO: replace placeholder with real BTC address derivation
        let btc_address = format!("IRONCLAD-VAULT-{}", id);

        let vault = Vault {
            id,
            owner: caller,
            btc_address,
            expected_deposit,
            btc_deposit_txid: None,
            btc_withdraw_txid: None,
            lock_until,
            status: VaultStatus::PendingDeposit,
            balance: 0,
            created_at: ts,
            updated_at: ts,
        };

        state.vaults.push(vault.clone());
        vault
    });

    record_event(vault.id, "VAULT_CREATED", "Vault created in PendingDeposit state");
    vault
}

/// Get all vaults owned by the caller.
#[query]
fn get_my_vaults() -> Vec<Vault> {
    let caller = msg_caller();
    with_state(|state| {
        state
            .vaults
            .iter()
            .filter(|v| v.owner == caller)
            .cloned()
            .collect()
    })
}

/// Get a single vault by id, only if owned by caller.
#[query]
fn get_vault(id: u64) -> Option<Vault> {
    let caller = msg_caller();
    with_state(|state| {
        state
            .vaults
            .iter()
            .find(|v| v.id == id && v.owner == caller)
            .cloned()
    })
}

/// Get history events for a given vault id (owned by caller).
#[query]
fn get_vault_events(id: u64) -> Vec<VaultEvent> {
    let caller = msg_caller();
    with_state(|state| {
        // Ensure caller owns the vault before showing history
        let owns = state
            .vaults
            .iter()
            .any(|v| v.id == id && v.owner == caller);

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
        let vault = match state
            .vaults
            .iter_mut()
            .find(|v| v.id == id)
        {
            Some(v) if v.owner == caller => v,
            Some(_) => return Err("Unauthorized: You don't own this vault".to_string()),
            None => return Err("Vault not found".to_string()),
        };

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
        let vault = match state
            .vaults
            .iter()
            .find(|v| v.id == id)
        {
            Some(v) if v.owner == caller => v,
            Some(_) => return Err("Vault not found or unauthorized".to_string()),
            None => return Err("Vault not found or unauthorized".to_string()),
        };

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
        let vault = match state
            .vaults
            .iter_mut()
            .find(|v| v.id == id)
        {
            Some(v) if v.owner == caller => v,
            Some(_) => return Err("Unauthorized: You don't own this vault".to_string()),
            None => return Err("Vault not found".to_string()),
        };

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
            VaultStatus::Unlockable => {
                Err("Vault is already unlocked".to_string())
            }
            VaultStatus::PendingDeposit => {
                Err("Vault must be locked before unlocking".to_string())
            }
            VaultStatus::Withdrawn => {
                Err("Vault has already been withdrawn".to_string())
            }
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
#[query]
fn get_unlockable_vaults() -> Vec<Vault> {
    let caller = msg_caller();
    let now = now_sec();

    with_state(|state| {
        state
            .vaults
            .iter()
            .filter(|v| {
                v.owner == caller
                    && matches!(v.status, VaultStatus::ActiveLocked)
                    && now >= v.lock_until
            })
            .cloned()
            .collect()
    })
}

/// Preview withdraw amount available for a vault (no mutation).
#[query]
fn preview_withdraw(id: u64) -> Result<u64, String> {
    let caller = msg_caller();
    with_state(|state| {
        let vault = match state.vaults.iter().find(|v| v.id == id) {
            Some(v) if v.owner == caller => v,
            _ => return Err("Vault not found or unauthorized".to_string()),
        };

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
        let vault = match state.vaults.iter_mut().find(|v| v.id == id) {
            Some(v) if v.owner == caller => v,
            Some(_) => return Err("Unauthorized: You don't own this vault".to_string()),
            None => return Err("Vault not found".to_string()),
        };

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
#[query]
fn get_withdrawable_vaults() -> Vec<Vault> {
    let caller = msg_caller();
    with_state(|state| {
        state
            .vaults
            .iter()
            .filter(|v| v.owner == caller && matches!(v.status, VaultStatus::Unlockable) && v.balance > 0)
            .cloned()
            .collect()
    })
}

// =======================
// Auto-Reinvest System
// =======================

/// Schedule auto-reinvest for a vault with a new lock duration.
#[update]
fn schedule_auto_reinvest(vault_id: u64, new_lock_duration: u64) -> Result<AutoReinvestConfig, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        // Find and validate vault
        let vault = match state.vaults.iter().find(|v| v.id == vault_id) {
            Some(v) if v.owner == caller => v,
            Some(_) => return Err("Unauthorized: You don't own this vault".to_string()),
            None => return Err("Vault not found".to_string()),
        };

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
            };
            state.auto_reinvest.push(config.clone());
            Ok(config)
        }
    });

    if let Ok(ref _config) = result {
        record_event(
            vault_id,
            "AUTO_REINVEST_SCHEDULED",
            &format!("Auto-reinvest scheduled with lock duration {} seconds", new_lock_duration),
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
            None => return Err("No active auto-reinvest config for this vault or unauthorized".to_string()),
        };

        // Disable config
        config.enabled = false;
        config.updated_at = ts;
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

    let result = with_state_mut(|state| {
        // Find active auto-reinvest config
        let config = match state
            .auto_reinvest
            .iter()
            .find(|c| c.vault_id == vault_id && c.owner == caller && c.enabled)
        {
            Some(c) => c.clone(),
            None => return Err("No active auto-reinvest config for this vault or unauthorized".to_string()),
        };

        // Find source vault
        let source_vault = match state.vaults.iter_mut().find(|v| v.id == vault_id) {
            Some(v) if v.owner == caller => v,
            Some(_) => return Err("Unauthorized: You don't own this vault".to_string()),
            None => return Err("Vault not found".to_string()),
        };

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

        let new_vault = Vault {
            id: new_id,
            owner: caller,
            btc_address: format!("IRONCLAD-VAULT-{}", new_id),
            expected_deposit: old_balance,
            btc_deposit_txid: None,
            btc_withdraw_txid: None,
            lock_until: ts + config.new_lock_duration,
            status: VaultStatus::ActiveLocked,
            balance: old_balance,
            created_at: ts,
            updated_at: ts,
        };

        state.vaults.push(new_vault.clone());

        // Disable the auto-reinvest config
        if let Some(cfg) = state
            .auto_reinvest
            .iter_mut()
            .find(|c| c.vault_id == vault_id && c.owner == caller)
        {
            cfg.enabled = false;
            cfg.updated_at = ts;
        }

        Ok(new_vault)
    });

    if let Ok(ref new_vault) = result {
        record_event(
            vault_id,
            "AUTO_REINVEST_EXECUTED_SOURCE",
            &format!("Source vault withdrawn for reinvestment"),
        );
        record_event(
            new_vault.id,
            "AUTO_REINVEST_EXECUTED_TARGET",
            &format!("New vault created from auto-reinvest with balance {}", new_vault.balance),
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
        // Find and validate vault
        let vault = match state.vaults.iter().find(|v| v.id == vault_id) {
            Some(v) if v.owner == caller => v,
            Some(_) => return Err("Unauthorized: You don't own this vault".to_string()),
            None => return Err("Vault not found".to_string()),
        };

        // Validate vault status
        if matches!(vault.status, VaultStatus::PendingDeposit) {
            return Err("Cannot list vault in PendingDeposit status".to_string());
        }
        if matches!(vault.status, VaultStatus::Withdrawn) {
            return Err("Cannot list withdrawn vault".to_string());
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
            &format!("Vault listed for {} satoshis with listing ID {}", price_sats, listing.id),
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
        // Find and validate listing
        let listing = match state.listings.iter_mut().find(|l| l.id == listing_id) {
            Some(l) => l,
            None => return Err("Listing not found".to_string()),
        };

        if !matches!(listing.status, ListingStatus::Active) {
            return Err("Listing is not active".to_string());
        }

        if listing.seller == caller {
            return Err("Cannot buy your own listing".to_string());
        }

        let vault_id = listing.vault_id;

        // Find and validate vault
        let vault = match state.vaults.iter_mut().find(|v| v.id == vault_id) {
            Some(v) => v,
            None => return Err("Vault not found".to_string()),
        };

        if matches!(vault.status, VaultStatus::PendingDeposit) {
            return Err("Cannot buy vault in PendingDeposit status".to_string());
        }
        if matches!(vault.status, VaultStatus::Withdrawn) {
            return Err("Cannot buy withdrawn vault".to_string());
        }

        // Transfer vault ownership
        vault.owner = caller;
        vault.updated_at = ts;

        // Update listing
        listing.status = ListingStatus::Filled;
        listing.buyer = Some(caller);
        listing.updated_at = ts;

        // Disable any active auto-reinvest config for this vault
        if let Some(config) = state
            .auto_reinvest
            .iter_mut()
            .find(|c| c.vault_id == vault_id && c.enabled)
        {
            config.enabled = false;
            config.updated_at = ts;
        }

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

// Export Candid interface
ic_cdk::export_candid!();
