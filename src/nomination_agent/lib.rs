#![cfg_attr(not(feature = "std"), no_std, no_main)]

mod data;
pub mod errors;
pub mod traits;

pub use crate::nomination_agent::NominationAgentRef;

#[ink::contract]
mod nomination_agent {
    use crate::data::{BondExtra, ConfigOp, MultiAddress, NominationCall, PoolState, RuntimeCall};
    use crate::errors::RuntimeError;
    use crate::traits::INominationAgent;
    use ink::env::Error as EnvError;

    const BIPS: u128 = 10000;

    #[ink(storage)]
    pub struct NominationAgent {
        vault: AccountId,
        registry: AccountId,
        admin: AccountId,
        validator: AccountId,
        pool_id: Option<u32>,
        staked: u128,
        unbonding: u128,
        creation_bond: u128,
    }

    impl From<EnvError> for RuntimeError {
        fn from(e: EnvError) -> Self {
            match e {
                EnvError::CallRuntimeFailed => RuntimeError::CallRuntimeFailed,
                _ => panic!("Unexpected error from `pallet-contracts`."),
            }
        }
    }

    impl NominationAgent {
        #[ink(constructor)]
        pub fn deploy_hash() -> Self {
            let account_id = Self::env().account_id();
            NominationAgent {
                vault: account_id,
                registry: account_id,
                admin: account_id,
                validator: account_id,
                pool_id: None,
                staked: 0,
                unbonding: 0,
                creation_bond: 0,
            }
        }

        #[ink(constructor, payable)]
        pub fn new(
            vault: AccountId,
            admin: AccountId,
            validator: AccountId,
            creation_bond: u128,
            existential_deposit: u128,
        ) -> Self {
            let account_id = Self::env().account_id();

            if Self::env().transferred_value() != creation_bond + existential_deposit {
                panic!("Insufficient transferred value");
            }

            let nomination_agent = NominationAgent {
                vault,
                registry: Self::env().caller(),
                admin,
                validator,
                pool_id: None,
                staked: 0,
                unbonding: 0,
                creation_bond,
            };

            // Create nomination pool
            nomination_agent.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::Create {
                        amount: creation_bond,
                        root: MultiAddress::Id(account_id),
                        nominator: MultiAddress::Id(account_id),
                        bouncer: MultiAddress::Id(account_id),
                    }
                )).unwrap();

            nomination_agent
        }
    }

    impl INominationAgent for NominationAgent {
        #[ink(message, selector = 0)]
        fn initialize(&mut self, pool_id: u32) -> Result<(), RuntimeError> {
            // Restricted to registry
            if Self::env().caller() != self.registry {
                return Err(RuntimeError::Unauthorized);
            }

            // Can only initialize once
            if self.pool_id.is_some() {
                return Err(RuntimeError::Initialized);
            }

            // Ensures agent has all roles (Root, Nominator, Bouncer)
            let account_id = Self::env().account_id();
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::UpdateRoles {
                        pool_id,
                        new_root: ConfigOp::Set(account_id),
                        new_nominator: ConfigOp::Set(account_id),
                        new_bouncer: ConfigOp::Set(account_id),
                    }
                ))?;

            // Disallow others to join nomination pool
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::SetState {
                        pool_id,
                        state: PoolState::Blocked,
                    }
                ))?;

            // Nominate to validator
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::Nominate {
                        pool_id,
                        validators: [self.validator].to_vec(),
                    }
                ))?;

            self.pool_id = Some(pool_id);

            Ok(())
        }

        #[ink(message, payable, selector = 1)]
        fn deposit(&mut self) -> Result<(), RuntimeError> {
            let deposit_amount = Self::env().transferred_value();

            // Restricted to vault
            if Self::env().caller() != self.vault {
                return Err(RuntimeError::Unauthorized);
            }

            self.staked += deposit_amount;

            // Bond extra AZERO to nomination pool
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::BondExtra {
                        extra: BondExtra::FreeBalance {
                            balance: deposit_amount,
                        }
                    }
                ))?;

            Ok(())
        }

        #[ink(message, selector = 2)]
        fn start_unbond(&mut self, amount: u128) -> Result<(), RuntimeError> {
            // Restricted to vault
            if Self::env().caller() != self.vault {
                return Err(RuntimeError::Unauthorized);
            }

            self.staked -= amount;
            self.unbonding += amount;

            // Trigger un-bonding process
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::Unbond {
                        member_account: MultiAddress::Id(Self::env().account_id()),
                        unbonding_points: amount,
                    }
                ))?;

            Ok(())
        }

        #[ink(message, selector = 3)]
        fn withdraw_unbonded(&mut self) -> Result<(), RuntimeError> {
            let vault = self.vault; // shadow

            // Restricted to vault
            if Self::env().caller() != vault {
                return Err(RuntimeError::Unauthorized);
            }

            let balance_before = Self::env().balance();

            if let Err(e) = self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::WithdrawUnbonded {
                        member_account: MultiAddress::Id(Self::env().account_id()),
                        num_slashing_spans: 0,
                    }
                )) {
                ink::env::debug_println!("Ignoring NominationCall::WithdrawUnbonded error {:?}", e);
                return Ok(());
            };

            let withdrawn = Self::env().balance() - balance_before;

            // Transfer withdrawn AZERO to vault
            if withdrawn > 0 {
                self.unbonding -= withdrawn;
                Self::env().transfer(vault, withdrawn)?;
            }

            Ok(())
        }

        #[ink(message, selector = 4)]
        fn compound(&mut self, incentive_percentage: u16) -> Result<(Balance, Balance), RuntimeError> {
            let vault = self.vault; // shadow

            // Restricted to vault
            if Self::env().caller() != vault {
                return Err(RuntimeError::Unauthorized);
            }

            // Claim available AZERO
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::ClaimPayout {}
                ))
                .ok();

            // Contract balance might also contain rewards claimed via `BondExtra`
            let rewards = Self::env().balance();

            // Gracefully return when nomination pool had nothing to claim
            if rewards == 0 {
                return Ok((0, 0));
            }

            let incentive = rewards * incentive_percentage as u128 / BIPS;
            let compound_amount = rewards - incentive;

            // Bond AZERO to nomination pool
            if compound_amount > 0 {
                self.staked += compound_amount;
                self.env()
                    .call_runtime(&RuntimeCall::NominationPools(
                        NominationCall::BondExtra {
                            extra: BondExtra::FreeBalance {
                                balance: compound_amount,
                            }
                        }
                    ))
                    .ok();
            }

            // Send incentive AZERO to vault which will handle distribution to caller
            if incentive > 0 {
                Self::env().transfer(vault, incentive)?;
            }

            Ok((compound_amount, incentive))
        }

        #[ink(message, selector = 12)]
        fn get_staked_value(&self) -> Balance {
            self.staked
        }

        #[ink(message, selector = 13)]
        fn get_unbonding_value(&self) -> Balance {
            self.unbonding
        }

        #[ink(message)]
        fn get_vault(&self) -> AccountId {
            self.vault
        }

        #[ink(message)]
        fn get_admin(&self) -> AccountId {
            self.admin
        }

        #[ink(message)]
        fn get_validator(&self) -> AccountId {
            self.validator
        }

        #[ink(message)]
        fn get_pool_id(&self) -> u32 {
            self.pool_id.unwrap()
        }

        /// Step 1 of 2 in finalizing the nomination pool's lifecycle
        /// Performs the following actions:
        ///     1) Puts the pool in a Destroying state
        ///     2) Removes the validator nomination
        ///     3) Begins unbonding the initial bond
        ///
        /// Can only be called by registry
        /// Must have no protocol funds staked
        /// Must have no protocol funds unbonding
        #[ink(message, selector = 100)]
        fn destroy(&mut self) -> Result<(), RuntimeError> {
            // Restricted to registry
            if Self::env().caller() != self.registry {
                return Err(RuntimeError::Unauthorized);
            }

            if self.staked > 0 || self.unbonding > 0 {
                return Err(RuntimeError::Active);
            }

            if self.pool_id.is_none() {
                return Err(RuntimeError::NotInitialized);
            }

            let pool_id = self.pool_id.unwrap(); // shadow

            // Begin pool destruction
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::SetState {
                        pool_id,
                        state: PoolState::Destroying,
                    }
                ))?;

            // Chill
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::Chill {
                        pool_id,
                    }
                ))?;

            // Unbond initial nomination pool bond
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::Unbond {
                        member_account: MultiAddress::Id(Self::env().account_id()),
                        unbonding_points: self.creation_bond,
                    }
                ))?;

            self.creation_bond = 0;

            Ok(())
        }

        /// Step 2 of 2 in finalizing the nomination pool's lifecycle
        /// Performs the following actions:
        ///     1) Withdraws the (now unbonded) initial bond
        ///     2) Transfers the initial bond to any account of choice
        ///
        /// Can only be called by admin
        /// Must be called after `destroy()`
        #[ink(message, selector = 101)]
        fn admin_withdraw_bond(&mut self, to: AccountId) -> Result<(), RuntimeError> {
            // Restricted to admin
            if Self::env().caller() != self.admin {
                return Err(RuntimeError::Unauthorized);
            }

            if self.creation_bond > 0 {
                return Err(RuntimeError::Active);
            }

            // Trigger un-bonding process
            self.env()
                .call_runtime(&RuntimeCall::NominationPools(
                    NominationCall::WithdrawUnbonded {
                        member_account: MultiAddress::Id(Self::env().account_id()),
                        num_slashing_spans: 0,
                    }
                ))
                .ok();

            Self::env().transfer(to, Self::env().balance())?;

            Ok(())
        }
    }
}
