/// Shared test harness that deploys and wires all 8 platform contracts.
use bounty_registry::{BountyRegistry, BountyRegistryClient};
use core_escrow::{CoreEscrow, CoreEscrowClient};
use crowdfund_registry::{CrowdfundRegistry, CrowdfundRegistryClient};
use governance_voting::{GovernanceVoting, GovernanceVotingClient};
use grant_hub::{GrantHub, GrantHubClient};
use hackathon_registry::{HackathonRegistry, HackathonRegistryClient};
use project_registry::{ProjectRegistry, ProjectRegistryClient};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env};

#[allow(dead_code)]
pub struct Platform<'a> {
    pub env: Env,
    pub admin: Address,
    pub treasury: Address,
    pub token_addr: Address,
    pub token: TokenClient<'a>,
    pub sac: StellarAssetClient<'a>,

    // Infrastructure
    pub escrow: CoreEscrowClient<'a>,
    pub reputation: ReputationRegistryClient<'a>,
    pub governance: GovernanceVotingClient<'a>,

    // Module registries
    pub project: ProjectRegistryClient<'a>,
    pub bounty: BountyRegistryClient<'a>,
    pub crowdfund: CrowdfundRegistryClient<'a>,
    pub grant: GrantHubClient<'a>,
    pub hackathon: HackathonRegistryClient<'a>,

    // Contract addresses (useful for authorization checks)
    pub escrow_addr: Address,
    pub reputation_addr: Address,
    pub governance_addr: Address,
    pub bounty_addr: Address,
    pub crowdfund_addr: Address,
    pub grant_addr: Address,
    pub hackathon_addr: Address,
}

/// Deploy all 8 contracts, initialize them, and wire cross-contract authorizations.
/// Mirrors the production deployment script (`scripts/deploy.sh`).
pub fn setup_platform() -> Platform<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    // --- Token ---
    let token_admin = Address::generate(&env);
    let token_addr = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token = TokenClient::new(&env, &token_addr);
    let sac = StellarAssetClient::new(&env, &token_addr);

    // --- Step 1: Deploy infrastructure ---
    let escrow_addr = env.register(CoreEscrow, ());
    let escrow = CoreEscrowClient::new(&env, &escrow_addr);
    escrow.init(&admin, &treasury);

    let reputation_addr = env.register(ReputationRegistry, ());
    let reputation = ReputationRegistryClient::new(&env, &reputation_addr);
    reputation.init(&admin);

    let governance_addr = env.register(GovernanceVoting, ());
    let governance = GovernanceVotingClient::new(&env, &governance_addr);
    governance.init(&admin);

    // --- Step 2: Deploy module registries ---
    let project_addr = env.register(ProjectRegistry, ());
    let project = ProjectRegistryClient::new(&env, &project_addr);
    project.init(&admin);

    let bounty_addr = env.register(BountyRegistry, ());
    let bounty = BountyRegistryClient::new(&env, &bounty_addr);
    bounty.init(&admin, &escrow_addr, &reputation_addr);

    let crowdfund_addr = env.register(CrowdfundRegistry, ());
    let crowdfund = CrowdfundRegistryClient::new(&env, &crowdfund_addr);
    crowdfund.init(&admin, &escrow_addr, &reputation_addr);

    let grant_addr = env.register(GrantHub, ());
    let grant = GrantHubClient::new(&env, &grant_addr);
    grant.init(&admin, &escrow_addr, &reputation_addr, &governance_addr);

    let hackathon_addr = env.register(HackathonRegistry, ());
    let hackathon = HackathonRegistryClient::new(&env, &hackathon_addr);
    hackathon.init(&admin, &escrow_addr, &reputation_addr);

    // --- Step 3: Wire authorization ---
    // CoreEscrow authorizes all 4 module registries
    escrow.authorize_module(&bounty_addr);
    escrow.authorize_module(&crowdfund_addr);
    escrow.authorize_module(&grant_addr);
    escrow.authorize_module(&hackathon_addr);

    // ReputationRegistry authorizes all 4 module registries
    reputation.add_authorized_module(&bounty_addr);
    reputation.add_authorized_module(&crowdfund_addr);
    reputation.add_authorized_module(&grant_addr);
    reputation.add_authorized_module(&hackathon_addr);

    // GovernanceVoting authorizes crowdfund, grant, hackathon
    governance.add_authorized_module(&crowdfund_addr);
    governance.add_authorized_module(&grant_addr);
    governance.add_authorized_module(&hackathon_addr);

    Platform {
        env,
        admin,
        treasury,
        token_addr,
        token,
        sac,
        escrow,
        reputation,
        governance,
        project,
        bounty,
        crowdfund,
        grant,
        hackathon,
        escrow_addr,
        reputation_addr,
        governance_addr,
        bounty_addr,
        crowdfund_addr,
        grant_addr,
        hackathon_addr,
    }
}
