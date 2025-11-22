#!/bin/bash

# ============================================================================
# Transfer ckBTC to a Principal
# ============================================================================
# Usage:
#   ./scripts/transfer_ckbtc.sh <recipient_principal> <amount_in_btc>
#
# Example:
#   ./scripts/transfer_ckbtc.sh "j2any-diae3-2z3np-wpawk-p5zrc-fkbey-ejlmk-337xu-dan52-bkjmj-uqe" 1000
# ============================================================================

set -e

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}================================================${NC}"
echo -e "${BLUE}  ckBTC Transfer Script${NC}"
echo -e "${BLUE}================================================${NC}"
echo ""

# Check arguments
if [ -z "$1" ]; then
    echo -e "${RED}Error: Recipient principal required${NC}"
    echo ""
    echo "Usage: $0 <recipient_principal> <amount_in_btc>"
    echo ""
    echo "Example:"
    echo "  $0 \"j2any-diae3-2z3np-wpawk-p5zrc-fkbey-ejlmk-337xu-dan52-bkjmj-uqe\" 1000"
    echo ""
    exit 1
fi

RECIPIENT="$1"
AMOUNT_BTC="${2:-1000}"  # Default 1000 BTC if not specified

# Convert BTC to e8s (1 BTC = 100,000,000 e8s)
AMOUNT_E8S=$(echo "$AMOUNT_BTC * 100000000" | bc)

echo -e "${YELLOW}Transfer Details:${NC}"
echo -e "  From:   $(dfx identity get-principal)"
echo -e "  To:     $RECIPIENT"
echo -e "  Amount: $AMOUNT_BTC BTC ($AMOUNT_E8S e8s)"
echo ""

# Check sender balance
echo -e "${YELLOW}Checking sender balance...${NC}"
SENDER_PRINCIPAL=$(dfx identity get-principal)
SENDER_BALANCE=$(dfx canister call ckbtc_ledger icrc1_balance_of "(record { owner = principal \"$SENDER_PRINCIPAL\"; subaccount = null })")

echo -e "  Sender Balance: $SENDER_BALANCE"
echo ""

# Execute transfer
echo -e "${YELLOW}Executing transfer...${NC}"

RESULT=$(dfx canister call ckbtc_ledger icrc1_transfer "(record {
  to = record {
    owner = principal \"$RECIPIENT\";
    subaccount = null;
  };
  amount = $AMOUNT_E8S : nat;
  fee = null;
  memo = null;
  from_subaccount = null;
  created_at_time = null;
})")

if echo "$RESULT" | grep -q "Ok"; then
    TX_INDEX=$(echo "$RESULT" | sed -n 's/.*(Ok = \([0-9]*\).*/\1/p' | head -1)
    echo -e "${GREEN}✓ Transfer successful!${NC}"
    echo -e "  Transaction Index: $TX_INDEX"
    echo ""
    
    # Verify recipient balance
    echo -e "${YELLOW}Verifying recipient balance...${NC}"
    NEW_BALANCE=$(dfx canister call ckbtc_ledger icrc1_balance_of "(record { owner = principal \"$RECIPIENT\"; subaccount = null })")
    echo -e "  Recipient Balance: $NEW_BALANCE"
    echo ""
    
    echo -e "${GREEN}================================================${NC}"
    echo -e "${GREEN}Transfer Complete!${NC}"
    echo -e "${GREEN}================================================${NC}"
else
    echo -e "${RED}✗ Transfer failed!${NC}"
    echo -e "  Result: $RESULT"
    exit 1
fi
