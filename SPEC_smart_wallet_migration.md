# Contract Spec: Smart-Wallet Migration

**Audience:** contracts team
**Goal:** prepare Boundless contracts for the smart-wallet (passkey + SAK) migration.
**Scope:** three changes — naming clarification on existing hackathon creation, pull-model prize claiming, and recovery-signer integration.

---

## Summary table

| Item | Status | Required action |
|---|---|---|
| **1. `create_and_fund_hackathon`** | Already implemented as `create_hackathon` | Optional rename + alias |
| **2. `claim_prize` (pull-model winnings)** | Not implemented | New function + refactor `finalize_hackathon` |
| **3. `add_passkey_signer` (recovery flow)** | Not a contract change — SAK primitives suffice | Integration spec only; no Boundless contract change |

---

## 1. `create_and_fund_hackathon`

### Current state

[`hackathon_registry::create_hackathon`](contracts/hackathon_registry/src/contract.rs#L60) already performs atomic create + fund + lock:

1. `creator.require_auth()` ✅
2. Creates registry entry
3. Calls `core_escrow::create_pool(creator, ModuleType::Hackathon, count, prize_pool, asset, judging_deadline, hackathon_registry_addr)` — which transfers `prize_pool` from creator to escrow at pool creation
4. Calls `core_escrow::lock_pool(pool_id)` to immediately lock
5. Emits `HackathonCreated`

This means **a single signed transaction by the creator publishes and funds the hackathon**.

### Recommended action

**Add an alias** `create_and_fund_hackathon` that delegates to `create_hackathon` with identical args. Reasons:
- The name makes the cost model obvious to integrators ("oh, this transfers my USDC immediately")
- We can deprecate `create_hackathon` later without breaking callers
- No semantic change, zero risk

```rust
pub fn create_and_fund_hackathon(
    env: Env,
    creator: Address,
    title: String,
    metadata_cid: String,
    prize_pool: i128,
    asset: Address,
    registration_deadline: u64,
    submission_deadline: u64,
    judging_deadline: u64,
    max_participants: u32,
    prize_tiers: Vec<u32>,
) -> Result<u64, HackathonError> {
    Self::create_hackathon(
        env,
        creator,
        title,
        metadata_cid,
        prize_pool,
        asset,
        registration_deadline,
        submission_deadline,
        judging_deadline,
        max_participants,
        prize_tiers,
    )
}
```

### Smart-account compatibility

`creator.require_auth()` resolves via Soroban's `Address` abstraction:

- **Classic G-address creator**: standard ed25519 signature check
- **Contract creator (SAK smart account)**: invokes `__check_auth` on the smart account contract, which runs passkey/policy verification

**No code changes needed for smart-account support.** The existing function works for both. Tests should be added to confirm:

```rust
#[test]
fn create_hackathon_works_with_smart_account_creator() {
    // Deploy a SAK smart account
    // Set it as `creator`
    // Mock __check_auth to pass
    // Verify create_hackathon succeeds and pool funds transferred
}
```

### Acceptance criteria
- [ ] `create_and_fund_hackathon` alias added (or document that `create_hackathon` is the function to call)
- [ ] Test added: smart-account creator publishes hackathon successfully
- [ ] Test added: classic G-address creator continues to work (regression)
- [ ] No state-shape changes to `Hackathon` struct

---

## 2. `claim_prize` (pull-model prize claiming)

### Why we're changing the model

Today [`finalize_hackathon`](contracts/hackathon_registry/src/contract.rs#L427) **pushes** prizes to winners by calling `core_escrow::release_partial` for each. Problems:

1. **Single huge transaction** — gas cost scales with `num_winners`; can fail near the limit
2. **Winners may not yet have a smart wallet** — push to a non-existent contract address or one without USDC trustline reverts
3. **Cannot finalize lazily** — anyone can `finalize_hackathon` so timing is unpredictable
4. **No winner consent** — winner can't choose tax/timing; prizes arrive when finalize fires

The pull model fixes all four. Finalize becomes cheap and idempotent; each winner claims when they're ready (with a deadline to prevent indefinite escrow lock).

### Refactor `finalize_hackathon`

Change finalize to **only mark winners**, not transfer funds.

```rust
pub fn finalize_hackathon(env: Env, hackathon_id: u64) -> Result<(), HackathonError> {
    let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

    if env.ledger().timestamp() <= hackathon.judging_deadline {
        return Err(HackathonError::JudgingNotOver);
    }
    if hackathon.status == HackathonStatus::Completed
        || hackathon.status == HackathonStatus::Cancelled
    {
        return Err(HackathonError::InvalidStatus);
    }

    let sub_count = hackathon.submission_count;
    if sub_count == 0 {
        return Err(HackathonError::NoSubmissions);
    }

    // Rank submissions (existing logic, unchanged)
    let leads = Self::compute_ranked_winners(&env, hackathon_id, sub_count)?;

    // Count prize tiers (existing logic, unchanged)
    let tier_count = Self::count_prize_tiers(&env, hackathon_id);
    let num_winners = leads.len().min(tier_count);

    // NEW: record winners + claimable amounts without releasing
    for rank in 0..num_winners {
        let winner = leads.get(rank).unwrap();
        let pct: u32 = env.storage().persistent()
            .get(&HackathonDataKey::PrizeTier(hackathon_id, rank))
            .unwrap();
        let amount = hackathon.prize_pool
            .checked_mul(pct as i128)
            .ok_or(HackathonError::Overflow)? / 10000;

        let winner_record = WinnerRecord {
            hackathon_id,
            rank,
            winner: winner.clone(),
            amount,
            claimed: false,
            claim_deadline: env.ledger().timestamp() + Self::claim_window_seconds(),
        };
        env.storage().persistent().set(
            &HackathonDataKey::Winner(hackathon_id, rank),
            &winner_record,
        );
        env.storage().persistent().set(
            &HackathonDataKey::WinnerByAddress(hackathon_id, winner.clone()),
            &rank,
        );

        // Reputation still recorded here (cheap, no token transfer)
        let is_win = rank == 0;
        let points = if rank == 0 { 100u32 } else if rank == 1 { 50u32 } else { 25u32 };
        Self::record_reputation(&env, &winner, points, is_win)?;
    }

    env.storage().persistent().set(
        &HackathonDataKey::WinnerCount(hackathon_id),
        &num_winners,
    );

    hackathon.status = HackathonStatus::Completed;
    env.storage().persistent()
        .set(&HackathonDataKey::Hackathon(hackathon_id), &hackathon);

    HackathonFinalized { hackathon_id, winner_count: num_winners }.publish(&env);

    Ok(())
}
```

Key changes:
- No `core_escrow::release_partial` calls inside finalize
- Records `WinnerRecord` per winner with `claim_deadline`
- Emits a new event `HackathonFinalized` (replaces `PrizesDistributed`, which now fires per-claim)

### New: `claim_prize`

```rust
/// Winner pulls their prize from escrow.
/// Auth: winner.require_auth() — works for G-addresses AND smart accounts.
/// Idempotent: subsequent calls error with AlreadyClaimed.
/// Time-bound: after claim_deadline, prize is forfeit (returned to creator or treasury).
pub fn claim_prize(
    env: Env,
    hackathon_id: u64,
    winner: Address,
) -> Result<i128, HackathonError> {
    winner.require_auth();

    let hackathon = Self::load_hackathon(&env, hackathon_id)?;
    if hackathon.status != HackathonStatus::Completed {
        return Err(HackathonError::HackathonNotFinalized);
    }

    let rank: u32 = env.storage().persistent()
        .get(&HackathonDataKey::WinnerByAddress(hackathon_id, winner.clone()))
        .ok_or(HackathonError::NotAWinner)?;

    let mut record: WinnerRecord = env.storage().persistent()
        .get(&HackathonDataKey::Winner(hackathon_id, rank))
        .ok_or(HackathonError::NotAWinner)?;

    if record.claimed {
        return Err(HackathonError::AlreadyClaimed);
    }
    if env.ledger().timestamp() > record.claim_deadline {
        return Err(HackathonError::ClaimWindowExpired);
    }

    // Release from escrow
    let escrow_addr = Self::get_escrow_addr(&env)?;
    let release_args: Vec<Val> = Vec::from_array(
        &env,
        [
            hackathon.pool_id.clone().into_val(&env),
            winner.clone().into_val(&env),
            record.amount.into_val(&env),
        ],
    );
    env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_partial"), release_args);

    record.claimed = true;
    record.claimed_at = Some(env.ledger().timestamp());
    env.storage().persistent().set(
        &HackathonDataKey::Winner(hackathon_id, rank),
        &record,
    );

    PrizeClaimed {
        hackathon_id,
        winner,
        rank,
        amount: record.amount,
    }.publish(&env);

    Ok(record.amount)
}
```

### New: `reclaim_unclaimed_prizes`

After `claim_deadline` passes, allow the creator (or admin) to sweep unclaimed prizes back to the creator or platform treasury. Without this, dead winners lock funds forever.

```rust
/// After claim window expires, return unclaimed prize amounts to the creator.
/// Auth: creator.require_auth() (or admin override).
/// Iterates all winner records; for each with claimed=false and now > claim_deadline,
/// releases the amount back to creator via core_escrow.
pub fn reclaim_unclaimed_prizes(
    env: Env,
    hackathon_id: u64,
) -> Result<i128, HackathonError> {
    let hackathon = Self::load_hackathon(&env, hackathon_id)?;
    hackathon.creator.require_auth();

    if hackathon.status != HackathonStatus::Completed {
        return Err(HackathonError::HackathonNotFinalized);
    }

    let winner_count: u32 = env.storage().persistent()
        .get(&HackathonDataKey::WinnerCount(hackathon_id))
        .unwrap_or(0);

    let now = env.ledger().timestamp();
    let escrow_addr = Self::get_escrow_addr(&env)?;
    let mut total_reclaimed: i128 = 0;

    for rank in 0..winner_count {
        let mut record: WinnerRecord = env.storage().persistent()
            .get(&HackathonDataKey::Winner(hackathon_id, rank))
            .unwrap();

        if record.claimed || now <= record.claim_deadline {
            continue;
        }

        // Return amount to creator
        let release_args: Vec<Val> = Vec::from_array(
            &env,
            [
                hackathon.pool_id.clone().into_val(&env),
                hackathon.creator.clone().into_val(&env),
                record.amount.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_partial"), release_args);

        record.claimed = true;  // mark to prevent re-reclaim
        record.claimed_at = Some(now);
        env.storage().persistent().set(
            &HackathonDataKey::Winner(hackathon_id, rank),
            &record,
        );
        total_reclaimed = total_reclaimed.checked_add(record.amount)
            .ok_or(HackathonError::Overflow)?;
    }

    if total_reclaimed > 0 {
        UnclaimedPrizesReclaimed {
            hackathon_id,
            total_amount: total_reclaimed,
        }.publish(&env);
    }

    Ok(total_reclaimed)
}
```

### New storage shapes

```rust
// storage/mod.rs additions

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WinnerRecord {
    pub hackathon_id: u64,
    pub rank: u32,
    pub winner: Address,
    pub amount: i128,
    pub claimed: bool,
    pub claimed_at: Option<u64>,
    pub claim_deadline: u64,
}

// Extend HackathonDataKey
pub enum HackathonDataKey {
    // ... existing variants
    Winner(u64, u32),                  // (hackathon_id, rank) -> WinnerRecord
    WinnerByAddress(u64, Address),     // (hackathon_id, address) -> u32 (rank lookup)
    WinnerCount(u64),                  // (hackathon_id) -> u32
}
```

### New events

```rust
// events/mod.rs additions

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HackathonFinalized {
    #[topic]
    pub hackathon_id: u64,
    pub winner_count: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrizeClaimed {
    #[topic]
    pub hackathon_id: u64,
    #[topic]
    pub winner: Address,
    pub rank: u32,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnclaimedPrizesReclaimed {
    #[topic]
    pub hackathon_id: u64,
    pub total_amount: i128,
}
```

### New error variants

```rust
// error.rs additions

HackathonNotFinalized = 1030,
NotAWinner = 1031,
AlreadyClaimed = 1032,
ClaimWindowExpired = 1033,
```

### Configuration

- `claim_window_seconds()` — recommend **90 days** (`90 * 86400 = 7,776,000`). Long enough that even occasional users can claim; short enough that funds don't sit indefinitely.
- Make it admin-configurable via `set_claim_window(seconds: u64)` so it can be tuned without redeploy.

### Migration impact

Old hackathons finalized before this upgrade will have `WinnerRecord` rows absent. Two options:

1. **Hard cutover** — finalize remaining hackathons via the old path before deploying, then upgrade. Simplest.
2. **Dual-path** — keep the old release-on-finalize logic for hackathons created pre-upgrade; new logic for post-upgrade. Adds complexity; only do this if needed.

Recommend option 1. Coordinate with backend team for the finalize sweep before upgrade.

### Acceptance criteria
- [ ] `finalize_hackathon` no longer transfers tokens; only marks winners
- [ ] `claim_prize` works for both G-address and smart-account winners
- [ ] `claim_prize` is idempotent (second call → `AlreadyClaimed`)
- [ ] `claim_prize` respects `claim_deadline` (post-deadline → `ClaimWindowExpired`)
- [ ] `reclaim_unclaimed_prizes` sweeps expired prizes back to creator
- [ ] Reentrancy: `claim_prize` updates `record.claimed = true` BEFORE `release_partial` call (or use a guard) — review with care
- [ ] Events emitted match the new schema
- [ ] Pre-upgrade hackathons migrated or dual-path is documented

### Security review checklist
- [ ] No way to claim more than once per (hackathon, winner)
- [ ] No way to claim a prize for someone else (winner.require_auth() enforced)
- [ ] No way to claim after the deadline
- [ ] No way to claim if hackathon was cancelled (status check)
- [ ] No integer overflow in amount calculations
- [ ] No way to manipulate prize amounts after finalize
- [ ] `reclaim_unclaimed_prizes` cannot double-reclaim (sets `claimed = true`)
- [ ] Test: malicious caller cannot drain pool via repeated calls

---

## 3. `add_passkey_signer` (recovery flow)

### This is NOT a Boundless contract change

The user's smart wallet is a SAK-deployed smart account contract (OpenZeppelin-derived). Adding signers to it is already supported by SAK's contract code via:

- `add_signer(context_rule_id, signer)`
- `batch_add_signer(context_rule_id, signers)`
- `remove_signer(context_rule_id, signer)`

We do NOT need to write or modify any contract for this. Instead, we need to **configure each user's smart account at creation** so that recovery works.

### What this section is: integration spec

This describes how the backend + frontend orchestrate the existing SAK primitives to deliver email-magic-link recovery.

### One-time setup at wallet creation

When a user creates their smart wallet (`kit.createWallet`), the FE should also register a recovery signer:

```typescript
// On wallet creation
const { contractId, credentialId } = await kit.createWallet(appName, email);

// Backend derives recovery key (KMS envelope-encrypted per user)
const { recoveryAddress } = await api.post('/auth/recovery/init', {
  contractId,
  email,
});

// Add recovery G-address as a delegated signer with a recovery-only context rule
const recoveryContextRule = await kit.rules.add(
  createCallContractContext(contractId),  // only this contract
  'Recovery',
  [createDelegatedSigner(recoveryAddress, ED25519_VERIFIER_ADDRESS)],
  // Policy: allowed function = add_signer ONLY
  // No spending limit because no funds move
  [/* RECOVERY_POLICY_ADDRESS that allows only `add_signer` calls */],
);
```

### Recovery key derivation (backend)

```typescript
// boundless-nestjs/src/modules/auth/recovery.service.ts

class RecoveryService {
  async getRecoveryKey(userId: string, email: string): Promise<Keypair> {
    // Fetch user's encrypted recovery key from DB
    const record = await this.prisma.recoveryKey.findUnique({
      where: { userId },
    });

    if (!record) {
      // First-time: generate, encrypt via KMS, store
      const keypair = Keypair.random();
      const ciphertext = await this.kms.encrypt({
        plaintext: keypair.secret(),
        context: { userId, email },  // encryption context binds to identity
      });
      await this.prisma.recoveryKey.create({
        data: { userId, ciphertext, publicKey: keypair.publicKey() },
      });
      return keypair;
    }

    // Existing: decrypt via KMS
    const secret = await this.kms.decrypt({
      ciphertext: record.ciphertext,
      context: { userId, email },
    });
    return Keypair.fromSecret(secret);
  }
}
```

KMS is AWS KMS or GCP KMS with envelope encryption. Encryption context binds the key to `(userId, email)` so a stolen ciphertext + leaked KMS access still can't decrypt without knowing the user.

### Recovery flow

```
1. User loses device. Goes to https://boundless.fi/recover-wallet
2. Enters email → backend sends magic link
3. User clicks link → backend verifies, generates short-lived recovery JWT
4. FE: prompts user to create a NEW passkey on the new device
5. FE → POST /auth/recovery/complete { recoveryToken, newCredentialId, newPublicKey }
6. Backend:
   a. Verifies recoveryToken (one-time, short-lived)
   b. Fetches user's smart account contractId
   c. Decrypts recovery key via KMS
   d. Builds tx: add_signer(contextRuleId, newPasskeySigner)
   e. Signs with recovery key
   f. Submits via relayer
7. FE: new passkey is now a signer on the smart account. User can connect normally.
```

### Backend-side primitives needed

```typescript
// boundless-nestjs/src/modules/auth/recovery.controller.ts

@Controller('auth/recovery')
class RecoveryController {
  // Step 1: user requests recovery
  @Post('request')
  async requestRecovery(@Body() { email }: { email: string }) {
    // Send magic link to email; rate-limit per email
  }

  // Step 2: user clicked link, FE created new passkey
  @Post('complete')
  async completeRecovery(
    @Body() { recoveryToken, newCredentialId, newPublicKey }: CompleteRecoveryDto,
  ) {
    const { userId, email } = await this.verifyRecoveryToken(recoveryToken);
    const user = await this.users.findById(userId);
    if (!user.smartWallet) throw new Error('No smart wallet to recover');

    const recoveryKeypair = await this.recovery.getRecoveryKey(userId, email);
    
    // Build add_signer tx targeting the user's smart account
    const tx = await this.stellarAdapter.buildContractCall(
      user.smartWallet.contractId,
      'add_signer',
      [RECOVERY_CONTEXT_RULE_ID, newPasskeySignerScVal(newCredentialId, newPublicKey)],
    );
    
    // Sign with recovery keypair, submit via relayer
    const signedXdr = signWithKeypair(tx, recoveryKeypair);
    const txResult = await this.relayer.submit(signedXdr);
    
    // Update DB
    await this.prisma.passkeyCredential.create({
      data: {
        userId,
        credentialId: newCredentialId,
        publicKey: newPublicKey,
        addedVia: 'RECOVERY',
      },
    });
    
    return { success: true, txHash: txResult.hash };
  }
}
```

### Security properties

- **Recovery key never leaves KMS unencrypted at rest.** Decrypted only during the brief signing operation, in process memory.
- **Encryption context binds key to email.** If user changes email, key must be re-encrypted with new context.
- **Recovery context rule is scoped.** It can ONLY call `add_signer` — not `transfer`, not `remove_signer`, not anything else. Even a KMS compromise can't drain funds; attacker can only add a passkey they control, but the user can then `remove_signer` it.
- **One-time recovery tokens** (short TTL, single-use) gate the API.
- **Rate limit recovery requests** per email to prevent spam.

### What's needed in the contracts repo

**Nothing new.** Just verify the existing SAK smart-account contract supports the operations we need:

- [ ] `add_signer(context_rule_id, signer)` — already in SAK
- [ ] Context rule with function allow-list — already in SAK via policy contracts
- [ ] A "recovery policy" contract that whitelists only the `add_signer` function

The last item may need to be deployed if SAK's default policy contracts don't include a function-allowlist policy. Contracts team should check SAK's existing policies:

- ThresholdPolicy
- SpendingLimitPolicy
- WeightedThresholdPolicy

If none restrict by function name, deploy a new `FunctionAllowlistPolicy` contract that takes `Vec<Symbol>` of allowed function names and rejects others.

### Acceptance criteria
- [ ] Documented recovery context-rule shape in the wallet-creation runbook
- [ ] Confirmed SAK has (or we deploy) a policy that restricts by function name
- [ ] Backend has `recovery.service.ts` with KMS-backed key derivation
- [ ] Backend has rate-limited `/auth/recovery/request` + `/auth/recovery/complete` endpoints
- [ ] Manual E2E test: lose passkey on device A, recover via email on device B, can sign tx with new passkey
- [ ] Failure test: recovery token reuse rejected
- [ ] Failure test: KMS decrypt with wrong encryption context fails

---

## Coordination notes

- **Contracts repo** ships: alias for `create_and_fund_hackathon`, refactored `finalize_hackathon`, new `claim_prize` + `reclaim_unclaimed_prizes`, new storage shapes, new events, new errors. (Optional: `FunctionAllowlistPolicy` if not in SAK.)
- **Backend repo** ships: `RecoveryService` + KMS integration, `/auth/recovery/*` endpoints, indexer handlers for `HackathonFinalized` and `PrizeClaimed` events, removes push-distribution logic.
- **Frontend repo** ships: `useClaimPrize` hook, "claim your prize" UI on hackathon page for winners, recovery UI at `/recover-wallet`, wallet-creation flow extended to add recovery signer.
- **All three repos** version-pin to a `bindings@x.y.z` package after the contract changes are deployed.

### Suggested order

1. Contracts team: implement and deploy to testnet (1-2 weeks)
2. Generate new bindings; FE + BE pin to new version
3. BE: indexer handlers + recovery service (1 week, parallelizable with contracts)
4. FE: claim UI + recovery UI (1 week, after bindings available)
5. E2E test on testnet (2-3 days)
6. Mainnet contract deployment + cutover (1 day, with rollback plan)

Total: ~4 weeks elapsed if parallelized.
