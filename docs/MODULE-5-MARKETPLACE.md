# MODULE-5: MARKETPLACE SYSTEM

## Overview

This module documents the Marketplace system for the Ironclad Vault canister. The Marketplace allows users to list their vaults for sale, browse active listings, and purchase vaults from other users. When a vault is sold, ownership is transferred and any active auto-reinvest configurations are automatically disabled.

## Features

- **ListingStatus Enum**: Tracks the lifecycle of a listing (Active, Cancelled, Filled).
- **MarketListing Struct**: Stores information about vault listings including price, seller, buyer, and status.
- **State Extension**: Maintains a list of market listings and listing ID counter in canister state.
- **Lifecycle Functions**:
  - `create_listing`: List a vault for sale at a specified price.
  - `cancel_listing`: Cancel an active listing (seller only).
  - `get_active_listings`: Browse all active listings on the marketplace.
  - `get_my_listings`: View all listings created by the caller.
  - `buy_listing`: Purchase a listed vault and transfer ownership.
- **Error Handling**: All functions return `Result<T, String>` with descriptive error messages.
- **Event Logging**: All marketplace actions are logged as events for auditability.
- **Auto-Reinvest Integration**: When a vault is sold, any active auto-reinvest configuration is automatically disabled.
- **Candid Interface**: The DID file is updated to include the new types and service methods.

## Types

### ListingStatus
```rust
enum ListingStatus {
    Active,      // Listing is available for purchase
    Cancelled,   // Listing was cancelled by seller
    Filled,      // Listing was successfully purchased
}
```

### MarketListing
```rust
struct MarketListing {
    id: u64,                    // Unique listing ID
    vault_id: u64,              // ID of the vault being sold
    seller: Principal,          // Original vault owner
    buyer: Option<Principal>,   // Buyer (set when Filled)
    price_sats: u64,            // Sale price in satoshis
    status: ListingStatus,      // Current listing status
    created_at: u64,            // Creation timestamp
    updated_at: u64,            // Last update timestamp
}
```

## Usage Flow

### 1. Create Listing

**Requirements:**
- Caller must own the vault
- Vault status cannot be `PendingDeposit` or `Withdrawn`
- Price must be greater than 0
- No active listing already exists for the vault

**Process:**
```rust
create_listing(vault_id, price_sats) -> Result<MarketListing, String>
```

**Effects:**
- Creates new `MarketListing` with status `Active`
- Increments listing ID counter
- Logs `VAULT_LISTED` event

### 2. Browse Listings

**Query Active Listings:**
```rust
get_active_listings() -> Vec<MarketListing>
```
Returns all listings with status `Active`.

**Query Own Listings:**
```rust
get_my_listings() -> Vec<MarketListing>
```
Returns all listings created by the caller (any status).

### 3. Cancel Listing

**Requirements:**
- Caller must be the seller
- Listing must be `Active`

**Process:**
```rust
cancel_listing(listing_id) -> Result<MarketListing, String>
```

**Effects:**
- Sets status to `Cancelled`
- Updates `updated_at` timestamp
- Logs `VAULT_LISTING_CANCELLED` event

### 4. Buy Listing

**Requirements:**
- Listing must exist and be `Active`
- Buyer cannot be the seller
- Vault must exist
- Vault status cannot be `PendingDeposit` or `Withdrawn`

**Process:**
```rust
buy_listing(listing_id) -> Result<Vault, String>
```

**Effects:**
- Transfers vault ownership to buyer
- Updates vault `owner` and `updated_at`
- Sets listing status to `Filled`
- Sets listing `buyer` to the purchaser
- Updates listing `updated_at`
- Disables any active auto-reinvest configuration for the vault
- Logs `VAULT_SOLD` event

## Example Usage

```rust
// Create and deposit to a vault
let vault = create_vault(lock_until, expected_deposit);
mock_deposit_vault(vault.id, amount);

// List the vault for sale
let listing = create_listing(vault.id, 50000)?;

// Browse active listings
let listings = get_active_listings();

// Buy a listing (different user)
let vault = buy_listing(listing.id)?;

// Cancel a listing
cancel_listing(listing.id)?;
```

## Event Types

- `VAULT_LISTED`: Vault was listed for sale
- `VAULT_LISTING_CANCELLED`: Listing was cancelled by seller
- `VAULT_SOLD`: Vault was successfully sold and ownership transferred

## Error Messages

All functions follow consistent error patterns:

**create_listing:**
- "Price must be greater than 0"
- "Unauthorized: You don't own this vault"
- "Vault not found"
- "Cannot list vault in PendingDeposit status"
- "Cannot list withdrawn vault"
- "Vault already has an active listing"

**cancel_listing:**
- "Unauthorized: You don't own this listing"
- "Listing not found"
- "Listing is not active"

**buy_listing:**
- "Listing not found"
- "Listing is not active"
- "Cannot buy your own listing"
- "Vault not found"
- "Cannot buy vault in PendingDeposit status"
- "Cannot buy withdrawn vault"

## Integration with Auto-Reinvest

When a vault with an active auto-reinvest configuration is sold:
1. The vault ownership is transferred to the buyer
2. The auto-reinvest config's `enabled` field is set to `false`
3. The config's `updated_at` timestamp is updated
4. The config remains in state but is no longer active

This ensures that:
- Sellers cannot trigger auto-reinvest after selling
- Buyers receive clean vaults without unexpected automation
- Historical auto-reinvest configs are preserved for audit trails

## Candid Interface

```candid
type ListingStatus = variant {
    Active;
    Cancelled;
    Filled;
};

type MarketListing = record {
    id: nat64;
    vault_id: nat64;
    seller: principal;
    buyer: opt principal;
    price_sats: nat64;
    status: ListingStatus;
    created_at: nat64;
    updated_at: nat64;
};

service : {
    create_listing: (nat64, nat64) -> (variant { Ok: MarketListing; Err: text });
    cancel_listing: (nat64) -> (variant { Ok: MarketListing; Err: text });
    get_active_listings: () -> (vec MarketListing) query;
    get_my_listings: () -> (vec MarketListing) query;
    buy_listing: (nat64) -> (variant { Ok: Vault; Err: text });
}
```

## Best Practices

1. **Always check active listings** before creating a new one for the same vault.
2. **Verify vault status** before attempting to list (must be locked or unlockable).
3. **Use different identities** for testing buy/sell flows.
4. **Check event logs** to verify all marketplace actions.
5. **Query listing status** before attempting to cancel or buy.
6. **Consider auto-reinvest** implications when buying vaults - configs are automatically disabled.

## Security Considerations

- **Ownership Validation**: All functions verify caller ownership before mutations.
- **Status Checks**: Prevents listing of incomplete or withdrawn vaults.
- **Self-Trading Prevention**: Buyers cannot purchase their own listings.
- **Atomic Transfers**: Ownership transfer and listing status update happen atomically.
- **Auto-Reinvest Safety**: Automatically disables automation when ownership changes.

## Auditability

All marketplace actions are logged as events with:
- Action type (VAULT_LISTED, VAULT_LISTING_CANCELLED, VAULT_SOLD)
- Vault ID
- Timestamp
- Descriptive notes

Events can be queried using `get_vault_events(vault_id)` by the current vault owner.

---

**End of Module 5 Documentation**
