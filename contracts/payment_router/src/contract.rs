use soroban_sdk::{contract, contractimpl, token, Address, Env, IntoVal, Symbol, Val, Vec};

use crate::error::Error;
use crate::events::{DepositRouted, FeeRateSet};
use crate::math;
use crate::storage::{DataKey, ModuleType};

#[contract]
pub struct PaymentRouter;

#[contractimpl]
impl PaymentRouter {
    pub fn init_payment_router(
        env: Env,
        admin: Address,
        treasury: Address,
        core_escrow: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage()
            .instance()
            .set(&DataKey::CoreEscrow, &core_escrow);

        // Default fee rates (bps)
        env.storage()
            .instance()
            .set(&DataKey::FeeRate(ModuleType::Bounty), &500_u32); // 5%
        env.storage()
            .instance()
            .set(&DataKey::FeeRate(ModuleType::Crowdfund), &500_u32);
        env.storage()
            .instance()
            .set(&DataKey::FeeRate(ModuleType::Grant), &300_u32); // 3%
        env.storage()
            .instance()
            .set(&DataKey::FeeRate(ModuleType::Hackathon), &400_u32); // 4%
        Ok(())
    }

    pub fn set_fee_rate(env: Env, module: ModuleType, rate_bps: u32) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        if rate_bps > 10000 {
            return Err(Error::RateExceedsLimit);
        }
        env.storage()
            .instance()
            .set(&DataKey::FeeRate(module), &rate_bps);

        FeeRateSet { module, rate_bps }.publish(&env);
        Ok(())
    }

    pub fn route_deposit(
        env: Env,
        payer: Address,
        gross_amount: i128,
        asset: Address,
        module: ModuleType,
    ) -> Result<i128, Error> {
        payer.require_auth();

        let fee_rate: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeRate(module))
            .unwrap_or(0);
        let total_fee = math::calculate_fee(gross_amount, fee_rate);

        if total_fee > 0 {
            let (insurance_portion, treasury_portion) = math::calculate_portions(total_fee, 500); // 5% of fee

            let treasury: Address = env
                .storage()
                .instance()
                .get(&DataKey::Treasury)
                .ok_or(Error::TreasuryNotSet)?;
            let core_escrow: Address = env
                .storage()
                .instance()
                .get(&DataKey::CoreEscrow)
                .ok_or(Error::EscrowNotSet)?;

            let token_client = token::Client::new(&env, &asset);

            // Transfer to Treasury
            if treasury_portion > 0 {
                token_client.transfer(&payer, &treasury, &treasury_portion);
            }

            // Transfer to Insurance (CoreEscrow) and notify
            if insurance_portion > 0 {
                token_client.transfer(&payer, &core_escrow, &insurance_portion);

                let func = Symbol::new(&env, "contribute_insurance");
                let mut call_args: Vec<Val> = Vec::new(&env);
                call_args.push_back(insurance_portion.into_val(&env));
                call_args.push_back(asset.clone().into_val(&env));

                env.invoke_contract::<()>(&core_escrow, &func, call_args);
            }
        }

        DepositRouted {
            payer,
            module,
            amount: gross_amount,
            fee: total_fee,
        }
        .publish(&env);

        Ok(math::calculate_net_amount(gross_amount, total_fee))
    }
}
