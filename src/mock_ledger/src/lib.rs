use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk_macros::{query, update};
use std::cell::RefCell;
use std::collections::HashMap;

/// Mock ICRC-1 ledger for local testing
/// Simulates ckBTC ledger balance lookups

#[derive(CandidType, Deserialize, Clone)]
pub struct Icrc1Account {
    pub owner: Principal,
    pub subaccount: Option<Vec<u8>>,
}

// In-memory balance storage for testing
thread_local! {
    static BALANCES: RefCell<HashMap<(Principal, Option<Vec<u8>>), u64>> = RefCell::new(HashMap::new());
}

/// Get balance for an account (matches ICRC-1 standard)
#[query]
fn icrc1_balance_of(account: Icrc1Account) -> Nat {
    BALANCES.with(|b| {
        let balance = b.borrow()
            .get(&(account.owner, account.subaccount))
            .copied()
            .unwrap_or(0);
        Nat::from(balance)
    })
}

/// Set balance for testing (admin function)
#[update]
fn set_balance(owner: Principal, subaccount: Option<Vec<u8>>, amount: u64) {
    BALANCES.with(|b| {
        b.borrow_mut().insert((owner, subaccount), amount);
    });
}

/// Simulate a deposit to a subaccount (for testing)
#[update]
fn simulate_deposit(owner: Principal, subaccount: Option<Vec<u8>>, amount: u64) -> Nat {
    BALANCES.with(|b| {
        let mut balances = b.borrow_mut();
        let current = balances.get(&(owner.clone(), subaccount.clone())).copied().unwrap_or(0);
        let new_balance = current + amount;
        balances.insert((owner, subaccount), new_balance);
        Nat::from(new_balance)
    })
}
