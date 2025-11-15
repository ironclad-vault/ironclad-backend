# Module 3: Withdraw Flow

## Summary

This module implements the complete withdrawal functionality, enabling vault owners to preview available balances, execute partial or full withdrawals, and query all withdrawable vaults.

- New functions: `preview_withdraw` (query), `withdraw_vault` (update), `get_withdrawable_vaults` (query).
- Requirements: vault must be in `Unlockable` status with balance > 0; only owner can withdraw.
- Behavior: supports partial withdrawals; sets `btc_withdraw_txid` as `MOCK-TXID-<id>`; transitions to `Withdrawn` when balance reaches 0.
- Events: `WITHDRAW_REQUESTED` and `WITHDRAW_COMPLETED` logged after successful mutation (outside `with_state_mut()`).
- Result: complete vault lifecycle from create → deposit → unlock → withdraw with full audit trail.

---

## 1. Overview Withdraw System

### Concept

After a vault reaches `Unlockable` status (timelock expired), the owner can withdraw the locked Bitcoin. The system supports:

- **Preview**: Check available balance before withdrawing
- **Partial Withdrawal**: Withdraw any amount ≤ balance
- **Full Withdrawal**: Withdraw entire balance, transitioning vault to `Withdrawn`
- **Mock Transaction**: Simulates Bitcoin withdrawal with mock transaction ID

### State Flow

```
Unlockable → (partial withdraw) → Unlockable (balance reduced)
Unlockable → (full withdraw) → Withdrawn (balance = 0)
```

### Key Components

1. **`balance`**: Current satoshi balance in vault
2. **`btc_withdraw_txid`**: Set to `MOCK-TXID-<vault_id>` after first withdrawal
3. **Status transitions**: `Unlockable` remains unless balance = 0, then → `Withdrawn`

---

## 2. Function 1: `preview_withdraw`

### Signature

```rust
#[query]
fn preview_withdraw(id: u64) -> Result<u64, String>
```

### Purpose

Query function to check available withdrawal balance without performing any state mutation.

### Implementation

```rust
#[query]
fn preview_withdraw(id: u64) -> Result<u64, String> {
    let caller = msg_caller();

    with_state(|state| {
        let vault = match state.vaults.iter().find(|v| v.id == id) {
            Some(v) if v.owner == caller => v,
            Some(_) => return Err("Vault not found or unauthorized".to_string()),
            None => return Err("Vault not found or unauthorized".to_string()),
        };

        match vault.status {
            VaultStatus::Unlockable if vault.balance > 0 => Ok(vault.balance),
            VaultStatus::Unlockable => Err("Vault has no balance to withdraw".to_string()),
            _ => Err("Vault must be unlocked before withdrawing".to_string()),
        }
    })
}
```

### Logic Flow

1. Get caller identity
2. Find vault by ID
3. Check ownership
4. Validate status is `Unlockable`
5. Check balance > 0
6. Return available balance

### Return Values

- `Ok(balance)` - Available amount in satoshis
- `Err("Vault not found or unauthorized")` - Vault doesn't exist or not owned by caller
- `Err("Vault has no balance to withdraw")` - Vault is `Unlockable` but balance = 0
- `Err("Vault must be unlocked before withdrawing")` - Vault not in `Unlockable` status

### Testing

```bash
# Success: Preview available balance
dfx canister call ironclad_vault_backend preview_withdraw '(0)'
# Output: (variant { Ok = 90_000 : nat64 })

# Error: Vault not unlocked
dfx canister call ironclad_vault_backend preview_withdraw '(1)'
# Output: (variant { Err = "Vault must be unlocked before withdrawing" })

# Error: Not found/unauthorized
dfx canister call ironclad_vault_backend preview_withdraw '(999)'
# Output: (variant { Err = "Vault not found or unauthorized" })
```

---

## 3. Function 2: `withdraw_vault`

### Signature

```rust
#[update]
fn withdraw_vault(id: u64, amount: u64) -> Result<Vault, String>
```

### Purpose

Execute withdrawal from vault, reducing balance and updating status if fully withdrawn.

### Implementation

```rust
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

        match vault.status {
            VaultStatus::Unlockable => {
                if amount == 0 {
                    return Err("Withdrawal amount must be greater than 0".to_string());
                }
                if amount > vault.balance {
                    return Err(format!(
                        "Insufficient balance. Available: {}, Requested: {}",
                        vault.balance, amount
                    ));
                }

                // Process withdrawal
                vault.balance -= amount;
                vault.btc_withdraw_txid = Some(format!("MOCK-TXID-{}", vault.id));
                vault.updated_at = ts;

                // If balance is now 0, mark as fully withdrawn
                if vault.balance == 0 {
                    vault.status = VaultStatus::Withdrawn;
                }

                Ok(vault.clone())
            }
            VaultStatus::Withdrawn => Err("Vault has already been fully withdrawn".to_string()),
            VaultStatus::ActiveLocked => {
                Err("Vault must be unlocked before withdrawing".to_string())
            }
            VaultStatus::PendingDeposit => {
                Err("Vault must be deposited and unlocked before withdrawing".to_string())
            }
        }
    });

    if let Ok(ref v) = result {
        record_event(
            id,
            "WITHDRAW_REQUESTED",
            &format!("Withdrawal of {} satoshis initiated", amount),
        );
        if v.status == VaultStatus::Withdrawn {
            record_event(
                id,
                "WITHDRAW_COMPLETED",
                "Vault fully withdrawn, status set to Withdrawn",
            );
        } else {
            record_event(
                id,
                "WITHDRAW_COMPLETED",
                &format!("Partial withdrawal completed. Remaining balance: {}", v.balance),
            );
        }
    }

    result
}
```

### Logic Flow

1. Get caller identity and timestamp
2. Find vault by ID (mutable)
3. Check ownership
4. Validate status is `Unlockable`
5. Validate amount: > 0 and ≤ balance
6. Reduce balance by withdrawal amount
7. Set `btc_withdraw_txid` to mock transaction ID
8. Update `updated_at` timestamp
9. If balance = 0, transition to `Withdrawn` status
10. Log events `WITHDRAW_REQUESTED` and `WITHDRAW_COMPLETED` (outside mutation)
11. Return updated vault

### Validations

| Current Status    | Condition                       | Result                                              |
|-------------------|---------------------------------|-----------------------------------------------------|
| `Unlockable`      | amount > 0 and amount ≤ balance | ✅ Withdraw, stay `Unlockable` if balance > 0      |
| `Unlockable`      | amount = balance                | ✅ Withdraw, transition to `Withdrawn`             |
| `Unlockable`      | amount = 0                      | ❌ Error: "Withdrawal amount must be > 0"          |
| `Unlockable`      | amount > balance                | ❌ Error: "Insufficient balance"                   |
| `Withdrawn`       | -                               | ❌ Error: "Already fully withdrawn"                |
| `ActiveLocked`    | -                               | ❌ Error: "Must be unlocked before withdrawing"    |
| `PendingDeposit`  | -                               | ❌ Error: "Must be deposited and unlocked..."      |

### Testing

```bash
# Success: Partial withdrawal
dfx canister call ironclad_vault_backend withdraw_vault '(0, 40000)'
# Output: (variant { Ok = record { balance = 50_000; status = variant { Unlockable }; btc_withdraw_txid = opt "MOCK-TXID-0"; ... } })

# Success: Full withdrawal
dfx canister call ironclad_vault_backend withdraw_vault '(0, 50000)'
# Output: (variant { Ok = record { balance = 0; status = variant { Withdrawn }; btc_withdraw_txid = opt "MOCK-TXID-0"; ... } })

# Error: Amount = 0
dfx canister call ironclad_vault_backend withdraw_vault '(0, 0)'
# Output: (variant { Err = "Withdrawal amount must be greater than 0" })

# Error: Insufficient balance
dfx canister call ironclad_vault_backend withdraw_vault '(0, 999999)'
# Output: (variant { Err = "Insufficient balance. Available: 90000, Requested: 999999" })

# Error: Already withdrawn
dfx canister call ironclad_vault_backend withdraw_vault '(0, 10000)'
# Output: (variant { Err = "Vault has already been fully withdrawn" })
```

---

## 4. Function 3: `get_withdrawable_vaults`

### Signature

```rust
#[query]
fn get_withdrawable_vaults() -> Vec<Vault>
```

### Purpose

Query function to retrieve all vaults owned by caller that are ready for withdrawal.

### Implementation

```rust
#[query]
fn get_withdrawable_vaults() -> Vec<Vault> {
    let caller = msg_caller();

    with_state(|state| {
        state
            .vaults
            .iter()
            .filter(|v| {
                v.owner == caller
                    && matches!(v.status, VaultStatus::Unlockable)
                    && v.balance > 0
            })
            .cloned()
            .collect()
    })
}
```

### Logic Flow

1. Get caller identity
2. Filter vaults by:
   - Owned by caller
   - Status is `Unlockable`
   - Balance > 0
3. Return filtered list

### Use Cases

- **UI Dashboard**: Show withdrawable vaults with available balances
- **Batch Processing**: Identify all vaults ready for withdrawal
- **Portfolio Management**: Track unlocked funds across multiple vaults

### Testing

```bash
# Get all withdrawable vaults
dfx canister call ironclad_vault_backend get_withdrawable_vaults '()'
# Output: (vec { record { id = 0; status = variant { Unlockable }; balance = 90_000; ... } })

# After full withdrawal, returns empty
dfx canister call ironclad_vault_backend get_withdrawable_vaults '()'
# Output: (vec {})
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
    // Module 1: Basic vault management
    create_vault: (nat64, nat64) -> (Vault);
    get_my_vaults: () -> (vec Vault) query;
    get_vault: (nat64) -> (opt Vault) query;
    get_vault_events: (nat64) -> (vec VaultEvent) query;
    mock_deposit_vault: (nat64, nat64) -> (variant { Ok: Vault; Err: text });
    
    // Module 2: Timelock and unlock
    is_vault_unlockable: (nat64) -> (variant { Ok: bool; Err: text }) query;
    unlock_vault: (nat64) -> (variant { Ok: Vault; Err: text });
    get_unlockable_vaults: () -> (vec Vault) query;
    
    // Module 3: Withdraw flow
    preview_withdraw: (nat64) -> (variant { Ok: nat64; Err: text }) query;
    withdraw_vault: (nat64, nat64) -> (variant { Ok: Vault; Err: text });
    get_withdrawable_vaults: () -> (vec Vault) query;
}
```

---

## 6. Complete Test Scenario

### Scenario: Full Lifecycle with Partial Withdrawal

```bash
# Step 1: Create vault with past lock_until (for testing)
dfx canister call ironclad_vault_backend create_vault '(1731700000, 100000)'
# Output: id=0, status=PendingDeposit

# Step 2: Deposit to activate lock
dfx canister call ironclad_vault_backend mock_deposit_vault '(0, 90000)'
# Output: status=ActiveLocked, balance=90000

# Step 3: Unlock vault
dfx canister call ironclad_vault_backend unlock_vault '(0)'
# Output: status=Unlockable

# Step 4: Preview withdrawal
dfx canister call ironclad_vault_backend preview_withdraw '(0)'
# Output: Ok(90000)

# Step 5: List withdrawable vaults
dfx canister call ironclad_vault_backend get_withdrawable_vaults '()'
# Output: [vault id=0, balance=90000]

# Step 6: Partial withdrawal
dfx canister call ironclad_vault_backend withdraw_vault '(0, 40000)'
# Output: status=Unlockable, balance=50000, btc_withdraw_txid="MOCK-TXID-0"

# Step 7: Check remaining balance
dfx canister call ironclad_vault_backend preview_withdraw '(0)'
# Output: Ok(50000)

# Step 8: Full withdrawal (remaining balance)
dfx canister call ironclad_vault_backend withdraw_vault '(0, 50000)'
# Output: status=Withdrawn, balance=0

# Step 9: Verify withdrawable list is empty
dfx canister call ironclad_vault_backend get_withdrawable_vaults '()'
# Output: []

# Step 10: Check complete event history
dfx canister call ironclad_vault_backend get_vault_events '(0)'
# Output:
# - VAULT_CREATED
# - MOCK_DEPOSIT
# - LOCK_STARTED
# - UNLOCK_READY
# - WITHDRAW_REQUESTED (40000)
# - WITHDRAW_COMPLETED (partial, remaining 50000)
# - WITHDRAW_REQUESTED (50000)
# - WITHDRAW_COMPLETED (fully withdrawn)
```

### Scenario: Error Handling

```bash
# Error: Try to withdraw from locked vault
dfx canister call ironclad_vault_backend withdraw_vault '(1, 10000)'
# Output: Err("Vault must be unlocked before withdrawing")

# Error: Try to withdraw 0
dfx canister call ironclad_vault_backend withdraw_vault '(0, 0)'
# Output: Err("Withdrawal amount must be greater than 0")

# Error: Try to withdraw more than balance
dfx canister call ironclad_vault_backend withdraw_vault '(0, 999999)'
# Output: Err("Insufficient balance. Available: 90000, Requested: 999999")

# Error: Try to withdraw from already withdrawn vault
dfx canister call ironclad_vault_backend withdraw_vault '(0, 10000)'
# Output: Err("Vault has already been fully withdrawn")

# Error: Preview unauthorized vault
dfx canister call ironclad_vault_backend preview_withdraw '(999)'
# Output: Err("Vault not found or unauthorized")
```

---

## 7. Event Logging

### New Event Types

1. **`WITHDRAW_REQUESTED`**: Logged when withdrawal is initiated
   ```rust
   record_event(id, "WITHDRAW_REQUESTED", 
       &format!("Withdrawal of {} satoshis initiated", amount));
   ```

2. **`WITHDRAW_COMPLETED`**: Logged after successful withdrawal
   - Partial: `"Partial withdrawal completed. Remaining balance: <amount>"`
   - Full: `"Vault fully withdrawn, status set to Withdrawn"`

### Event Timeline Example

```
1. VAULT_CREATED       @ 1763201227 - "Vault created in PendingDeposit state"
2. MOCK_DEPOSIT        @ 1763201236 - "Mock deposit of 90000 satoshis"
3. LOCK_STARTED        @ 1763201236 - "Vault moved to ActiveLocked"
4. UNLOCK_READY        @ 1763201262 - "Vault unlocked after timelock expired at 1763201262"
5. WITHDRAW_REQUESTED  @ 1763201280 - "Withdrawal of 40000 satoshis initiated"
6. WITHDRAW_COMPLETED  @ 1763201280 - "Partial withdrawal completed. Remaining balance: 50000"
7. WITHDRAW_REQUESTED  @ 1763201295 - "Withdrawal of 50000 satoshis initiated"
8. WITHDRAW_COMPLETED  @ 1763201295 - "Vault fully withdrawn, status set to Withdrawn"
```

---

## 8. Architecture Compliance

### ✅ Following Best Practices

1. **State Management**
   - ✅ Use `with_state()` for reads (preview, get_withdrawable)
   - ✅ Use `with_state_mut()` for writes (withdraw_vault)
   - ✅ No nested borrows

2. **Event Logging**
   - ✅ `record_event()` called OUTSIDE `with_state_mut()`
   - ✅ Two events per withdrawal: REQUESTED and COMPLETED
   - ✅ Descriptive notes with amounts and status

3. **Error Handling**
   - ✅ Use `Result<T, String>` for all fallible operations
   - ✅ Specific error messages for each failure case
   - ✅ Pattern matching for comprehensive validation

4. **Ownership & Security**
   - ✅ Check `caller == owner` before any operation
   - ✅ Validate status before mutation
   - ✅ Consistent unauthorized error messages

5. **Time & Updates**
   - ✅ Update `updated_at` timestamp on mutation
   - ✅ Set `btc_withdraw_txid` for audit trail

6. **Partial Withdrawal Support**
   - ✅ Allow amount < balance
   - ✅ Maintain `Unlockable` status until balance = 0
   - ✅ Transition to `Withdrawn` only when fully depleted

---

## 9. State Machine Diagram

```
┌─────────────────┐
│ PendingDeposit  │
└────────┬────────┘
         │ mock_deposit_vault()
         ▼
┌─────────────────┐
│  ActiveLocked   │
└────────┬────────┘
         │ unlock_vault()
         ▼
┌─────────────────┐
│   Unlockable    │◄──────┐
└────────┬────────┘       │
         │                │
         │ withdraw_vault(partial)
         │                │
         └────────────────┘
         │
         │ withdraw_vault(full, balance=0)
         ▼
┌─────────────────┐
│   Withdrawn     │◄──── final state
└─────────────────┘
```

---

## 10. Key Takeaways

### ✅ What We Implemented

1. **Query Function**: `preview_withdraw()` - Check balance without mutation
2. **Update Function**: `withdraw_vault()` - Execute partial or full withdrawal
3. **Query Function**: `get_withdrawable_vaults()` - Batch lookup
4. **Mock Transaction ID**: Simulate Bitcoin withdrawal with unique ID
5. **Dual Event Logging**: Track request and completion separately
6. **Comprehensive Validation**: All edge cases covered

### 🎯 Design Decisions

1. **Partial Withdrawal Support**
   - Stay in `Unlockable` status as long as balance > 0
   - Enable multiple withdrawals from same vault
   - Useful for gradual fund release

2. **Mock Transaction ID**
   - Format: `MOCK-TXID-<vault_id>`
   - Set on first withdrawal
   - Remains consistent across partial withdrawals

3. **Dual Event Pattern**
   - `WITHDRAW_REQUESTED`: Initiation with amount
   - `WITHDRAW_COMPLETED`: Success with remaining balance or completion status

4. **Security**
   - Ownership checks mandatory
   - Status validation before withdrawal
   - Amount validation (> 0, ≤ balance)

### ⚠️ Important Notes

- 💰 Supports partial withdrawals (amount < balance)
- 🔐 Only vault owner can preview/withdraw
- 📝 Events logged after successful mutations
- ✅ Status transitions validated strictly
- 🔄 Can withdraw multiple times until balance = 0

### 🚀 Integration Points

For production Bitcoin/ckBTC integration:

- Replace mock transaction ID with actual BTC/ckBTC transfer
- Add transaction confirmation logic
- Implement fee calculation and deduction
- Add retry mechanism for failed transfers
- Store actual transaction hashes

---

## Summary

Module 3 completes the withdraw flow with full lifecycle support:

- 3 new functions (2 queries, 1 update) with comprehensive validations.
- Partial withdrawal support enabling flexible fund management.
- Dual event logging (`WITHDRAW_REQUESTED`, `WITHDRAW_COMPLETED`) for audit trail.
- Error handling for all edge cases with clear messages.

The vault now supports complete lifecycle: create → deposit → lock → unlock → withdraw (partial/full) with full event history and security. Ready for production Bitcoin/ckBTC integration! 🎉
