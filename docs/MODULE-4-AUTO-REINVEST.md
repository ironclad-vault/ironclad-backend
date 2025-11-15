# MODULE-4: AUTO-REINVEST SYSTEM

## Overview
This module documents the Auto-Reinvest system for the Ironclad Vault canister. The Auto-Reinvest feature allows users to automatically reinvest their unlocked vault balance into a new locked vault, streamlining the reinvestment process and improving user experience.

## Features
- **AutoReinvestConfig Struct**: Stores configuration for scheduled auto-reinvest actions.
- **State Extension**: Maintains a list of auto-reinvest configurations in canister state.
- **Lifecycle Functions**:
  - `schedule_auto_reinvest`: Schedule an auto-reinvest action for a vault.
  - `cancel_auto_reinvest`: Cancel a scheduled auto-reinvest action.
  - `get_auto_reinvest_config`: Retrieve the auto-reinvest configuration for a specific vault.
  - `get_my_auto_reinvest_configs`: List all auto-reinvest configurations for the caller.
  - `execute_auto_reinvest`: Execute the auto-reinvest action when the vault is unlockable.
- **Error Handling**: All functions return `Result<T, String>` with descriptive error messages.
- **Event Logging**: All state mutations are logged as events for auditability.
- **Candid Interface**: The DID file is updated to include the new record type and service methods.

## Usage Flow
1. **Schedule Auto-Reinvest**
   - Call `schedule_auto_reinvest` with the target vault and desired lock duration.
   - The system creates an `AutoReinvestConfig` and stores it in state.
   - An event `AUTO_REINVEST_SCHEDULED` is logged.

2. **Cancel Auto-Reinvest**
   - Call `cancel_auto_reinvest` to remove a scheduled auto-reinvest action.
   - The configuration is removed from state.
   - An event `AUTO_REINVEST_CANCELLED` is logged.

3. **Execute Auto-Reinvest**
   - When the vault becomes unlockable, call `execute_auto_reinvest`.
   - The system withdraws the balance and creates a new locked vault with the specified duration.
   - The source vault status is updated to `Withdrawn`.
   - The auto-reinvest config is disabled.
   - Events `AUTO_REINVEST_EXECUTED_SOURCE` and `AUTO_REINVEST_EXECUTED_TARGET` are logged.

4. **Query Configurations**
   - Use `get_auto_reinvest_config` and `get_my_auto_reinvest_configs` to view scheduled actions.

## Example
```rust
// Schedule auto-reinvest
schedule_auto_reinvest(vault_id, lock_duration)

// Execute auto-reinvest when unlockable
execute_auto_reinvest(vault_id)

// Cancel auto-reinvest
cancel_auto_reinvest(vault_id)
```

## Event Types
- `AUTO_REINVEST_SCHEDULED`
- `AUTO_REINVEST_CANCELLED`
- `AUTO_REINVEST_EXECUTED_SOURCE`
- `AUTO_REINVEST_EXECUTED_TARGET`

## Error Handling
All functions return errors as `Result<T, String>`, e.g.:
- "Vault not found"
- "Vault not unlockable"
- "Auto-reinvest config not found"
- "Caller not authorized"

## Auditability
All auto-reinvest actions are logged as events and can be queried for full audit trails.

## Candid Interface
The DID file includes:
- `record AutoReinvestConfig { ... }`
- Service methods for all auto-reinvest functions

## Best Practices
- Always check vault status before executing auto-reinvest.
- Use event logs to verify actions and state changes.
- Handle errors gracefully and provide clear messages to users.

---
**End of Module 4 Documentation**
