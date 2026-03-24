#!/usr/bin/env bash
# Boundless Platform — Contract Upgrade Script
#
# Usage:
#   ./scripts/upgrade.sh all                    # upgrade all 8 contracts
#   ./scripts/upgrade.sh core_escrow            # upgrade a single contract
#   ./scripts/upgrade.sh core_escrow bounty_registry   # upgrade specific contracts
#
# Prerequisites:
#   - stellar CLI installed
#   - ADMIN_SECRET env var set
#   - NETWORK env var set (testnet | mainnet)
#   - Contract ID env vars set (CORE_ESCROW_ID, BOUNTY_REGISTRY_ID, etc.)
#   - WASM files built via `stellar contract build`

set -euo pipefail

WASM_DIR="${WASM_DIR:-target/wasm32v1-none/release}"
NETWORK="${NETWORK:-testnet}"

if [[ -z "${ADMIN_SECRET:-}" ]]; then
  echo "ERROR: ADMIN_SECRET environment variable required"
  exit 1
fi

# ── Contract name → env var mapping ──────────────────────
declare -A CONTRACT_IDS=(
  [core_escrow]="${CORE_ESCROW_ID:-}"
  [reputation_registry]="${REPUTATION_REGISTRY_ID:-}"
  [governance_voting]="${GOVERNANCE_VOTING_ID:-}"
  [project_registry]="${PROJECT_REGISTRY_ID:-}"
  [bounty_registry]="${BOUNTY_REGISTRY_ID:-}"
  [crowdfund_registry]="${CROWDFUND_REGISTRY_ID:-}"
  [grant_hub]="${GRANT_HUB_ID:-}"
  [hackathon_registry]="${HACKATHON_REGISTRY_ID:-}"
)

# Upgrade order: infrastructure first, then modules
UPGRADE_ORDER=(
  core_escrow
  reputation_registry
  governance_voting
  project_registry
  bounty_registry
  crowdfund_registry
  grant_hub
  hackathon_registry
)

# ── Helpers ──────────────────────────────────────────────

install_wasm() {
  local wasm="$1"
  echo "  Installing WASM on-chain..."
  stellar contract install \
    --wasm "$wasm" \
    --source "$ADMIN_SECRET" \
    --network "$NETWORK"
}

upgrade_contract() {
  local name="$1"
  local contract_id="${CONTRACT_IDS[$name]:-}"
  local wasm="$WASM_DIR/${name}.wasm"

  if [[ -z "$contract_id" ]]; then
    echo "SKIP: $name — no contract ID set (${name^^}_ID)"
    return 1
  fi

  if [[ ! -f "$wasm" ]]; then
    echo "ERROR: WASM not found: $wasm"
    return 1
  fi

  echo ""
  echo "────────────────────────────────────────"
  echo "Upgrading: $name"
  echo "  Contract: $contract_id"
  echo "  WASM:     $wasm"
  echo "────────────────────────────────────────"

  # Install the new WASM and get the hash
  local wasm_hash
  wasm_hash=$(install_wasm "$wasm")
  echo "  WASM hash: $wasm_hash"

  # Call the contract's upgrade function
  echo "  Calling upgrade..."
  stellar contract invoke \
    --id "$contract_id" \
    --source "$ADMIN_SECRET" \
    --network "$NETWORK" \
    -- upgrade \
    --new_wasm_hash "$wasm_hash"

  echo "  Done: $name upgraded successfully"
}

# ── Main ─────────────────────────────────────────────────

TARGETS=("$@")

if [[ ${#TARGETS[@]} -eq 0 ]]; then
  echo "Usage: $0 <all | contract_name [contract_name ...]>"
  echo ""
  echo "Contracts: ${UPGRADE_ORDER[*]}"
  exit 1
fi

# Expand "all" to the full ordered list
if [[ "${TARGETS[0]}" == "all" ]]; then
  TARGETS=("${UPGRADE_ORDER[@]}")
fi

echo "=== Boundless Platform — Contract Upgrade ==="
echo "Network:   $NETWORK"
echo "Targets:   ${TARGETS[*]}"
echo ""

SUCCEEDED=0
FAILED=0
SKIPPED=0

for name in "${TARGETS[@]}"; do
  # Validate contract name
  if [[ -z "${CONTRACT_IDS[$name]+exists}" ]]; then
    echo "ERROR: Unknown contract '$name'"
    echo "Valid contracts: ${!CONTRACT_IDS[*]}"
    exit 1
  fi

  if upgrade_contract "$name"; then
    SUCCEEDED=$((SUCCEEDED + 1))
  else
    FAILED=$((FAILED + 1))
  fi
done

echo ""
echo "=== Upgrade Summary ==="
echo "  Succeeded: $SUCCEEDED"
echo "  Failed:    $FAILED"
echo ""

if [[ $FAILED -gt 0 ]]; then
  exit 1
fi
