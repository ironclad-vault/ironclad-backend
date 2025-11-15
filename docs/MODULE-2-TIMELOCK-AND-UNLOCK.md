# Module 2: Timelock Logic & Unlock State Machine

## Summary

This module adds timelock logic and unlock transitions focused on security and determinism.

- New functions: `is_vault_unlockable` (query), `unlock_vault` (update), `get_unlockable_vaults` (query).
- Rules: only owner can check/unlock; time uses seconds; unlock valid when `now >= lock_until` (inclusive).
- Events: `UNLOCK_READY` logged after successful mutation and called outside `with_state_mut()`.
- Result: complete create → deposit → unlock flow; ready for Module 3 (withdraw flow).

---

## 1. Overview Timelock System

### Concept

Ironclad Vault uses **time-based lock** to lock Bitcoin until a specific time (`lock_until`). After that time passes, the vault can be unlocked and funds are ready for withdrawal.

### State Flow

```
PendingDeposit → (deposit) → ActiveLocked → (timelock expired) → Unlockable → (withdraw) → Withdrawn
```

### Key Components

1. **`lock_until`**: Unix timestamp (seconds) when vault can be unlocked
2. **`now_sec()`**: Current time in seconds
3. **State transitions**: State validation before unlock

---

## 2. Function 1: `is_vault_unlockable`

### Signature

```rust
#[query]
fn is_vault_unlockable(id: u64) -> Result<bool, String>
```

### Purpose

Check if vault can be unlocked without performing state mutation.

### Implementation

```rust
#[query]
fn is_vault_unlockable(id: u64) -> Result<bool, String> {
    let caller = msg_caller();
    let now = now_sec();

    with_state(|state| {
        let vault = match state.vaults.iter().find(|v| v.id == id) {
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
```

### Logic Flow

1. Get caller identity
2. Get current time
3. Find vault by ID
4. Check ownership
5. Check if `ActiveLocked` AND `now >= lock_until`
6. Return `true` if both conditions met, `false` otherwise

### Return Values

- `Ok(true)` - Vault can be unlocked (timelock expired)
- `Ok(false)` - Vault cannot be unlocked yet
- `Err("Vault not found or unauthorized")` - Vault not found or not owned by caller

### Testing

```bash
# Vault with lock_until in the past (expired)
dfx canister call ironclad_vault_backend is_vault_unlockable '(0)'
# Output: (variant { Ok = true })

# Vault with lock_until in the future (still locked)
dfx canister call ironclad_vault_backend is_vault_unlockable '(1)'
# Output: (variant { Ok = false })

# Vault not found
dfx canister call ironclad_vault_backend is_vault_unlockable '(999)'
# Output: (variant { Err = "Vault not found or unauthorized" })
```

---

## 3. Function 2: `unlock_vault`

### Signature

```rust
#[update]
fn unlock_vault(id: u64) -> Result<Vault, String>
```

### Purpose

Unlock vault after timelock expires, changing status from `ActiveLocked` to `Unlockable`.

### Implementation

```rust
#[update]
fn unlock_vault(id: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        let vault = match state.vaults.iter_mut().find(|v| v.id == id) {
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
```

### Logic Flow

1. Get caller identity
2. Get current time
3. Find vault by ID (mutable)
4. Check ownership
5. Validate current status
6. Check timelock: `now >= lock_until`
7. Update status to `Unlockable`
8. Update `updated_at` timestamp
9. Log event `UNLOCK_READY` (outside `with_state_mut`)
10. Return updated vault

### Validations

| Current Status    | Condition                  | Result                                          |
|-------------------|----------------------------|-------------------------------------------------|
| `ActiveLocked`    | `now >= lock_until`        | ✅ Unlock to `Unlockable`                      |
| `ActiveLocked`    | `now < lock_until`         | ❌ Error: "Vault still locked until..."        |
| `Unlockable`      | -                          | ❌ Error: "Vault is already unlocked"          |
| `PendingDeposit`  | -                          | ❌ Error: "Vault must be locked before..."     |
| `Withdrawn`       | -                          | ❌ Error: "Vault has already been withdrawn"   |

### Testing

```bash
# Success: Unlock expired vault
dfx canister call ironclad_vault_backend unlock_vault '(0)'
# Output: (variant { Ok = record { status = variant { Unlockable }; ... } })

# Error: Already unlocked
dfx canister call ironclad_vault_backend unlock_vault '(0)'
# Output: (variant { Err = "Vault is already unlocked" })

# Error: Still locked
dfx canister call ironclad_vault_backend unlock_vault '(1)'
# Output: (variant { Err = "Vault still locked until 9999999999. Current time: 1763201305" })

# Error: Not found
dfx canister call ironclad_vault_backend unlock_vault '(999)'
# Output: (variant { Err = "Vault not found" })
```

---

## 4. Function 3: `get_unlockable_vaults`

### Signature

```rust
#[query]
fn get_unlockable_vaults() -> Vec<Vault>
```

### Purpose

Get all vaults owned by caller that can be unlocked (timelock has expired).

### Implementation

```rust
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
```

### Logic Flow

1. Get caller identity
2. Get current time
3. Filter vaults by:
   - Owned by caller
   - Status is `ActiveLocked`
   - Current time >= `lock_until`
4. Return filtered list

### Use Cases

- UI: Show list of vaults that can be unlocked
- Automation: Batch unlock multiple vaults
- Monitoring: Track which vaults are ready

### Testing

```bash
# Get all unlockable vaults
dfx canister call ironclad_vault_backend get_unlockable_vaults '()'
# Output: (vec { record { id = 0; status = variant { ActiveLocked }; ... } })
```

---

## 5. Updated Candid Interface

### File: `ironclad_vault_backend.did`

```candid
type VaultStatus = variant {
    PendingDeposit;
    ActiveLocked;
    Unlockable;
    Withdrawn;
};

type Vault = record {
    id: nat64;
    owner: principal;
    btc_address: text;
    expected_deposit: nat64;
    btc_deposit_txid: opt text;
    btc_withdraw_txid: opt text;
    lock_until: nat64;
    status: VaultStatus;
    balance: nat64;
    created_at: nat64;
    updated_at: nat64;
};

type VaultEvent = record {
    vault_id: nat64;
    action: text;
    timestamp: nat64;
    notes: text;
};

service : {
    // Previous functions
    create_vault: (nat64, nat64) -> (Vault);
    get_my_vaults: () -> (vec Vault) query;
    get_vault: (nat64) -> (opt Vault) query;
    get_vault_events: (nat64) -> (vec VaultEvent) query;
    mock_deposit_vault: (nat64, nat64) -> (variant { Ok: Vault; Err: text });
    
    // New timelock functions
    is_vault_unlockable: (nat64) -> (variant { Ok: bool; Err: text }) query;
    unlock_vault: (nat64) -> (variant { Ok: Vault; Err: text });
    get_unlockable_vaults: () -> (vec Vault) query;
}
```

---

## 6. Complete Test Scenario

### Scenario: Create → Lock → Check → Unlock

```bash
# Step 1: Create vault with lock_until in the past (for testing)
dfx canister call ironclad_vault_backend create_vault '(1731700000, 100000)'
# Output: vault with id=0, status=PendingDeposit

# Step 2: Deposit to activate lock
dfx canister call ironclad_vault_backend mock_deposit_vault '(0, 50000)'
# Output: status=ActiveLocked, balance=50000

# Step 3: Check if can be unlocked
dfx canister call ironclad_vault_backend is_vault_unlockable '(0)'
# Output: Ok(true) - because lock_until has passed

# Step 4: Get list unlockable vaults
dfx canister call ironclad_vault_backend get_unlockable_vaults '()'
# Output: [vault id=0]

# Step 5: Unlock vault
dfx canister call ironclad_vault_backend unlock_vault '(0)'
# Output: status=Unlockable

# Step 6: Check events
dfx canister call ironclad_vault_backend get_vault_events '(0)'
# Output:
# - VAULT_CREATED
# - MOCK_DEPOSIT
# - LOCK_STARTED
# - UNLOCK_READY
```

### Scenario: Try Unlock Locked Vault (Should Fail)

```bash
# Step 1: Create vault with lock_until in the future
dfx canister call ironclad_vault_backend create_vault '(9999999999, 200000)'
# Output: vault with id=1, status=PendingDeposit

# Step 2: Deposit
dfx canister call ironclad_vault_backend mock_deposit_vault '(1, 75000)'
# Output: status=ActiveLocked

# Step 3: Check unlockable (should be false)
dfx canister call ironclad_vault_backend is_vault_unlockable '(1)'
# Output: Ok(false)

# Step 4: Try to unlock (should fail)
dfx canister call ironclad_vault_backend unlock_vault '(1)'
# Output: Err("Vault still locked until 9999999999. Current time: 1763201305")
```

---

## 7. Event Logging

### New Event Type: `UNLOCK_READY`

Each time a vault is unlocked, the event is logged:

```rust
record_event(
    id,
    "UNLOCK_READY",
    &format!("Vault unlocked after timelock expired at {}", ts),
);
```

### Event Timeline Example

```
1. VAULT_CREATED      @ 1763201227 - "Vault created in PendingDeposit state"
2. MOCK_DEPOSIT       @ 1763201236 - "Mock deposit of 50000 satoshis"
3. LOCK_STARTED       @ 1763201236 - "Vault moved to ActiveLocked"
4. UNLOCK_READY       @ 1763201262 - "Vault unlocked after timelock expired at 1763201262"
```

---

## 8. Architecture Compliance

### ✅ Following Best Practices

1. **State Management**
   - ✅ Use `with_state()` for reads
   - ✅ Use `with_state_mut()` for writes
   - ✅ No nested borrows

2. **Event Logging**
   - ✅ `record_event()` called OUTSIDE `with_state_mut()`
   - ✅ Descriptive event names and notes

3. **Error Handling**
   - ✅ Use `Result<T, String>` for mutations
   - ✅ Specific error messages
   - ✅ Pattern matching for validations

4. **Ownership & Security**
   - ✅ Check `caller == owner` before mutations
   - ✅ Validate state transitions
   - ✅ Return appropriate errors for unauthorized access

5. **Time Handling**
   - ✅ Use `now_sec()` helper consistently
   - ✅ Compare timestamps properly

---

## 9. State Machine Diagram

```
┌─────────────────┐
│ PendingDeposit  │
└────────┬────────┘
         │ mock_deposit_vault()
         │ balance > 0
         ▼
┌─────────────────┐
│  ActiveLocked   │◄──── timelock active (now < lock_until)
└────────┬────────┘
         │ unlock_vault()
         │ now >= lock_until
         ▼
┌─────────────────┐
│   Unlockable    │◄──── ready for withdrawal
└────────┬────────┘
         │ (future: withdraw_vault)
         ▼
┌─────────────────┐
│   Withdrawn     │◄──── final state
└─────────────────┘
```

---

## 10. Key Takeaways

### ✅ What We Implemented

1. **Query Function**: `is_vault_unlockable()` - Non-mutating check
2. **Update Function**: `unlock_vault()` - State transition with validations
3. **Query Function**: `get_unlockable_vaults()` - Batch lookup
4. **Event Logging**: Track unlock operations
5. **Comprehensive Error Handling**: Clear feedback for all edge cases

### 🎯 Design Decisions

1. **Separate Check & Unlock**
   - `is_vault_unlockable()` for UI/preview
   - `unlock_vault()` for actual state change

2. **Explicit Time Comparison**
   - `now >= lock_until` (inclusive)
   - Clear error messages with timestamps

3. **State Validation**
   - Prevent invalid transitions
   - Handle all status enum variants

4. **Security**
   - Ownership checks in every function
   - Consistent error messages for unauthorized

### ⚠️ Important Notes

- ⏰ Timelock uses **seconds**, not nanoseconds
- 🔐 Only vault owner can check/unlock
- 📝 Events logged after successful mutations
- ✅ State transitions validated strictly

### 🚀 Next Steps

Future modules can add:

- **Module 3**: Withdraw function (actual BTC/ckBTC transfer)
- **Module 4**: Marketplace (list, buy, sell vaults)
- **Module 5**: Auto-reinvest options
- **Module 6**: Cron jobs for auto-unlock
- **Module 7**: Transfer ownership

---

## Summary

Module 2 completes the timelock flow up to `Unlockable` status with:

- 3 new functions (2 queries, 1 update) and comprehensive validation.
- Event `UNLOCK_READY` and safe logging pattern (outside mutation).
- Consistent error handling (`Result<T, String>`) and ownership checks.

As a result, the vault has a stable create → lock → unlock cycle and is ready for Module 3 (Withdraw Flow). 🎉
