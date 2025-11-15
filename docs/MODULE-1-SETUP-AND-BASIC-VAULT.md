# Module 1: Setup & Basic Vault Management

## Summary
This module completes local setup and vault feature foundation up to ActiveLocked stage.

- Goal: deployment-ready environment, basic vault flow (create → deposit → ActiveLocked).
- Key fixes: upgrade `ic-cdk`/`ic-cdk-macros` to 0.18, replace `caller()` → `msg_caller()`, fix invalid `.did`, add `wasm32-unknown-unknown` target.
- Security & architecture: avoid nested `RefCell` borrow, call `record_event()` outside `with_state_mut()`, use `Result<T, String>` for clear errors.
- Available APIs: `create_vault`, `get_my_vaults`, `get_vault`, `get_vault_events`, `mock_deposit_vault` (Result).
- Quick try:

```bash
dfx canister call ironclad_vault_backend create_vault '(1731700000, 100000)'
dfx canister call ironclad_vault_backend mock_deposit_vault '(0, 50000)'
dfx canister call ironclad_vault_backend get_vault_events '(0)'
```

---

## 1. Setup & Dependency Fixes

### Problem Encountered
When first deploying with `dfx deploy ironclad_vault_backend`, an error occurred:

```
error: failed to select a version for `ic-cdk-executor`.
package `ic-cdk-executor` links to the native library `ic-cdk async executor`
but it conflicts with a previous package
```

**Root Cause:**
- `ic-cdk = "0.15"` uses `ic-cdk-executor = "^0.1.0"`
- `ic-cdk-timers = "0.12"` requires `ic-cdk = "0.18.x"` which uses `ic-cdk-executor = "^1.0.0"`
- Two different versions of `ic-cdk-executor` cannot coexist

### Solution
Update dependencies in `Cargo.toml`:

```toml
[dependencies]
candid = "0.10"
ic-cdk = "0.18"           # ← Updated from 0.15
ic-cdk-macros = "0.18"     # ← Updated from 0.15
serde = { version = "1", features = ["derive"] }
ic-cdk-timers = "0.12"
```

**Steps:**
1. Update `Cargo.toml`
2. Remove `Cargo.lock` and `target/`: `rm -rf Cargo.lock target/ .dfx/`
3. Regenerate lock file: `cargo update`
4. Install WASM target: `rustup target add wasm32-unknown-unknown`
5. Deploy: `dfx deploy ironclad_vault_backend`

---

## 2. Fixing Deprecation Warnings

### Problem
After successful deployment, 4 warnings appeared:

```
warning: use of deprecated function `ic_cdk::caller`: Use `msg_caller` instead
```

### Solution
1. **Import `msg_caller` from `ic_cdk::api`:**
   ```rust
   use ic_cdk::api::{msg_caller, time};
   ```

2. **Replace all `ic_cdk::caller()` with `msg_caller()`:**
   - In `create_vault()` - line 73
   - In `get_my_vaults()` - line 96
   - In `get_vault()` - line 109
   - In `mock_update_vault()` - line 122

**Result:** Build succeeded without warnings.

---

## 3. Fixing Invalid Candid Interface

### Problem
The `.did` file uses invalid placeholder `...`:

```candid
type VaultStatus = variant { ... };  // ❌ Invalid syntax
```

Error:
```
error: parser error
Expected one of "decimal", "hex", "id", "text" or "}"
```

### Solution
Create a valid `.did` file with complete definitions:

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
```

---

## 4. Fixing RefCell Borrow Error

### Problem
Error during `create_vault` test:

```
Panicked at 'RefCell already borrowed'
```

**Root Cause:**
`record_event()` is called **inside** `with_state_mut()`, causing nested borrow on `STATE`:

```rust
with_state_mut(|state| {
    // ... mutate state
    record_event(id, "VAULT_CREATED", "..."); // ❌ Nested borrow!
    vault
})
```

### Solution
Move `record_event()` **outside** of `with_state_mut()`:

```rust
let vault = with_state_mut(|state| {
    // ... mutate state
    vault
});

// ✅ Call record_event outside
record_event(vault.id, "VAULT_CREATED", "Vault created in PendingDeposit state");
vault
```

**Same pattern applied in:**
- `create_vault()`
- `mock_deposit_vault()`

---

## 5. Improving Error Handling

### Problem
`mock_deposit_vault()` uses `Option<Vault>` which is not informative:
- `Some(vault)` - success
- `None` - failure (but why?)

### Solution
Replace with `Result<Vault, String>`:

```rust
#[update]
fn mock_deposit_vault(id: u64, amount: u64) -> Result<Vault, String> {
    let caller = msg_caller();
    let ts = now_sec();

    let result = with_state_mut(|state| {
        let vault = match state.vaults.iter_mut().find(|v| v.id == id) {
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
        record_event(id, "MOCK_DEPOSIT", &format!("Mock deposit of {} satoshis", amount));
        record_event(id, "LOCK_STARTED", "Vault moved to ActiveLocked");
    }

    result
}
```

**Benefits:**
- Clear and specific error messages
- Input validation (amount > 0)
- Proper ownership check

---

## 6. Architecture & State Management

### State Structure
```rust
#[derive(Default)]
pub struct State {
    pub next_id: u64,
    pub vaults: Vec<Vault>,
    pub history: Vec<VaultEvent>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}
```

### Helper Functions
```rust
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
```

**Key Rules:**
- ✅ Always use helper functions, never access `STATE` directly
- ✅ Call `record_event()` **outside** `with_state_mut()`
- ✅ Use `now_sec()` instead of `time()` directly

---

## 7. Implemented Functions

### `create_vault(lock_until: u64, expected_deposit: u64) -> Vault`
**Type:** Update function

**Purpose:**
- Create new vault with timelock
- Generate unique ID and placeholder BTC address
- Initial status: `PendingDeposit`
- Log event: `VAULT_CREATED`

**Example:**
```bash
dfx canister call ironclad_vault_backend create_vault '(1731700000, 100000)'
```

---

### `get_my_vaults() -> Vec<Vault>`
**Type:** Query function

**Purpose:**
- Get all vaults owned by caller
- Filter by owner

**Example:**
```bash
dfx canister call ironclad_vault_backend get_my_vaults '()'
```

---

### `get_vault(id: u64) -> Option<Vault>`
**Type:** Query function

**Purpose:**
- Get vault by ID
- Only if owned by caller

**Example:**
```bash
dfx canister call ironclad_vault_backend get_vault '(0)'
```

---

### `get_vault_events(id: u64) -> Vec<VaultEvent>`
**Type:** Query function

**Purpose:**
- Get event history for vault
- Only if owned by caller

**Example:**
```bash
dfx canister call ironclad_vault_backend get_vault_events '(0)'
```

---

### `mock_deposit_vault(id: u64, amount: u64) -> Result<Vault, String>`
**Type:** Update function

**Purpose:**
- Simulate deposit (for testing)
- Update balance and status to `ActiveLocked`
- Validation: amount > 0, ownership check
- Log events: `MOCK_DEPOSIT`, `LOCK_STARTED`

**Example:**
```bash
# Success
dfx canister call ironclad_vault_backend mock_deposit_vault '(0, 50000)'
# Output: (variant { Ok = record { ... } })

# Error: amount 0
dfx canister call ironclad_vault_backend mock_deposit_vault '(0, 0)'
# Output: (variant { Err = "Deposit amount must be greater than 0" })

# Error: vault not found
dfx canister call ironclad_vault_backend mock_deposit_vault '(999, 10000)'
# Output: (variant { Err = "Vault not found" })
```

---

## 8. Testing Summary

### Test Flow
```bash
# 1. Create vault
dfx canister call ironclad_vault_backend create_vault '(1731700000, 100000)'

# 2. Get my vaults
dfx canister call ironclad_vault_backend get_my_vaults '()'

# 3. Deposit
dfx canister call ironclad_vault_backend mock_deposit_vault '(0, 50000)'

# 4. Get events
dfx canister call ironclad_vault_backend get_vault_events '(0)'
```

### Expected Output
```
Events:
1. VAULT_CREATED - "Vault created in PendingDeposit state"
2. MOCK_DEPOSIT - "Mock deposit of 50000 satoshis"
3. LOCK_STARTED - "Vault moved to ActiveLocked"
```

---

## Key Takeaways

### ✅ What We Learned
1. **Dependency Management:** IC-CDK version compatibility matters
2. **RefCell Borrow Rules:** Never nest `STATE` borrows
3. **Error Handling:** Use `Result<T, String>` for better UX
4. **State Pattern:** Always use helper functions
5. **Event Logging:** Call outside state mutations

### ⚠️ Common Pitfalls
- ❌ Calling `record_event()` inside `with_state_mut()`
- ❌ Using deprecated `ic_cdk::caller()`
- ❌ Using `Option` instead of `Result` for errors
- ❌ Forgetting to update `updated_at` timestamp
- ❌ Not checking ownership before mutation

### 🎯 Best Practices
- ✅ Use `msg_caller()` from `ic_cdk::api`
- ✅ Use `now_sec()` helper for timestamps
- ✅ Pattern match with ownership checks
- ✅ Log events after successful mutations
- ✅ Return descriptive error messages

---

## Next Steps
Module 2 will add **timelock logic** and **unlock state machine** to enable vault unlocking after timelock expires.
