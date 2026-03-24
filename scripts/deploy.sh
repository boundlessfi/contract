#!/usr/bin/env bash
# Boundless Platform — Ordered Deployment & Wiring Script
# Usage: ./scripts/deploy.sh [testnet|mainnet]
#
# Prerequisites:
#   - stellar CLI installed
#   - ADMIN_SECRET set in environment (deployer/admin keypair secret)
#   - TREASURY_ADDRESS set in environment (treasury public key)
#   - Contracts built via `stellar contract build`

set -euo pipefail

NETWORK="${1:-testnet}"
WASM_DIR="target/wasm32v1-none/release"

# Validate environment
if [[ -z "${ADMIN_SECRET:-}" ]]; then
  echo "ERROR: ADMIN_SECRET environment variable required"
  exit 1
fi
if [[ -z "${TREASURY_ADDRESS:-}" ]]; then
  echo "ERROR: TREASURY_ADDRESS environment variable required"
  exit 1
fi

ADMIN_ADDRESS=$(stellar keys address "$ADMIN_SECRET" 2>/dev/null || echo "$ADMIN_SECRET")

echo "=== Boundless Platform Deployment ==="
echo "Network:  $NETWORK"
echo "Admin:    $ADMIN_ADDRESS"
echo "Treasury: $TREASURY_ADDRESS"
echo ""

# Helper: deploy a contract and return its ID
deploy_contract() {
  local name="$1"
  local wasm="$WASM_DIR/${name}.wasm"

  if [[ ! -f "$wasm" ]]; then
    echo "ERROR: WASM not found: $wasm (run 'stellar contract build' first)"
    exit 1
  fi

  echo "Deploying $name..."
  local contract_id
  contract_id=$(stellar contract deploy \
    --wasm "$wasm" \
    --source "$ADMIN_SECRET" \
    --network "$NETWORK")
  echo "  -> $contract_id"
  echo "$contract_id"
}

# Helper: invoke a contract function
invoke() {
  local contract_id="$1"
  shift
  stellar contract invoke \
    --id "$contract_id" \
    --source "$ADMIN_SECRET" \
    --network "$NETWORK" \
    -- "$@"
}

echo ""
echo "=========================================="
echo "Step 1: Deploy Infrastructure Contracts"
echo "=========================================="

CORE_ESCROW_ID=$(deploy_contract "core_escrow")
REPUTATION_ID=$(deploy_contract "reputation_registry")
GOVERNANCE_ID=$(deploy_contract "governance_voting")

echo ""
echo "=========================================="
echo "Step 2: Initialize Infrastructure"
echo "=========================================="

echo "Initializing CoreEscrow..."
invoke "$CORE_ESCROW_ID" init \
  --admin "$ADMIN_ADDRESS" \
  --treasury "$TREASURY_ADDRESS"

echo "Initializing ReputationRegistry..."
invoke "$REPUTATION_ID" init \
  --admin "$ADMIN_ADDRESS"

echo "Initializing GovernanceVoting..."
invoke "$GOVERNANCE_ID" init \
  --admin "$ADMIN_ADDRESS"

echo ""
echo "=========================================="
echo "Step 3: Deploy Module Registries"
echo "=========================================="

PROJECT_REGISTRY_ID=$(deploy_contract "project_registry")
BOUNTY_REGISTRY_ID=$(deploy_contract "bounty_registry")
CROWDFUND_REGISTRY_ID=$(deploy_contract "crowdfund_registry")
GRANT_HUB_ID=$(deploy_contract "grant_hub")
HACKATHON_REGISTRY_ID=$(deploy_contract "hackathon_registry")

echo ""
echo "=========================================="
echo "Step 4: Initialize Module Registries"
echo "=========================================="

echo "Initializing ProjectRegistry..."
invoke "$PROJECT_REGISTRY_ID" init \
  --admin "$ADMIN_ADDRESS"

echo "Initializing BountyRegistry..."
invoke "$BOUNTY_REGISTRY_ID" init \
  --admin "$ADMIN_ADDRESS" \
  --core_escrow "$CORE_ESCROW_ID" \
  --reputation_registry "$REPUTATION_ID"

echo "Initializing CrowdfundRegistry..."
invoke "$CROWDFUND_REGISTRY_ID" init \
  --admin "$ADMIN_ADDRESS" \
  --core_escrow "$CORE_ESCROW_ID" \
  --reputation_registry "$REPUTATION_ID"

echo "Initializing GrantHub..."
invoke "$GRANT_HUB_ID" init \
  --admin "$ADMIN_ADDRESS" \
  --core_escrow "$CORE_ESCROW_ID" \
  --reputation_registry "$REPUTATION_ID" \
  --governance_voting "$GOVERNANCE_ID"

echo "Initializing HackathonRegistry..."
invoke "$HACKATHON_REGISTRY_ID" init \
  --admin "$ADMIN_ADDRESS" \
  --core_escrow "$CORE_ESCROW_ID" \
  --reputation_registry "$REPUTATION_ID"

echo ""
echo "=========================================="
echo "Step 5: Wire Authorization"
echo "=========================================="

echo "Authorizing modules on CoreEscrow..."
for module_id in "$BOUNTY_REGISTRY_ID" "$CROWDFUND_REGISTRY_ID" "$GRANT_HUB_ID" "$HACKATHON_REGISTRY_ID"; do
  invoke "$CORE_ESCROW_ID" authorize_module --module_addr "$module_id"
done

echo "Authorizing modules on ReputationRegistry..."
for module_id in "$BOUNTY_REGISTRY_ID" "$CROWDFUND_REGISTRY_ID" "$GRANT_HUB_ID" "$HACKATHON_REGISTRY_ID"; do
  invoke "$REPUTATION_ID" add_authorized_module --module "$module_id"
done

echo "Authorizing modules on GovernanceVoting..."
for module_id in "$CROWDFUND_REGISTRY_ID" "$GRANT_HUB_ID" "$HACKATHON_REGISTRY_ID"; do
  invoke "$GOVERNANCE_ID" add_authorized_module --module "$module_id"
done

echo "Authorizing modules on ProjectRegistry..."
for module_id in "$BOUNTY_REGISTRY_ID" "$CROWDFUND_REGISTRY_ID" "$GRANT_HUB_ID" "$HACKATHON_REGISTRY_ID"; do
  invoke "$PROJECT_REGISTRY_ID" add_authorized_module --module "$module_id"
done

echo ""
echo "=========================================="
echo "Deployment Complete!"
echo "=========================================="
echo ""
echo "Contract Addresses:"
echo "  CoreEscrow:          $CORE_ESCROW_ID"
echo "  ReputationRegistry:  $REPUTATION_ID"
echo "  GovernanceVoting:    $GOVERNANCE_ID"
echo "  ProjectRegistry:     $PROJECT_REGISTRY_ID"
echo "  BountyRegistry:      $BOUNTY_REGISTRY_ID"
echo "  CrowdfundRegistry:   $CROWDFUND_REGISTRY_ID"
echo "  GrantHub:            $GRANT_HUB_ID"
echo "  HackathonRegistry:   $HACKATHON_REGISTRY_ID"
echo ""
echo "Save these addresses for frontend configuration."