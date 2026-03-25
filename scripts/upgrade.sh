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
#   - .env file with ADMIN_SECRET and contract IDs (or set them as env vars)
#   - WASM files built via `stellar contract build`

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Source .env if present
if [[ -f "$PROJECT_ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$PROJECT_ROOT/.env"
  set +a
fi

WASM_DIR="${WASM_DIR:-$PROJECT_ROOT/target/wasm32v1-none/release}"
NETWORK="${NETWORK:-testnet}"

if [[ -z "${ADMIN_SECRET:-}" ]]; then
  >&2 echo "ERROR: ADMIN_SECRET not set (add to .env or export it)"
  exit 1
fi

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

# Map contract name → contract ID env var value
get_contract_id() {
  local name="$1"
  case "$name" in
    core_escrow)          echo "${CORE_ESCROW_ID:-}" ;;
    reputation_registry)  echo "${REPUTATION_REGISTRY_ID:-}" ;;
    governance_voting)    echo "${GOVERNANCE_VOTING_ID:-}" ;;
    project_registry)     echo "${PROJECT_REGISTRY_ID:-}" ;;
    bounty_registry)      echo "${BOUNTY_REGISTRY_ID:-}" ;;
    crowdfund_registry)   echo "${CROWDFUND_REGISTRY_ID:-}" ;;
    grant_hub)            echo "${GRANT_HUB_ID:-}" ;;
    hackathon_registry)   echo "${HACKATHON_REGISTRY_ID:-}" ;;
    *) return 1 ;;
  esac
}

install_wasm() {
  local wasm="$1"
  >&2 echo "  Installing WASM on-chain..."
  stellar contract install \
    --wasm "$wasm" \
    --source "$ADMIN_SECRET" \
    --network "$NETWORK"
}

upgrade_contract() {
  local name="$1"
  local contract_id
  contract_id="$(get_contract_id "$name")"
  local wasm="$WASM_DIR/${name}.wasm"

  if [[ -z "$contract_id" ]]; then
    >&2 echo "SKIP: $name — no contract ID set"
    return 1
  fi

  if [[ ! -f "$wasm" ]]; then
    >&2 echo "ERROR: WASM not found: $wasm"
    return 1
  fi

  >&2 echo ""
  >&2 echo "────────────────────────────────────────"
  >&2 echo "Upgrading: $name"
  >&2 echo "  Contract: $contract_id"
  >&2 echo "  WASM:     $wasm"
  >&2 echo "────────────────────────────────────────"

  # Install the new WASM and get the hash
  local wasm_hash
  wasm_hash=$(install_wasm "$wasm")
  >&2 echo "  WASM hash: $wasm_hash"

  # Call the contract's upgrade function
  >&2 echo "  Calling upgrade..."
  stellar contract invoke \
    --id "$contract_id" \
    --source "$ADMIN_SECRET" \
    --network "$NETWORK" \
    -- upgrade \
    --new_wasm_hash "$wasm_hash"

  >&2 echo "  Done: $name upgraded successfully"
}

# ── Main ─────────────────────────────────────────────────

TARGETS=("$@")

if [[ ${#TARGETS[@]} -eq 0 ]]; then
  >&2 echo "Usage: $0 <all | contract_name [contract_name ...]>"
  >&2 echo ""
  >&2 echo "Contracts: ${UPGRADE_ORDER[*]}"
  exit 1
fi

# Expand "all" to the full ordered list
if [[ "${TARGETS[0]}" == "all" ]]; then
  TARGETS=("${UPGRADE_ORDER[@]}")
fi

>&2 echo "=== Boundless Platform — Contract Upgrade ==="
>&2 echo "Network:   $NETWORK"
>&2 echo "Targets:   ${TARGETS[*]}"
>&2 echo ""

SUCCEEDED=0
FAILED=0

for name in "${TARGETS[@]}"; do
  # Validate contract name
  if ! get_contract_id "$name" > /dev/null 2>&1; then
    >&2 echo "ERROR: Unknown contract '$name'"
    >&2 echo "Valid contracts: ${UPGRADE_ORDER[*]}"
    exit 1
  fi

  if upgrade_contract "$name"; then
    SUCCEEDED=$((SUCCEEDED + 1))
  else
    FAILED=$((FAILED + 1))
  fi
done

>&2 echo ""
>&2 echo "=== Upgrade Summary ==="
>&2 echo "  Succeeded: $SUCCEEDED"
>&2 echo "  Failed:    $FAILED"
>&2 echo ""

if [[ $FAILED -gt 0 ]]; then
  exit 1
fi
