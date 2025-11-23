#!/bin/bash

# ============================================================================
# IRONCLAD VAULT - Local ckBTC Ledger Deployment Script
# ============================================================================
# This script deploys a real ICRC-1 ledger canister locally that mimics ckBTC
# for local development and testing purposes.
#
# Requirements:
# - dfx CLI installed
# - Internet connection (for downloading WASM and Candid files)
#
# Usage:
#   chmod +x scripts/deploy_local_ckbtc.sh
#   ./scripts/deploy_local_ckbtc.sh
# ============================================================================

set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================================${NC}"
echo -e "${BLUE}  Ironclad Vault - ckBTC Ledger Deployment${NC}"
echo -e "${BLUE}================================================${NC}"
echo ""

# ============================================================================
# Step 1: Check if dfx is running
# ============================================================================
echo -e "${YELLOW}[1/5] Checking dfx replica status...${NC}"

if dfx ping > /dev/null 2>&1; then
    echo -e "${GREEN}✓ dfx replica is running${NC}"
else
    echo -e "${YELLOW}⚠ dfx replica is not running. Starting clean replica...${NC}"
    dfx start --background --clean
    sleep 3
    
    if dfx ping > /dev/null 2>&1; then
        echo -e "${GREEN}✓ dfx replica started successfully${NC}"
    else
        echo -e "${RED}✗ Failed to start dfx replica${NC}"
        exit 1
    fi
fi

echo ""

# ============================================================================
# Step 2: Get current user's principal
# ============================================================================
echo -e "${YELLOW}[2/5] Retrieving user principal...${NC}"

export OWNER=$(dfx identity get-principal)

if [ -z "$OWNER" ]; then
    echo -e "${RED}✗ Failed to retrieve principal${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Owner Principal: ${OWNER}${NC}"
echo ""

# ============================================================================
# Step 3: Deploy ckBTC Ledger with Mainnet ID
# ============================================================================
echo -e "${YELLOW}[3/5] Deploying ckBTC Ledger canister...${NC}"
echo -e "${BLUE}   Using Mainnet ID: mxzaz-hqaaa-aaaar-qaada-cai${NC}"

# Initial balance: 100,000 ckBTC (100,000 * 10^8 e8s = 10,000,000,000,000 e8s)
INITIAL_BALANCE="10_000_000_000_000"

# Deploy with exact Mainnet ID for consistency
dfx deploy ckbtc_ledger --specified-id mxzaz-hqaaa-aaaar-qaada-cai --argument "(variant { 
  Init = record {
    token_symbol = \"ckBTC\";
    token_name = \"Chain Key Bitcoin\";
    minting_account = record { owner = principal \"$OWNER\" };
    transfer_fee = 10;
    metadata = vec {};
    feature_flags = opt record { icrc2 = true };
    initial_balances = vec { 
      record { 
        record { owner = principal \"$OWNER\" }; 
        $INITIAL_BALANCE
      } 
    };
    archive_options = record { 
      num_blocks_to_archive = 1000; 
      trigger_threshold = 2000; 
      controller_id = principal \"$OWNER\";
      cycles_for_archive_creation = opt 1_000_000_000_000;
    };
  }
})"

if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ ckBTC Ledger deployed successfully${NC}"
else
    echo -e "${RED}✗ Failed to deploy ckBTC Ledger${NC}"
    exit 1
fi

echo ""

# ============================================================================
# Step 4: Verify Deployment
# ============================================================================
echo -e "${YELLOW}[4/5] Verifying deployment...${NC}"

# Get canister ID
CANISTER_ID=$(dfx canister id ckbtc_ledger 2>/dev/null)

if [ -z "$CANISTER_ID" ]; then
    echo -e "${RED}✗ Failed to retrieve canister ID${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Canister ID: ${CANISTER_ID}${NC}"

# Verify balance
echo -e "${YELLOW}   Checking initial balance...${NC}"

BALANCE=$(dfx canister call ckbtc_ledger icrc1_balance_of "(record { owner = principal \"$OWNER\"; subaccount = null })")

if [ -z "$BALANCE" ]; then
    echo -e "${RED}✗ Failed to retrieve balance${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Initial Balance: ${BALANCE}${NC}"
echo ""

# ============================================================================
# Step 5: Display Configuration Summary
# ============================================================================
echo -e "${YELLOW}[5/5] Deployment Summary${NC}"
echo -e "${BLUE}================================================${NC}"
echo -e "${GREEN}✓ Deployment Complete!${NC}"
echo ""
echo -e "Configuration Details:"
echo -e "  Token:           ckBTC (Chain Key Bitcoin)"
echo -e "  Canister ID:     ${CANISTER_ID}"
echo -e "  Owner:           ${OWNER}"
echo -e "  Initial Balance: 100,000 ckBTC"
echo -e "  Transfer Fee:    10 e8s (0.0000001 ckBTC)"
echo ""
echo -e "Frontend Environment Variables:"
echo -e "  ${BLUE}NEXT_PUBLIC_CKBTC_LEDGER_ID=${CANISTER_ID}${NC}"
echo -e "  ${BLUE}NEXT_PUBLIC_IC_HOST=http://127.0.0.1:4943${NC}"
echo ""
echo -e "Verification Commands:"
echo -e "  ${YELLOW}# Check balance${NC}"
echo -e "  dfx canister call ckbtc_ledger icrc1_balance_of '(record { owner = principal \"$OWNER\"; subaccount = null })'"
echo ""
echo -e "  ${YELLOW}# Get token metadata${NC}"
echo -e "  dfx canister call ckbtc_ledger icrc1_metadata '()'"
echo ""
echo -e "  ${YELLOW}# Check token symbol${NC}"
echo -e "  dfx canister call ckbtc_ledger icrc1_symbol '()'"
echo ""
echo -e "${BLUE}================================================${NC}"
echo -e "${GREEN}Ready for local development!${NC}"
echo ""
