Boundless Bounty — Soroban Smart Contract Architecture Plan
Version 1.0 | Based on Platform Documentation v1.0

Overview
The Boundless Bounty platform requires 6 core Soroban contracts operating as an interconnected system on Stellar. Each contract has a clearly scoped responsibility, and they communicate via cross-contract calls using Stellar's native authorization model.
┌─────────────────────────────────────────────────────────────┐
│                    BOUNDLESS BOUNTY CONTRACTS               │
│                                                             │
│   BountyRegistry ──────► EscrowVault                       │
│         │                     ▲                            │
│         ▼                     │                            │
│   MilestoneManager        PaymentRouter                    │
│         │                     ▲                            │
│         ▼                     │                            │
│   ReputationRegistry ──► SparkCredits                      │
│         │                                                  │
│         ▼                                                  │
│   ProjectRegistry                                          │
└─────────────────────────────────────────────────────────────┘

Contract 1: BountyRegistry
The central hub. Manages the full lifecycle of every bounty posted on the platform.
Storage Schema
// Bounty core data
struct Bounty {
    id: u64,
    creator: Address,
    title: String,       // stored off-chain (IPFS CID stored here)
    metadata_cid: String, // IPFS CID → title, desc, requirements
    skill_tags: Vec<Symbol>,
    model: BountyModel,
    status: BountyStatus,
    budget_xlm: i128,
    asset: Asset,        // XLM or project token
    deadline: u64,       // Unix timestamp
    created_at: u64,
    max_applicants: u32,
    kyc_required: u8,    // 0 = none, 1 = basic, 2 = full
    project_id: u64,
}

enum BountyModel {
    SingleClaim,
    Application,
    Competition,
    MultiWinner,
}

enum BountyStatus {
    Open,
    Claimed,       // SingleClaim only
    InReview,
    Completed,
    Cancelled,
    Expired,
}

// Application record
struct Application {
    bounty_id: u64,
    applicant: Address,
    proposal_cid: String, // IPFS CID for proposal text
    submitted_at: u64,
    status: AppStatus,
    deliverable_cid: Option<String>,
    rating: Option<u8>,   // 1-5, set by project on completion
}

enum AppStatus {
    Pending,
    Selected,
    Rejected,
    Submitted,    // work submitted
    Approved,
    RevisionRequested,
}
Key Data Maps (Soroban persistent storage)
   Key Value Description     Bounty(id) Bounty Core bounty data   BountyCount u64 Auto-increment ID counter   Applications(bounty_id) Vec<Address> Applicant list per bounty   Application(bounty_id, address) Application Per-applicant record   ProjectBounties(project_id) Vec<u64> Bounty IDs per project   UserApplications(address) Vec<u64> Bounty IDs a user applied to   ClaimedBy(bounty_id) Address SingleClaim lock holder   ClaimTime(bounty_id) u64 Timestamp of claim (for auto-release)   Functions
// ── CREATION ──────────────────────────────────────────────
fn create_bounty(
    env: Env,
    creator: Address,
    project_id: u64,
    metadata_cid: String,
    skill_tags: Vec<Symbol>,
    model: BountyModel,
    budget: i128,
    asset: Asset,
    deadline: u64,
    max_applicants: u32,
    kyc_required: u8,
) -> u64  // returns bounty_id
// Validates: project verification level vs budget, escrow funded

// ── SINGLE CLAIM MODEL ────────────────────────────────────
fn claim_bounty(env: Env, claimer: Address, bounty_id: u64)
// Deducts 1 SparkCredit, locks bounty to claimer
// Validates: model == SingleClaim, status == Open, KYC level

fn auto_release_check(env: Env, bounty_id: u64)
// Anyone can call; checks if 7 days passed with no submission
// If stale: resets status to Open, penalizes reputation

// ── APPLICATION MODEL ─────────────────────────────────────
fn apply(
    env: Env,
    applicant: Address,
    bounty_id: u64,
    proposal_cid: String,
)
// Deducts 1 SparkCredit, creates Application record

fn select_applicant(
    env: Env,
    creator: Address,
    bounty_id: u64,
    applicant: Address,
)
// Returns credits to all rejected applicants via SparkCredits contract
// Sets selected applicant status to Selected

// ── COMPETITION MODEL ─────────────────────────────────────
fn enter_competition(env: Env, entrant: Address, bounty_id: u64)
// Validates: slots available (max_applicants), model == Competition

fn submit_work(
    env: Env,
    contributor: Address,
    bounty_id: u64,
    deliverable_cid: String,
)
// Submits work. For Competition: sealed until deadline

// ── REVIEW & PAYMENT ──────────────────────────────────────
fn approve_submission(
    env: Env,
    creator: Address,
    bounty_id: u64,
    contributor: Address,
    rating: u8,           // 1-5
)
// Triggers EscrowVault.release(), updates ReputationRegistry

fn request_revision(
    env: Env,
    creator: Address,
    bounty_id: u64,
    contributor: Address,
    feedback_cid: String,
)

fn reject_submission(
    env: Env,
    creator: Address,
    bounty_id: u64,
    contributor: Address,
    reason_cid: String,
)
// Refunds 1 SparkCredit to contributor

fn cancel_bounty(env: Env, creator: Address, bounty_id: u64)
// Only if no active claims/selections; returns escrow to creator
// Refunds all applicant SparkCredits

// ── QUERIES ───────────────────────────────────────────────
fn get_bounty(env: Env, bounty_id: u64) -> Bounty
fn get_applications(env: Env, bounty_id: u64) -> Vec<Application>
fn get_user_applications(env: Env, user: Address) -> Vec<u64>
fn is_stale(env: Env, bounty_id: u64) -> bool
Events Emitted
BountyCreated { id, creator, model, budget, deadline }
BountyClaimed { id, claimer }
ApplicationSubmitted { bounty_id, applicant }
ApplicantSelected { bounty_id, applicant }
WorkSubmitted { bounty_id, contributor }
WorkApproved { bounty_id, contributor, rating }
BountyCancelled { id }
BountyExpired { id }

Contract 2: EscrowVault
Trustless fund custody. Holds all bounty budgets in escrow; funds are unreachable by the creator once locked.
Storage Schema
struct EscrowEntry {
    bounty_id: u64,
    project: Address,
    total_amount: i128,
    asset: Asset,
    locked: bool,
    released: i128,     // cumulative released (for milestone bounties)
}
Functions
// ── DEPOSIT ───────────────────────────────────────────────
fn deposit(
    env: Env,
    project: Address,
    bounty_id: u64,
    amount: i128,
    asset: Asset,
)
// Called at bounty creation. Transfers tokens into contract's account.
// Sets locked = true

// ── RELEASE ───────────────────────────────────────────────
fn release(
    env: Env,
    bounty_id: u64,
    recipient: Address,
    amount: i128,
)
// Auth: only callable by BountyRegistry or MilestoneManager
// Transfers amount from escrow to recipient
// For MultiWinner: called per milestone completion

// ── REFUND ────────────────────────────────────────────────
fn refund(env: Env, bounty_id: u64)
// Called on cancellation or expiry
// Returns unreleased funds to project
// Deducts any forfeited deposit (ProjectRegistry integration)

// ── INSURANCE FUND ────────────────────────────────────────
fn contribute_to_insurance(env: Env, bounty_id: u64, amount: i128)
// 5% of platform fee auto-deposited here on each completion

fn claim_insurance(
    env: Env,
    contributor: Address,
    bounty_id: u64,
    evidence_cid: String,
)
// Verified claims only; requires admin multisig approval

// ── QUERIES ───────────────────────────────────────────────
fn get_escrow(env: Env, bounty_id: u64) -> EscrowEntry
fn insurance_balance(env: Env) -> i128
fn get_unreleased(env: Env, bounty_id: u64) -> i128
Security Properties
release() is gated by require_auth from BountyRegistry address only
No direct project withdrawal once locked = true
All flows are event-logged for auditability
The insurance fund sub-account is behind an admin multisig

Contract 3: MilestoneManager
Multi-Winner bounty orchestration. Handles the funnel structure where contributors advance through stages.
Storage Schema
struct Milestone {
    id: u32,
    bounty_id: u64,
    name: String,
    description_cid: String,
    max_winners: u32,
    payout_per_winner: i128,
    total_budget: i128,
    deadline: u64,
    min_quality_score: u8,   // min rating to advance (e.g., 4)
    advance_top_n: u32,      // top N advance to next milestone
    status: MilestoneStatus,
}

enum MilestoneStatus {
    Locked,      // not yet open (previous milestone not done)
    Open,
    InReview,
    Completed,
}

struct MilestoneSubmission {
    milestone_id: u32,
    contributor: Address,
    deliverable_cid: String,
    submitted_at: u64,
    score: Option<u8>,
    advanced: bool,   // eligible for next milestone
}
Functions
fn create_milestone_bounty(
    env: Env,
    creator: Address,
    bounty_id: u64,
    milestones: Vec<MilestoneInput>, // all milestones defined upfront
)
// Validates total budget == sum of (max_winners * payout) per milestone

fn enter_milestone(
    env: Env,
    contributor: Address,
    bounty_id: u64,
    milestone_id: u32,
)
// M1: open to all qualifying contributors
// M2+: requires advanced == true from previous milestone

fn submit_milestone_work(
    env: Env,
    contributor: Address,
    milestone_id: u32,
    deliverable_cid: String,
)

fn approve_milestone_submission(
    env: Env,
    creator: Address,
    milestone_id: u32,
    contributor: Address,
    score: u8,       // 1-5
)
// If score >= min_quality_score: pays contributor immediately
// Calculates advancement eligibility

fn finalize_milestone(env: Env, creator: Address, milestone_id: u32)
// Closes review window, determines top_N advancers
// Unlocks next milestone
// Refunds budget for unfilled winner slots

fn get_milestone_standings(env: Env, milestone_id: u32) -> Vec<(Address, u8)>
// Returns ranked list of contributors by score

Contract 4: ReputationRegistry
On-chain contributor credibility. Stores weighted reputation scores, skill-specific ratings, and level classifications.
Storage Schema
struct ContributorProfile {
    address: Address,
    overall_score: u32,
    level: u8,               // 1-5
    bounties_completed: u32,
    total_ratings: u32,      // sum of all rating points received
    ratings_count: u32,      // number of ratings (avg = total/count)
    on_time_count: u32,      // delivered before deadline
    late_count: u32,
    abandoned_count: u32,
    skill_scores: Map<Symbol, SkillScore>,
    joined_at: u64,
    last_active: u64,
}

struct SkillScore {
    skill: Symbol,
    total_rating: u32,
    rating_count: u32,
    completions: u32,
}
Reputation Formula (on-chain)
overall_score = (
  (bounties_completed * 40)         // Completion volume

+ (avg_rating * 30)               // Quality (1-5 scaled to 0-30)
+ (on_time_rate *15)             // Timeliness (%* 15)
+ (collab_score * 10)             // From MultiWinner bounties
+ (community_bonus * 5)           // Referrals, bug reports
)
Level thresholds: 0-100 → L1, 101-300 → L2, 301-600 → L3, 601-1000 → L4, 1001+ → L5
Functions
fn initialize_profile(env: Env, contributor: Address)
// Called on first bounty application. Creates profile with zeroed scores.

fn record_completion(
    env: Env,
    contributor: Address,
    bounty_id: u64,
    rating: u8,           // 1-5 from project
    skill_tag: Symbol,
    delivered_before_deadline: bool,
    is_collab: bool,      // was this a MultiWinner bounty
)
// Auth: only BountyRegistry or MilestoneManager
// Updates all score components, recalculates level

fn record_abandonment(env: Env, contributor: Address)
// -10 points, increments abandoned_count

fn record_late_delivery(env: Env, contributor: Address)
// -5 points

fn record_fraud(env: Env, contributor: Address)
// -100 points + emit FraudFlagged event (triggers ban flow off-chain)

fn add_community_bonus(
    env: Env,
    contributor: Address,
    reason: Symbol,  // Referral, BugReport, SurgeEvent, etc.
    points: u32,
)
// Auth: Admin only

fn get_profile(env: Env, contributor: Address) -> ContributorProfile
fn get_level(env: Env, contributor: Address) -> u8
fn get_skill_rating(env: Env, contributor: Address, skill: Symbol) -> u32
fn meets_requirements(
    env: Env,
    contributor: Address,
    min_level: u8,
    required_skill: Option<Symbol>,
    min_skill_rating: Option<u32>,
) -> bool

Contract 5: SparkCredits
Anti-spam application credits. Non-transferable, account-bound credits with multiple earning vectors.
Storage Schema
struct CreditBalance {
    address: Address,
    credits: u8,         // current balance (max 10, Level 3+ gets 11)
    max_credits: u8,     // 10 by default
    last_recharge: u64,  // epoch timestamp of last bi-weekly recharge
    total_earned: u32,   // lifetime stat
    total_spent: u32,    // lifetime stat
}

// For recharge scheduling
const RECHARGE_AMOUNT: u8 = 3;
const RECHARGE_INTERVAL_SECS: u64 = 1_209_600; // 14 days
Functions
fn initialize(env: Env, user: Address)
// Called on first platform interaction
// Grants 3 starting credits

fn spend(env: Env, user: Address, bounty_id: u64) -> bool
// Deducts 1 credit. Returns false if balance == 0.
// Auth: only BountyRegistry

fn restore(env: Env, user: Address, reason: Symbol)
// +1 credit: rejection, competition loss, cancelled bounty
// Auth: only BountyRegistry

fn award(
    env: Env,
    user: Address,
    amount: u8,
    reason: Symbol, // Completion, HighQuality, EarlyDelivery, Referral, etc.
)
// Auth: BountyRegistry, ReputationRegistry, or Admin

fn try_recharge(env: Env, user: Address)
// Anyone can call on behalf of a user
// Checks if 14 days elapsed since last_recharge
// If so: +3 credits (capped at max), updates last_recharge

fn get_balance(env: Env, user: Address) -> u8
fn next_recharge_at(env: Env, user: Address) -> u64
fn can_apply(env: Env, user: Address) -> bool
Award Logic Summary
   Trigger Amount Auth Caller     Bi-weekly recharge +3 Anyone (time-gated)   Bounty completed +2 BountyRegistry   5/5 rating received +3 BountyRegistry   Delivered in <50% of deadline +1 BountyRegistry   Not selected (Application model) +1 (restore) BountyRegistry   Competition, didn't win +1 (restore) BountyRegistry   Bounty cancelled by creator +1 (restore) BountyRegistry   Referral completed 1 bounty +1 Admin   Valid bug report +2 Admin   Surge event participation +1 Admin
Contract 6: ProjectRegistry
Project verification and deposit management. Controls which projects can post what budget tiers and enforces anti-scam deposits.
Storage Schema
struct Project {
    id: u64,
    owner: Address,
    org_name: String,
    metadata_cid: String,   // website, description, social links
    verification_level: u8, // 0, 1, 2
    deposit_held: i128,     // current deposit balance
    active_bounty_budget: i128, // sum of open bounty budgets
    total_bounties_posted: u32,
    total_paid_out: i128,
    avg_contributor_rating: u32,
    dispute_count: u32,
    missed_milestones: u32,
    warning_level: u8,      // 0=none, 1=yellow, 2=orange, 3=red
    suspended: bool,
}
Budget & Deposit Rules (from documentation)
   Verification Level Max Per Bounty Platform Total Deposit Rate     Level 0 (Unverified) 2,000 XLM 10,000 XLM 10%   Level 1 (Basic) 10,000 XLM 50,000 XLM 5%   Level 2 (Full) Unlimited Unlimited 0%   Functions
fn register_project(
    env: Env,
    owner: Address,
    metadata_cid: String,
) -> u64  // returns project_id
// Starts at Level 0

fn upgrade_verification(
    env: Env,
    project_id: u64,
    new_level: u8,
    attestation_cid: String, // KYC/legal docs reference
)
// Auth: Admin (after off-chain KYC review)

fn lock_deposit(
    env: Env,
    project_id: u64,
    bounty_budget: i128,
)
// Called at bounty creation
// Calculates required deposit = budget * deposit_rate
// Transfers deposit from project to contract

fn release_deposit(env: Env, project_id: u64, bounty_id: u64)
// Called on successful bounty completion
// Returns deposit to project

fn forfeit_deposit(env: Env, project_id: u64, reason: Symbol)
// Auth: Admin only
// Deposits forfeited amount → EscrowVault insurance fund

fn validate_budget(
    env: Env,
    project_id: u64,
    budget: i128,
) -> bool
// Checks verification level budget limits

fn record_dispute(env: Env, project_id: u64)
fn record_missed_milestone(env: Env, project_id: u64)
// Both update warning_level automatically:
// 3 disputes OR 3 missed milestones → triggers warning upgrade

fn suspend_project(env: Env, project_id: u64)
// Auth: Admin only; blocks new bounty creation

fn get_project(env: Env, project_id: u64) -> Project
fn get_warning_level(env: Env, project_id: u64) -> u8
fn is_suspended(env: Env, project_id: u64) -> bool

Cross-Contract Call Flow: Full Bounty Lifecycle
Flow 1: Single Claim Bounty

1. Project calls BountyRegistry::create_bounty()
   └─► ProjectRegistry::validate_budget() ✓
   └─► ProjectRegistry::lock_deposit() ← transfers deposit
   └─► EscrowVault::deposit() ← transfers full budget to escrow
   └─► Stores Bounty{status: Open}

2. Contributor calls BountyRegistry::claim_bounty()
   └─► ReputationRegistry::meets_requirements() ✓
   └─► SparkCredits::spend() ← deducts 1 credit
   └─► Sets status: Claimed, ClaimedBy = contributor

3. Contributor calls BountyRegistry::submit_work()
   └─► Stores deliverable CID, status → InReview

4. [Auto-release check] If 7 days elapsed with no submission:
   └─► BountyRegistry::auto_release_check()
   └─► ReputationRegistry::record_abandonment()
   └─► Resets status → Open

5. Project calls BountyRegistry::approve_submission(rating)
   └─► EscrowVault::release() ← pays contributor
   └─► ReputationRegistry::record_completion()
   └─► SparkCredits::award() ← +2 or +3 based on rating
   └─► ProjectRegistry::release_deposit()
   └─► Status → Completed
Flow 2: Application + Selection Bounty
6. create_bounty() → escrow funded (same as above)

7. Contributor calls apply()
   └─► SparkCredits::spend()
   └─► Creates Application{status: Pending}

8. [N contributors apply during window]

9. Project calls select_applicant(winner)
   └─► Sets winner Application → Selected
   └─► For all others: SparkCredits::restore() (refund)

10. Winner submits → approved → paid (same as Step 3-5 above)
Flow 3: Competition Bounty
11. create_bounty() [Competition, consolation prizes configured]
   └─► EscrowVault::deposit(total = 1st + 2nd + 3rd prize)

12. Up to N contributors call enter_competition()
   └─► SparkCredits::spend() per entrant

13. All submit before deadline (submissions sealed by status flag)

14. After deadline: Project reviews all submissions

15. Project calls approve_submission() for 1st, 2nd, 3rd:
   └─► EscrowVault::release(1st_prize, winner_1)
   └─► EscrowVault::release(consolation, winner_2)
   └─► etc.
   └─► Non-winners: SparkCredits::restore()

16. Remaining escrow (unused consolation slots) → refunded to project
Flow 4: Multi-Winner Milestone Bounty
17. create_milestone_bounty() defines all milestones upfront
   └─► EscrowVault::deposit(sum of all milestone budgets)
   └─► MilestoneManager stores milestones[0..N]
   └─► Only M1 status = Open; M2+ = Locked

18. Contributors enter M1, submit, get scored
   └─► Each approved M1 submission → immediate payout
   └─► MilestoneManager tracks top_N scores

19. finalize_milestone(M1):
   └─► Determines top_N advancers (by score)
   └─► Unlocks M2
   └─► Refunds unused M1 slots to project

20. Repeat for M2, M3...

21. Final milestone completion → project fully done

Admin & Governance Functions
A separate Admin multisig account (2-of-3 or 3-of-5) controls sensitive operations:
   Function Contract Trigger     upgrade_verification ProjectRegistry Off-chain KYC confirmed   forfeit_deposit ProjectRegistry Fraud confirmed   suspend_project ProjectRegistry Policy violation   record_fraud ReputationRegistry Plagiarism/fraud confirmed   add_community_bonus SparkCredits Bug reports, referrals   claim_insurance EscrowVault Contributor complaint upheld
Deployment Order & Initialization
Deploy contracts in this sequence to avoid circular dependency issues:

1. EscrowVault          (no dependencies)
2. SparkCredits         (no dependencies)
3. ReputationRegistry   (no dependencies)
4. ProjectRegistry      (depends on EscrowVault)
5. MilestoneManager     (depends on EscrowVault, ReputationRegistry, SparkCredits)
6. BountyRegistry       (depends on all above — set addresses post-deploy)
After deployment, call BountyRegistry::set_contracts(escrow, credits, reputation, projects, milestones) to wire up cross-contract addresses.

Security Considerations
Authorization Model
Every state-mutating function checks contributor.require_auth() or creator.require_auth()
Cross-contract calls use invoker-gated auth: only BountyRegistry can call EscrowVault::release()
Admin functions require the admin multisig address
Reentrancy
Soroban's execution model prevents traditional reentrancy
Escrow state (locked, released amount) is updated before token transfer calls
Integer Overflow
All amounts use i128 (Soroban standard for token amounts)
Budget arithmetic validated with checked_add/checked_sub
Stale Bounty Handling
auto_release_check() is permissionless — any user (or a cron-bot) can call it
Protects contributors from indefinitely locked slots
Upgrade Path
Contracts should implement upgrade(new_wasm_hash) behind admin multisig
State migration logic included in each upgrade

Events Reference (Full Platform)
// BountyRegistry
BountyCreated     { id, creator, project_id, model, budget, skill_tags, deadline }
BountyClaimed     { id, claimer }
ApplicationSubmitted { bounty_id, applicant }
ApplicantSelected { bounty_id, applicant }
WorkSubmitted     { bounty_id, contributor, deliverable_cid }
WorkApproved      { bounty_id, contributor, rating, payout }
RevisionRequested { bounty_id, contributor }
WorkRejected      { bounty_id, contributor }
BountyCancelled   { id }
BountyExpired     { id }

// EscrowVault
Deposited         { bounty_id, project, amount, asset }
Released          { bounty_id, recipient, amount }
Refunded          { bounty_id, project, amount }
InsuranceClaimed  { bounty_id, contributor, amount }

// MilestoneManager
MilestoneOpened   { bounty_id, milestone_id }
MilestoneApproved { milestone_id, contributor, score }
MilestoneFinalized { milestone_id, advancers: Vec<Address> }

// ReputationRegistry
ScoreUpdated      { contributor, old_score, new_score, level_change }
FraudFlagged      { contributor, bounty_id }
AbandonmentRecorded { contributor, bounty_id }

// SparkCredits
CreditsSpent      { user, bounty_id, balance_after }
CreditsRestored   { user, reason, balance_after }
CreditsAwarded    { user, amount, reason, balance_after }
Recharged         { user, amount, next_recharge_at }

// ProjectRegistry
ProjectRegistered { id, owner }
VerificationUpgraded { project_id, new_level }
DepositForfeited  { project_id, amount, reason }
ProjectSuspended  { project_id }
WarningIssued     { project_id, warning_level }

Estimated Contract Complexity
   Contract Lines (est.) Complexity Priority     BountyRegistry ~800 High MVP Core   EscrowVault ~300 Medium MVP Core   SparkCredits ~250 Low MVP Core   ReputationRegistry ~400 Medium MVP Core   ProjectRegistry ~350 Medium MVP Core   MilestoneManager ~500 High Phase 2   MVP recommendation: Launch with BountyRegistry + EscrowVault + SparkCredits + ReputationRegistry using a simplified ProjectRegistry (Level 0/1 only). Add full MilestoneManager and deposit logic in Phase 2.

Boundless Bounty Smart Contract Architecture | Stellar / Soroban | v1.0
