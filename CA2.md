# Boundless Platform — Unified Soroban Smart Contract Architecture

**Version 2.0 | All Modules: Bounties · Crowdfunding · Grants · Hackathons**
*No TrustlessWork dependency — all escrow logic is native Soroban*

---

## Platform Module Map

| Module | Sub-Types | Core Mechanism |
|--------|-----------|----------------|
| **Bounties** | Fixed/FCFS, Contest, Split, Application | Speed / Quality / Selection |
| **Crowdfunding** | Milestone-based | Pledge → Vote → Escrow → Staged Release |
| **Grants** | Milestone, Retrospective, Quadratic (QF) | Admin-directed / Vote-weighted payout |
| **Hackathons** | Traditional (ranked), Sponsored Tracks | Judge voting → Multi-prize pool |

---

## System Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        BOUNDLESS UNIFIED CONTRACT SYSTEM                    │
│                                                                             │
│  ┌──────────────┐  ┌─────────────────┐  ┌──────────┐  ┌──────────────────┐│
│  │BountyRegistry│  │CrowdfundRegistry│  │GrantHub  │  │HackathonRegistry ││
│  └──────┬───────┘  └────────┬────────┘  └────┬─────┘  └────────┬─────────┘│
│         │                   │                │                  │           │
│         └──────────┬────────┘                └─────────┬────────┘           │
│                    │                                    │                   │
│                    ▼                                    ▼                   │
│             ┌─────────────┐                   ┌──────────────────┐         │
│             │ CoreEscrow  │◄──────────────────►│ GovernanceVoting │         │
│             └──────┬──────┘                   └──────────────────┘         │
│                    │                                    ▲                   │
│                    ▼                                    │                   │
│             ┌──────────────┐       ┌────────────────────┴──┐               │
│             │PaymentRouter │       │  ReputationRegistry   │               │
│             └──────────────┘       └───────────────────────┘               │
│                                                            ▲               │
│                    ┌───────────────────────────────────────┘               │
│                    │                                                        │
│             ┌──────┴──────┐    ┌─────────────────┐                        │
│             │SparkCredits │    │ ProjectRegistry  │                        │
│             └─────────────┘    └─────────────────┘                        │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Total Contracts: 9**

- 4 Module Registries (Bounty, Crowdfund, Grant, Hackathon)
- 5 Shared Infrastructure (CoreEscrow, PaymentRouter, GovernanceVoting, ReputationRegistry, SparkCredits, ProjectRegistry)

> **Key principle:** The 5 shared contracts are deployed once and reused by all 4 module registries. This avoids code duplication and means reputation, escrow, and voting logic is consistent platform-wide.

---

## Shared Contract 1: `CoreEscrow`

**Replaces TrustlessWork entirely.** The single source of truth for all fund custody across every module. Funds are untouchable by creators until released by the authorized caller (a module registry).

### Why One Escrow Contract?

- Single audit surface for all fund management
- Unified insurance fund fed by all platform fees
- Consistent refund behavior across modules
- One contract address for users to trust/verify

### Storage Schema

```rust
// Master record for any escrowed pool
struct EscrowPool {
    pool_id: BytesN<32>,      // unique across all modules (module_type + module_id)
    module: ModuleType,       // Bounty | Crowdfund | Grant | Hackathon
    authorized_caller: Address, // which registry contract controls this pool
    owner: Address,           // project/creator who deposited
    total_deposited: i128,
    total_released: i128,
    total_refunded: i128,
    asset: Asset,             // XLM, USDC, EURC, or project token
    locked: bool,             // true = funds committed, owner cannot withdraw
    created_at: u64,
    expires_at: u64,          // deadline after which unclaimed funds refundable
}

// Granular release schedule (for milestone-based pools)
struct ReleaseSlot {
    pool_id: BytesN<32>,
    slot_index: u32,
    amount: i128,
    recipient: Address,
    released: bool,
    released_at: Option<u64>,
}

// Insurance fund (separate sub-account)
struct InsuranceFund {
    balance: i128,
    total_contributions: i128,
    total_paid_out: i128,
}

enum ModuleType {
    Bounty,
    Crowdfund,
    Grant,
    Hackathon,
}
```

### Core Functions

```rust
// ── DEPOSIT ────────────────────────────────────────────────────────────────
fn create_pool(
    env: Env,
    owner: Address,
    module: ModuleType,
    module_id: u64,
    total_amount: i128,
    asset: Asset,
    expires_at: u64,
) -> BytesN<32>
// Transfers funds from owner into contract
// Sets locked = false until explicitly locked
// Returns pool_id

fn lock_pool(env: Env, pool_id: BytesN<32>)
// Auth: authorized_caller (module registry) only
// After lock: owner cannot withdraw. Funds are committed.
// Called when: campaign funded, bounty claimed, hackathon started

fn define_release_slots(
    env: Env,
    pool_id: BytesN<32>,
    slots: Vec<(Address, i128)>,   // (recipient, amount)
)
// Auth: authorized_caller only
// Defines who gets how much (used for milestone, QF payout, etc.)
// Sum of slot amounts must == total_deposited

// ── RELEASE ────────────────────────────────────────────────────────────────
fn release_slot(
    env: Env,
    pool_id: BytesN<32>,
    slot_index: u32,
)
// Auth: authorized_caller only
// Releases one slot to its recipient
// Used per milestone approval, per bounty approval, per hackathon winner

fn release_all(env: Env, pool_id: BytesN<32>)
// Auth: authorized_caller only
// Releases all un-released slots at once (e.g., retrospective grant)

fn release_partial(
    env: Env,
    pool_id: BytesN<32>,
    recipient: Address,
    amount: i128,
)
// Auth: authorized_caller only
// For QF or dynamic payout amounts not defined upfront

// ── REFUND ─────────────────────────────────────────────────────────────────
fn refund_all(env: Env, pool_id: BytesN<32>)
// Auth: authorized_caller only
// Returns all unreleased funds to original depositors
// For crowdfunding: must iterate backer list (see note below)
// For bounty/grant/hackathon: single owner refund

fn refund_backers(
    env: Env,
    pool_id: BytesN<32>,
    backers: Vec<(Address, i128)>,  // (address, pledge_amount)
)
// Used by CrowdfundRegistry when campaign fails
// Iterates backer list and sends each their pledged amount back

fn refund_remaining(env: Env, pool_id: BytesN<32>)
// Returns only the unreleased portion back to owner
// Used when partial milestone payouts done and campaign abandoned

// ── INSURANCE ──────────────────────────────────────────────────────────────
fn contribute_insurance(env: Env, amount: i128, asset: Asset)
// Called automatically on each payment: 5% of platform fee goes here
// Auth: Any module registry

fn claim_insurance(
    env: Env,
    claimant: Address,
    pool_id: BytesN<32>,
    evidence_cid: String,
    admin: Address,
)
// Auth: Admin multisig required
// Max payout: remaining slot value (capped at 10,000 XLM equivalent)

// ── QUERIES ────────────────────────────────────────────────────────────────
fn get_pool(env: Env, pool_id: BytesN<32>) -> EscrowPool
fn get_slot(env: Env, pool_id: BytesN<32>, index: u32) -> ReleaseSlot
fn get_unreleased(env: Env, pool_id: BytesN<32>) -> i128
fn get_insurance_balance(env: Env) -> i128
fn is_locked(env: Env, pool_id: BytesN<32>) -> bool
```

### Escrow Pool ID Generation

```rust
// Pool IDs are deterministic: no collision across modules
pool_id = sha256(module_type_byte ++ module_id_u64_bytes)
// e.g., Bounty #42 → sha256(0x01 ++ 42u64)
//       Campaign #7 → sha256(0x02 ++ 7u64)
```

---

## Shared Contract 2: `PaymentRouter`

**Handles fee deduction and multi-asset normalization** before funds hit CoreEscrow or leave to recipients.

### Platform Fee Logic

| Module | Platform Fee | Insurance Contribution |
|--------|-------------|----------------------|
| Bounty | 5% of payout | 5% of platform fee |
| Crowdfunding | 5% of funds raised | 5% of platform fee |
| Grant | 3% of grant amount | 5% of platform fee |
| Hackathon | 4% of prize pool | 5% of platform fee |

```rust
fn route_deposit(
    env: Env,
    payer: Address,
    pool_id: BytesN<32>,
    gross_amount: i128,
    asset: Asset,
    module: ModuleType,
) -> i128  // returns net amount after fee
// Splits: platform_fee → treasury, insurance_cut → CoreEscrow insurance
// Net amount → CoreEscrow::create_pool or top-up

fn route_payout(
    env: Env,
    pool_id: BytesN<32>,
    recipient: Address,
    gross_amount: i128,
) -> i128  // returns amount actually sent to recipient
// No fee on payouts (fee taken at deposit)
// Triggers CoreEscrow::release_slot internally

fn get_fee_rate(env: Env, module: ModuleType) -> u32  // basis points
fn treasury_balance(env: Env) -> i128
fn set_treasury(env: Env, new_treasury: Address)  // Admin only
```

---

## Shared Contract 3: `GovernanceVoting`

**Reusable voting engine** for crowdfunding validation, retrospective grant distribution, QF rounds, and hackathon judging.

### This one contract handles 4 different voting contexts

| Context | Module | Outcome |
|---------|--------|---------|
| Campaign validation | Crowdfunding | Threshold reached → moves to Campaigning |
| Retrospective grant | Grants | Votes determine payout shares |
| QF round | Grants | Donor signals amplified by square root math |
| Hackathon judging | Hackathons | Judge scores determine prize allocation |

### Storage Schema

```rust
struct VotingSession {
    session_id: BytesN<32>,
    context: VoteContext,
    module_id: u64,           // which campaign/grant/hackathon
    created_at: u64,
    start_at: u64,
    end_at: u64,
    status: VoteStatus,
    threshold: Option<u32>,   // for validation polls (e.g., 100 votes)
    threshold_reached: bool,
    options: Vec<VoteOption>,
    total_votes: u32,
    quorum: Option<u32>,      // min votes for result to be valid
    weight_by_reputation: bool,
}

struct VoteOption {
    id: u32,
    label: String,           // "Support", "Abstain", "Flag" / project name / submission id
    votes: u32,
    weighted_votes: u64,
}

struct VoteRecord {
    session_id: BytesN<32>,
    voter: Address,
    option_id: u32,
    weight: u32,             // 1 for unweighted, reputation_score for weighted
    voted_at: u64,
}

enum VoteContext {
    CampaignValidation,   // Crowdfunding: support threshold
    RetrospectiveGrant,   // Grant: share allocation
    QFRound,              // Grant: donor signal amplification
    HackathonJudging,     // Hackathon: judge scoring
}

enum VoteStatus {
    Pending,
    Active,
    Concluded,
    Cancelled,
}
```

### Functions

```rust
fn create_session(
    env: Env,
    context: VoteContext,
    module_id: u64,
    options: Vec<String>,
    start_at: u64,
    end_at: u64,
    threshold: Option<u32>,
    weight_by_reputation: bool,
) -> BytesN<32>
// Auth: Respective module registry

fn cast_vote(
    env: Env,
    voter: Address,
    session_id: BytesN<32>,
    option_id: u32,
)
// One vote per user per session (enforced via VoteRecord key)
// If weight_by_reputation: fetches weight from ReputationRegistry
// Updates VoteOption counts

fn conclude_session(env: Env, session_id: BytesN<32>)
// Can be called by anyone after end_at
// Evaluates threshold, computes final results
// Emits SessionConcluded event

fn get_result(env: Env, session_id: BytesN<32>) -> Vec<(String, u32, u64)>
// Returns (option_label, raw_votes, weighted_votes)

fn has_voted(env: Env, session_id: BytesN<32>, voter: Address) -> bool
fn get_session(env: Env, session_id: BytesN<32>) -> VotingSession
fn threshold_reached(env: Env, session_id: BytesN<32>) -> bool

// ── QF-SPECIFIC ────────────────────────────────────────────────────────────
fn record_qf_donation(
    env: Env,
    session_id: BytesN<32>,
    donor: Address,
    project_option_id: u32,
    amount: i128,
)
// QF: tracks donation amounts per project (not just vote counts)
// Stores (donor, project, amount) for coefficient calculation

fn compute_qf_distribution(
    env: Env,
    session_id: BytesN<32>,
    matching_pool: i128,
) -> Vec<(u32, i128)>
// Returns (option_id, matching_amount) per project
// Formula: matching_i = (√Σdonations_i)² / Σ(√Σdonations_j)² * matching_pool
// Pure on-chain calculation using fixed-point math
```

---

## Shared Contract 4: `ReputationRegistry`

*(Expanded from bounty-only plan to serve all modules)*

### New additions for non-bounty modules

```rust
// Added to ContributorProfile:
struct ContributorProfile {
    // ... (all original bounty fields) ...
    hackathons_entered: u32,
    hackathons_won: u32,
    campaigns_backed: u32,
    grants_received: u32,
    total_earned_all_modules: i128,
    total_donated_qf: i128,    // participation in QF rounds
}
```

### New functions

```rust
fn record_hackathon_win(
    env: Env,
    contributor: Address,
    prize_amount: i128,
    track: Symbol,
)
// Auth: HackathonRegistry only

fn record_campaign_backed(env: Env, backer: Address)
// Auth: CrowdfundRegistry only

fn record_grant_received(
    env: Env,
    recipient: Address,
    amount: i128,
)
// Auth: GrantHub only
```

---

## Shared Contract 5: `SparkCredits`

*(Unchanged from bounty plan — only applies to Bounty module)*

---

## Shared Contract 6: `ProjectRegistry`

*(Expanded — now covers projects posting hackathons and grants too)*

```rust
// Added to Project struct:
struct Project {
    // ... (all original fields) ...
    hackathons_hosted: u32,
    grants_distributed: i128,
    campaigns_launched: u32,
    total_platform_spend: i128,
}
```

---

---

# MODULE CONTRACTS

---

## Module Contract 1: `BountyRegistry`

*(Carries forward the full bounty architecture from v1.0 with one change: EscrowVault is replaced by CoreEscrow)*

### Bounty Sub-Types and Their CoreEscrow Patterns

| Sub-Type | Pool Lock Trigger | Release Trigger | Refund Trigger |
|----------|------------------|-----------------|----------------|
| Fixed/FCFS (Single Claim) | `claim_bounty()` | `approve_submission()` | Stale timeout / `cancel_bounty()` |
| Application + Selection | Admin `select_applicant()` | `approve_submission()` | Creator cancels / not selected |
| Contest (Competition) | Bounty created (pre-lock all prizes) | `approve_submission()` for each winner | Non-winners auto-refund |
| Split Bounty | Bounty created | `approve_milestone()` per contributor | Abandoned slot refund |

### Key changes from v1.0

```rust
// OLD (removed):
// EscrowVault::deposit()
// EscrowVault::release()

// NEW:
fn create_bounty(...) {
    let pool_id = CoreEscrow::create_pool(
        creator, ModuleType::Bounty, bounty_id,
        budget, asset, deadline + grace_period
    );
    PaymentRouter::route_deposit(creator, pool_id, budget, asset, ModuleType::Bounty);
    CoreEscrow::lock_pool(pool_id);  // immediately locked for FCFS
    // For Application model: lock after selection
}

fn approve_submission(creator, bounty_id, contributor, rating) {
    CoreEscrow::release_partial(pool_id, contributor, payout_amount);
    ReputationRegistry::record_completion(...);
    SparkCredits::award(...);
    // 5% fee already deducted at deposit via PaymentRouter
}
```

---

## Module Contract 2: `CrowdfundRegistry`

**Manages the complete crowdfunding lifecycle** — draft through completion — without TrustlessWork.

### Campaign State Machine

```
Draft → Submitted → Validated → Campaigning → Funded → Executing → Completed
                                                    ↘
                                                   Failed
```

### Storage Schema

```rust
struct Campaign {
    id: u64,
    creator: Address,
    project_id: u64,
    metadata_cid: String,    // IPFS: title, desc, team, media
    category: Symbol,
    status: CampaignStatus,
    funding_goal: i128,
    asset: Asset,
    raised: i128,
    backer_count: u32,
    start_at: u64,
    end_at: u64,
    created_at: u64,
    milestones: Vec<CampaignMilestone>,
    pool_id: Option<BytesN<32>>,       // CoreEscrow pool, set when funding opens
    vote_session_id: Option<BytesN<32>>, // GovernanceVoting session
    admin_reviewer: Address,
    rejection_cid: Option<String>,
    platform_fee_bps: u32,   // locked in at campaign creation
}

struct CampaignMilestone {
    index: u32,
    title: String,
    description_cid: String,  // full details on IPFS
    budget_pct: u8,           // percentage of total (all must sum to 100)
    amount: i128,             // calculated: budget_pct * funding_goal / 100
    deadline: u64,
    status: MilestoneStatus,
    submission_cid: Option<String>,
    submitted_at: Option<u64>,
    approved_at: Option<u64>,
    revision_count: u8,
}

// Separate backer tracking (needed for refunds)
// Key: (campaign_id, backer_address) → pledge_amount
// Key: campaign_id → Vec<Address>  (backer list)

enum CampaignStatus {
    Draft,
    Submitted,       // awaiting admin review
    Validated,       // approved, in voting period
    Campaigning,     // voting passed, accepting pledges
    Funded,          // goal reached, escrow locked
    Executing,       // milestones in progress
    Completed,       // all milestones approved
    Failed,          // deadline passed, goal not met
    Cancelled,       // admin or creator cancelled
}

enum MilestoneStatus {
    Locked,          // not yet active
    Active,          // current milestone, creator should be working
    Submitted,       // creator submitted, awaiting review
    RevisionNeeded,
    Approved,
    Disputed,
    Failed,
}
```

### Functions

```rust
// ── CAMPAIGN CREATION ──────────────────────────────────────────────────────
fn create_campaign(
    env: Env,
    creator: Address,
    project_id: u64,
    metadata_cid: String,
    category: Symbol,
    funding_goal: i128,
    asset: Asset,
    duration_days: u32,          // 15-60
    milestones: Vec<MilestoneInput>,
) -> u64
// Validates: milestone % sum == 100, count in [2,10], spacing >= 2 weeks
// Status → Draft

fn submit_for_review(env: Env, creator: Address, campaign_id: u64)
// Auth: creator
// Status → Submitted

// ── ADMIN REVIEW ───────────────────────────────────────────────────────────
fn approve_campaign(
    env: Env,
    admin: Address,
    campaign_id: u64,
    notes_cid: Option<String>,
)
// Status → Validated
// Creates GovernanceVoting session (CampaignValidation context)
// Vote window: 7-14 days, threshold: 100 support votes

fn reject_campaign(
    env: Env,
    admin: Address,
    campaign_id: u64,
    rejection_cid: String,   // detailed feedback on IPFS
)
// Status stays Draft (creator can revise and resubmit)

fn request_revisions(
    env: Env,
    admin: Address,
    campaign_id: u64,
    feedback_cid: String,
)

// ── COMMUNITY VALIDATION ───────────────────────────────────────────────────
fn vote_campaign(
    env: Env,
    voter: Address,
    campaign_id: u64,
    option: VoteOption,  // Support | Abstain | FlagConcern
)
// Delegates to GovernanceVoting::cast_vote()
// If FlagConcern: auto-notifies admin

fn check_vote_threshold(env: Env, campaign_id: u64) -> bool
// Reads GovernanceVoting threshold_reached
// If true: automatically transitions to Campaigning
// Emits CampaignLaunched event

// ── FUNDING PHASE ──────────────────────────────────────────────────────────
fn pledge(
    env: Env,
    backer: Address,
    campaign_id: u64,
    amount: i128,
    asset: Asset,
)
// Validates: campaign status == Campaigning, deadline not passed
// Creates CoreEscrow pool if first pledge (pool not pre-funded)
// Deposits into pool (NOT locked yet — refundable if goal not met)
// Records backer pledge: BackerPledge(campaign_id, backer) = amount
// Updates campaign.raised
// Checks if goal reached → if yes: calls _finalize_funding()

fn _finalize_funding(env: Env, campaign_id: u64)
// Internal: called when raised >= funding_goal
// CoreEscrow::lock_pool(pool_id)  ← funds now irrevocable to backers
// Status → Funded
// Defines release slots on CoreEscrow: one slot per milestone
// Activates Milestone 0
// PaymentRouter::route_deposit handles fee

fn check_deadline(env: Env, campaign_id: u64)
// Permissionless: anyone can call after end_at
// If status == Campaigning and raised < funding_goal:
//   → CoreEscrow::refund_backers(pool_id, backer_list)
//   → Status → Failed

// ── MILESTONE EXECUTION ────────────────────────────────────────────────────
fn submit_milestone(
    env: Env,
    creator: Address,
    campaign_id: u64,
    milestone_index: u32,
    submission_cid: String,   // IPFS: report, links, evidence
)
// Validates: milestone status == Active, creator == campaign.creator
// Status → Submitted

fn approve_milestone(
    env: Env,
    admin: Address,
    campaign_id: u64,
    milestone_index: u32,
    notes_cid: Option<String>,
)
// Auth: admin (designated reviewer)
// CoreEscrow::release_slot(pool_id, milestone_index)
//   → Transfers milestone.amount to creator wallet
// If last milestone: status → Completed
// Else: activate next milestone
// ReputationRegistry::record_completion(creator, ...)

fn request_milestone_revision(
    env: Env,
    admin: Address,
    campaign_id: u64,
    milestone_index: u32,
    feedback_cid: String,
)
// milestone status → RevisionNeeded
// Creator resubmits via submit_milestone()

fn reject_milestone(
    env: Env,
    admin: Address,
    campaign_id: u64,
    milestone_index: u32,
    reason_cid: String,
)
// milestone status → Failed
// Triggers dispute window (backers can signal)

fn dispute_milestone(
    env: Env,
    reporter: Address,   // backer or creator
    campaign_id: u64,
    milestone_index: u32,
    evidence_cid: String,
)
// Sets milestone status → Disputed
// Notifies admin for arbitration
// Possible outcomes (admin resolves):
//   • approve_milestone() — creator gets paid
//   • force_partial_refund() — partial refund to backers
//   • terminate_campaign() — remaining escrow refunded

fn terminate_campaign(
    env: Env,
    admin: Address,
    campaign_id: u64,
    reason_cid: String,
)
// Refunds remaining unreleased escrow to backers proportionally
// Status → Cancelled

// ── STALE MILESTONE ENFORCEMENT ────────────────────────────────────────────
fn flag_overdue_milestone(env: Env, campaign_id: u64, milestone_index: u32)
// Permissionless: anyone calls after deadline + 7 days
// 7 days overdue  → yellow flag
// 14 days overdue → public explanation required
// 30 days overdue → admin auto-review triggered
// 60 days overdue → admin can call terminate_campaign()

// ── QUERIES ────────────────────────────────────────────────────────────────
fn get_campaign(env: Env, campaign_id: u64) -> Campaign
fn get_milestone(env: Env, campaign_id: u64, idx: u32) -> CampaignMilestone
fn get_backer_pledge(env: Env, campaign_id: u64, backer: Address) -> i128
fn get_backer_list(env: Env, campaign_id: u64) -> Vec<Address>
fn total_raised(env: Env, campaign_id: u64) -> i128
fn time_remaining(env: Env, campaign_id: u64) -> i64
```

### Events Emitted

```
CampaignCreated      { id, creator, goal, asset, duration }
CampaignSubmitted    { id }
CampaignApproved     { id, admin }
CampaignRejected     { id, admin }
VotingOpened         { campaign_id, session_id, threshold }
VotingThresholdMet   { campaign_id }
CampaignLaunched     { campaign_id }
PledgeReceived       { campaign_id, backer, amount, total_raised }
CampaignFunded       { campaign_id, total_raised }
CampaignFailed       { campaign_id }
MilestoneActivated   { campaign_id, milestone_index }
MilestoneSubmitted   { campaign_id, milestone_index }
MilestoneApproved    { campaign_id, milestone_index, amount_released }
MilestoneDisputed    { campaign_id, milestone_index }
CampaignCompleted    { campaign_id }
CampaignTerminated   { campaign_id }
```

---

## Module Contract 3: `GrantHub`

**Handles three fundamentally different grant types** under one contract, each with its own state machine and payout logic.

### Grant Sub-Types

| Type | Key Actors | When Recipient Known | Payout Structure |
|------|-----------|---------------------|-----------------|
| Milestone Grant | Admin → Recipient | At grant creation | Staged (per milestone) |
| Retrospective | Community → Many applicants | After voting | Lump sum to winner(s) |
| Quadratic (QF) | Donors + Matching Pool | After round ends | Formula-driven distribution |

### Storage Schema

```rust
struct Grant {
    id: u64,
    grant_type: GrantType,
    creator: Address,            // org/ecosystem posting the grant
    project_id: u64,
    metadata_cid: String,
    category: Symbol,
    status: GrantStatus,
    total_budget: i128,
    asset: Asset,
    pool_id: BytesN<32>,

    // Milestone grant only
    recipient: Option<Address>,
    milestones: Vec<GrantMilestone>,

    // Retrospective only
    vote_session_id: Option<BytesN<32>>,
    applicants: Vec<Address>,
    winner: Option<Address>,
    winner_amount: Option<i128>,

    // QF only
    qf_round_id: Option<u64>,
    matching_pool: Option<i128>, // separate from donations

    created_at: u64,
    deadline: u64,
}

struct GrantMilestone {
    index: u32,
    description_cid: String,
    budget_pct: u8,
    amount: i128,
    deadline: u64,
    status: MilestoneStatus,     // reuses Crowdfund enum
    submission_cid: Option<String>,
}

struct QFRound {
    id: u64,
    metadata_cid: String,
    matching_pool: i128,
    asset: Asset,
    pool_id: BytesN<32>,         // holds matching pool
    vote_session_id: BytesN<32>, // tracks donations via GovernanceVoting
    start_at: u64,
    end_at: u64,
    status: QFStatus,
    projects: Vec<u64>,          // project_ids eligible for this round
}

enum GrantType {
    Milestone,      // staged release to known recipient
    Retrospective,  // lump sum, winner decided by vote
    Quadratic,      // formula-driven, donor-signaled
}

enum GrantStatus {
    Draft,
    Active,         // open for applications (Retrospective) or active (Milestone)
    Voting,         // Retrospective: community voting in progress
    Distributing,   // QF: round ended, computing distributions
    Completed,
    Cancelled,
}

enum QFStatus {
    Setup,
    Active,
    Ended,
    Distributed,
}
```

### Functions — Milestone Grant

```rust
fn create_milestone_grant(
    env: Env,
    creator: Address,
    project_id: u64,
    recipient: Address,
    metadata_cid: String,
    total_budget: i128,
    asset: Asset,
    milestones: Vec<GrantMilestoneInput>,
) -> u64
// Deposits into CoreEscrow immediately
// Defines release slots per milestone
// Status → Active, Milestone 0 activated
// Identical milestone flow to CrowdfundRegistry::submit_milestone / approve_milestone

fn submit_grant_milestone(
    env: Env,
    recipient: Address,
    grant_id: u64,
    milestone_index: u32,
    submission_cid: String,
)

fn approve_grant_milestone(
    env: Env,
    admin: Address,
    grant_id: u64,
    milestone_index: u32,
)
// CoreEscrow::release_slot(pool_id, milestone_index)
// ReputationRegistry::record_grant_received(recipient, amount)
// If last milestone → status → Completed

fn reject_grant_milestone(
    env: Env,
    admin: Address,
    grant_id: u64,
    milestone_index: u32,
    feedback_cid: String,
)
```

### Functions — Retrospective Grant

```rust
fn create_retrospective_grant(
    env: Env,
    creator: Address,
    project_id: u64,
    metadata_cid: String,
    total_budget: i128,
    asset: Asset,
    voting_duration_days: u32,
    options: Vec<String>,   // project names / applicants
) -> u64
// Deposits into CoreEscrow (NOT locked yet)
// Creates GovernanceVoting session (RetrospectiveGrant context)
// Status → Active (collecting votes)

fn vote_retrospective(
    env: Env,
    voter: Address,
    grant_id: u64,
    option_id: u32,
)
// Delegates to GovernanceVoting::cast_vote
// Reputation-weighted (high rep = more weight)

fn finalize_retrospective(env: Env, grant_id: u64)
// Permissionless: callable after voting ends
// Reads GovernanceVoting results
// Determines winner(s) by most weighted votes
// CoreEscrow::lock_pool() then ::release_partial() to winner(s)
// Can support top-N winners with proportional shares:
//   winner_share = weighted_votes_i / total_weighted_votes * total_budget
// Status → Completed
// ReputationRegistry::record_grant_received for each winner
```

### Functions — Quadratic Funding Round

```rust
fn create_qf_round(
    env: Env,
    creator: Address,
    project_id: u64,
    metadata_cid: String,
    matching_pool_amount: i128,
    asset: Asset,
    eligible_projects: Vec<u64>,
    start_at: u64,
    end_at: u64,
) -> u64
// Deposits matching_pool into CoreEscrow (separate pool for matching)
// Creates GovernanceVoting session (QFRound context) for donor signals
// Status → Setup

fn add_qf_project(env: Env, admin: Address, round_id: u64, project_id: u64)
// Adds project as eligible option in vote session

fn donate_to_project(
    env: Env,
    donor: Address,
    round_id: u64,
    project_id: u64,
    amount: i128,
)
// Transfers donation directly to project's separate donation pool
// Records signal in GovernanceVoting::record_qf_donation()
// NOTE: donations go direct to project, NOT to matching pool
// This matches QF spec: matching pool supplements community donations

fn finalize_qf_round(env: Env, round_id: u64)
// Permissionless: callable after end_at
// Calls GovernanceVoting::compute_qf_distribution(session_id, matching_pool)
// Releases matching funds to each project proportionally:
//   CoreEscrow::release_partial(matching_pool_id, project_addr, match_amount)
// Status → Distributed
// Each project receives: their donations + their QF match

// QF Formula (computed in GovernanceVoting, pure on-chain):
// For project i with donations [d1, d2, ..., dn]:
//   contribution_i = (√d1 + √d2 + ... + √dn)²
//   match_i = contribution_i / Σ(contribution_j) * matching_pool

fn get_qf_round(env: Env, round_id: u64) -> QFRound
fn get_project_donations(env: Env, round_id: u64, project_id: u64) -> i128
fn preview_qf_distribution(env: Env, round_id: u64) -> Vec<(u64, i128)>
```

### Events Emitted

```
GrantCreated         { id, grant_type, creator, budget }
GrantMilestoneApproved { grant_id, milestone_index, amount_released }
RetrospectiveVoteOpen { grant_id, session_id }
RetrospectiveFinalized { grant_id, winner, amount }
QFRoundOpened        { round_id, matching_pool, eligible_projects }
QFDonation           { round_id, donor, project_id, amount }
QFRoundFinalized     { round_id, distributions: Vec<(project_id, amount)> }
```

---

## Module Contract 4: `HackathonRegistry`

**Manages competitive events** with multiple parallel tracks, judge-based scoring, and ranked prize distribution.

### Hackathon Sub-Types

| Type | Prize Logic | Parallel Tracks |
|------|-------------|----------------|
| Traditional | 1st/2nd/3rd ranked from one pool | No |
| Sponsored Tracks | "Best UI", "Best DeFi" etc. | Yes — each track has its own pool |

### Storage Schema

```rust
struct Hackathon {
    id: u64,
    organizer: Address,
    project_id: u64,
    metadata_cid: String,
    category: Symbol,
    status: HackathonStatus,
    main_prize_pool: i128,
    asset: Asset,
    pool_id: BytesN<32>,         // main prize pool escrow
    tracks: Vec<HackathonTrack>,
    judges: Vec<Address>,
    max_participants: u32,
    min_team_size: u8,
    max_team_size: u8,
    submission_deadline: u64,
    judging_deadline: u64,
    start_at: u64,
    created_at: u64,
    prize_distribution: Vec<PrizeTier>, // for Traditional: 1st=60%, 2nd=30%, 3rd=10%
}

struct HackathonTrack {
    id: u32,
    name: String,               // "Best UI", "Best DeFi Integration"
    description_cid: String,
    sponsor: Option<Address>,   // external sponsor for this track
    prize_pool: i128,
    asset: Asset,
    pool_id: BytesN<32>,        // separate escrow pool per track
    max_winners: u32,           // usually 1-3
    prize_distribution: Vec<PrizeTier>,
    vote_session_id: BytesN<32>, // judge scoring session
    winners: Vec<(Address, u32, i128)>, // (winner_addr, rank, prize_amount)
}

struct HackathonSubmission {
    hackathon_id: u64,
    team_lead: Address,
    team_members: Vec<Address>,
    project_name: String,
    submission_cid: String,      // IPFS: demo, repo, deck, video
    submitted_at: u64,
    track_ids: Vec<u32>,         // which tracks they're entering
    scores: Map<Address, u8>,    // judge_address → score (1-10)
    final_score: Option<u32>,    // weighted average * 100 (fixed-point)
    rank: Option<u32>,           // 1=first, 2=second, etc.
    disqualified: bool,
}

struct PrizeTier {
    rank: u32,            // 1, 2, 3
    pct: u8,             // e.g., 60, 30, 10
    amount: i128,        // calculated from pool
}

enum HackathonStatus {
    Draft,
    Published,        // open for registration
    Active,           // building period
    Judging,          // submission deadline passed, judges scoring
    Distributing,     // winners determined, paying out
    Completed,
    Cancelled,
}
```

### Functions

```rust
// ── SETUP ──────────────────────────────────────────────────────────────────
fn create_hackathon(
    env: Env,
    organizer: Address,
    project_id: u64,
    metadata_cid: String,
    main_prize_pool: i128,
    asset: Asset,
    prize_tiers: Vec<(u32, u8)>,  // (rank, pct)
    start_at: u64,
    submission_deadline: u64,
    judging_deadline: u64,
    judges: Vec<Address>,
    max_participants: u32,
) -> u64
// Deposits main_prize_pool → CoreEscrow (NOT locked until Active)
// Validates: prize_tiers pct sum == 100

fn add_sponsored_track(
    env: Env,
    organizer: Address,   // or sponsor directly
    hackathon_id: u64,
    track_name: String,
    description_cid: String,
    sponsor: Address,
    prize_pool: i128,
    asset: Asset,
    prize_tiers: Vec<(u32, u8)>,
    max_winners: u32,
) -> u32  // returns track_id
// Creates separate CoreEscrow pool for this track
// Sponsor deposits prize_pool
// Creates separate GovernanceVoting::HackathonJudging session for track

fn add_judge(env: Env, organizer: Address, hackathon_id: u64, judge: Address)
fn remove_judge(env: Env, organizer: Address, hackathon_id: u64, judge: Address)

fn publish_hackathon(env: Env, organizer: Address, hackathon_id: u64)
// Status → Published

// ── PARTICIPATION ──────────────────────────────────────────────────────────
fn register_team(
    env: Env,
    team_lead: Address,
    hackathon_id: u64,
    team_members: Vec<Address>,
    track_ids: Vec<u32>,        // which tracks they plan to enter
)
// Validates: participant count within max, team size within bounds
// SparkCredits::spend(team_lead) — 1 credit to enter

fn submit_project(
    env: Env,
    team_lead: Address,
    hackathon_id: u64,
    project_name: String,
    submission_cid: String,
    track_ids: Vec<u32>,
)
// Validates: before submission_deadline, status == Active
// Creates HackathonSubmission record
// All submissions sealed until judging_deadline (blind review)

fn withdraw_submission(env: Env, team_lead: Address, hackathon_id: u64)
// Before judging starts only

// ── JUDGING ────────────────────────────────────────────────────────────────
fn open_judging(env: Env, hackathon_id: u64)
// Permissionless: callable after submission_deadline
// Status → Judging
// All submissions revealed
// GovernanceVoting sessions activated for all tracks
// Creates judging session per track + one for main competition

fn judge_submission(
    env: Env,
    judge: Address,
    hackathon_id: u64,
    team_lead: Address,     // submission identifier
    track_id: u32,
    score: u8,              // 1-10
    comments_cid: Option<String>,
)
// Auth: judge must be in hackathon.judges list
// Delegates to GovernanceVoting::cast_vote (HackathonJudging context)
// Each judge votes once per submission per track

fn finalize_judging(env: Env, hackathon_id: u64)
// Permissionless: callable after judging_deadline
// Aggregates scores: final_score = sum(judge_scores) / judge_count * 100
// Ranks submissions by final_score
// Assigns rank 1, 2, 3... to top performers
// Handles ties: same score = same rank, next rank skipped
// Status → Distributing

// ── PRIZE DISTRIBUTION ─────────────────────────────────────────────────────
fn distribute_prizes(env: Env, hackathon_id: u64)
// Auth: organizer (or permissionless after auto-distribution delay)
// For main competition:
//   Calculates prize per tier from prize_tiers
//   CoreEscrow::release_partial(pool_id, winner_1, 1st_prize)
//   CoreEscrow::release_partial(pool_id, winner_2, 2nd_prize)
//   etc.
// For each track:
//   CoreEscrow::release_partial(track_pool_id, track_winner, track_prize)
// ReputationRegistry::record_hackathon_win() for each winner
// SparkCredits::award(+1) to all participants (Surge-like bonus)
// Status → Completed

fn distribute_track_prizes(
    env: Env,
    hackathon_id: u64,
    track_id: u32,
)
// Can distribute one track independently if sponsors want early release

fn disqualify_submission(
    env: Env,
    admin: Address,
    hackathon_id: u64,
    team_lead: Address,
    reason_cid: String,
)
// Marks submission.disqualified = true
// Excluded from prize distribution
// Prize redistributed to next-ranked team

// ── QUERIES ────────────────────────────────────────────────────────────────
fn get_hackathon(env: Env, id: u64) -> Hackathon
fn get_track(env: Env, hackathon_id: u64, track_id: u32) -> HackathonTrack
fn get_submission(env: Env, hackathon_id: u64, team_lead: Address) -> HackathonSubmission
fn get_leaderboard(env: Env, hackathon_id: u64) -> Vec<(Address, u32, u32)> // (team, rank, score)
fn get_track_leaderboard(env: Env, hackathon_id: u64, track_id: u32) -> Vec<(Address, u32)>
fn total_participants(env: Env, hackathon_id: u64) -> u32
```

### Events Emitted

```
HackathonCreated       { id, organizer, main_prize_pool }
TrackAdded             { hackathon_id, track_id, sponsor, prize_pool }
TeamRegistered         { hackathon_id, team_lead, track_ids }
ProjectSubmitted       { hackathon_id, team_lead, track_ids }
JudgingOpened          { hackathon_id }
SubmissionScored       { hackathon_id, team_lead, judge, score, track_id }
JudgingFinalized       { hackathon_id, rankings: Vec<(Address, u32)> }
PrizesDistributed      { hackathon_id, distributions: Vec<(Address, i128)> }
TrackPrizesDistributed { hackathon_id, track_id, distributions: Vec<(Address, i128)> }
SubmissionDisqualified { hackathon_id, team_lead }
```

---

## Cross-Module Interaction Summary

```
Every module registry interacts with these shared contracts:

ALL MODULES ──────────────────────────────────────────────────────────────────
  → CoreEscrow:         create_pool, lock_pool, release_slot, refund_all
  → PaymentRouter:      route_deposit, route_payout
  → ProjectRegistry:    validate_budget, lock_deposit, release_deposit
  → ReputationRegistry: record_completion / record_hackathon_win / etc.

BOUNTY ONLY ──────────────────────────────────────────────────────────────────
  → SparkCredits:       spend, restore, award

CROWDFUNDING + GRANTS ────────────────────────────────────────────────────────
  → GovernanceVoting:   create_session, cast_vote, conclude_session
  → GovernanceVoting:   record_qf_donation, compute_qf_distribution (QF only)

HACKATHONS ───────────────────────────────────────────────────────────────────
  → GovernanceVoting:   create_session (HackathonJudging), cast_vote
  → SparkCredits:       award (participant bonus post-event)
```

---

## Complete Contract Inventory

| # | Contract | Module(s) | LOC (est.) | Priority |
|---|----------|-----------|-----------|----------|
| 1 | `CoreEscrow` | All | ~450 | **MVP Core** |
| 2 | `PaymentRouter` | All | ~200 | **MVP Core** |
| 3 | `ReputationRegistry` | All | ~450 | **MVP Core** |
| 4 | `ProjectRegistry` | All | ~400 | **MVP Core** |
| 5 | `SparkCredits` | Bounty, Hackathon | ~250 | **MVP Core** |
| 6 | `GovernanceVoting` | Crowdfund, Grants, Hackathon | ~500 | **MVP Core** |
| 7 | `BountyRegistry` | Bounty | ~800 | **MVP Core** |
| 8 | `CrowdfundRegistry` | Crowdfunding | ~700 | Phase 1 |
| 9 | `GrantHub` | Grants (all 3 types) | ~750 | Phase 2 |
| 10 | `HackathonRegistry` | Hackathons (both types) | ~700 | Phase 2 |

> **Total estimated codebase: ~5,200 lines of Soroban Rust**

---

## Deployment Order

```
Phase 0 — Infrastructure (no deps):
  1. CoreEscrow
  2. PaymentRouter
  3. ReputationRegistry
  4. SparkCredits
  5. GovernanceVoting

Phase 1 — Registry Layer (deps on Phase 0):
  6. ProjectRegistry        (needs CoreEscrow, PaymentRouter)
  7. BountyRegistry         (needs all Phase 0 + ProjectRegistry)
  8. CrowdfundRegistry      (needs CoreEscrow, PaymentRouter, Governance, Reputation)

Phase 2 — Extended Modules:
  9. GrantHub               (needs CoreEscrow, Governance, Reputation)
  10. HackathonRegistry     (needs CoreEscrow, Governance, Reputation, SparkCredits)

Post-deploy: Wire cross-contract addresses via set_contracts() on each registry.
```

---

## Security Model

### Authorization Layers

```
Level 1 — User Auth:     contributor.require_auth() / creator.require_auth()
Level 2 — Module Auth:   CoreEscrow::release only callable by module registries
Level 3 — Admin Auth:    Fraud flags, insurance claims, suspension: Admin multisig (3-of-5)
Level 4 — Time-gates:    Permissionless calls only succeed after timestamps pass
```

### Key Invariants

1. **Escrow is monotonically decreasing**: `total_deposited >= total_released + total_refunded` always holds
2. **No double-release**: Each slot has `released: bool`, flipped atomically
3. **QF math overflow protection**: All square root operations use u128 intermediate values
4. **Vote session uniqueness**: One active session per module_id per VoteContext
5. **Upgrade safety**: All contracts expose `upgrade(new_wasm_hash)` behind admin multisig

---

*Boundless Platform — Full Smart Contract Architecture | Stellar / Soroban | v2.0*
*Covers: Bounties (4 types) · Crowdfunding · Grants (3 types) · Hackathons (2 types)*
