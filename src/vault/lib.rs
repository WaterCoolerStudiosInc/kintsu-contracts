#![cfg_attr(not(feature = "std"), no_std, no_main)]

mod data;
mod nomination_agent_utils;

#[ink::contract]
mod vault {
    use crate::data::*;

    use ink::{
        codegen::EmitEvent,
        contract_ref,
        env::Error as InkEnvError,
        prelude::{format, string::String, vec::Vec},
        reflect::ContractEventBase,
        ToAccountId,
    };
    use psp22::{PSP22Burnable, PSP22};
    use registry::RegistryRef;
    use share_token::{ShareToken, TokenRef};

    /// Errors returned by the contract's methods.
    impl From<InkEnvError> for VaultError {
        fn from(e: InkEnvError) -> Self {
            VaultError::InkEnvError(format!("{:?}", e))
        }
    }

    /// Alias for wrapper around all events in this contract generated by ink!.
    type Event = <Vault as ContractEventBase>::Type;

    #[ink(event)]
    pub struct Staked {
        #[ink(topic)]
        staker: AccountId,
        azero: Balance,
        new_shares: Balance,
        virtual_shares: Balance,
    }
    #[ink(event)]
    pub struct Compounded {
        caller: AccountId,
        azero: Balance,
        incentive: Balance,
        virtual_shares: Balance,
    }
    #[ink(event)]
    pub struct UnlockRequested {
        #[ink(topic)]
        staker: AccountId,
        shares: Balance,
        unlock_id: u128,
        batch_id: u64,
    }
    #[ink(event)]
    pub struct UnlockCanceled {
        #[ink(topic)]
        staker: AccountId,
        shares: Balance,
        batch_id: u64,
        unlock_id: u128,
    }
    #[ink(event)]
    pub struct BatchUnlockSent {
        #[ink(topic)]
        batch_id: u64,
        shares: Balance,
        virtual_shares: Balance,
        spot_value: Balance,
    }
    #[ink(event)]
    pub struct UnlockRedeemed {
        #[ink(topic)]
        staker: AccountId,
        azero: Balance,
        batch_id: u64,
        unlock_id: u64,
    }
    #[ink(event)]
    pub struct FeesWithdrawn {
        shares: Balance,
    }
    #[ink(event)]
    pub struct FeesAdjusted {
        new_fee: u16,
        virtual_shares: Balance,
    }
    #[ink(event)]
    pub struct IncentiveAdjusted {
        new_incentive: u16,
    }
    #[ink(event)]
    pub struct MinimumStakeAdjusted {
        new_minimum_stake: Balance,
    }
    #[ink(event)]
    pub struct OwnershipTransferred {
        new_account: AccountId,
    }
    #[ink(event)]
    pub struct RoleSetFeesTransferred {
        new_account: AccountId,
    }
    #[ink(event)]
    pub struct RoleSetFeesAdminTransferred {
        new_account: AccountId,
    }

    #[ink(storage)]
    pub struct Vault {
        pub data: VaultData,
    }

    impl Vault {
        fn emit_event<EE>(emitter: EE, event: Event)
        where
            EE: EmitEvent<Vault>,
        {
            emitter.emit_event(event);
        }

        fn transfer_shares_from(
            &self,
            from: &AccountId,
            to: &AccountId,
            amount: Balance,
        ) -> Result<(), VaultError> {
            let mut token: contract_ref!(PSP22) = self.data.shares_contract.into();
            if let Err(e) = token.transfer_from(*from, *to, amount, Vec::new()) {
                return Err(VaultError::TokenError(e));
            }
            Ok(())
        }

        fn transfer_shares_to(&self, to: &AccountId, amount: &Balance) -> Result<(), VaultError> {
            let mut token: contract_ref!(PSP22) = self.data.shares_contract.into();
            if let Err(e) = token.transfer(*to, *amount, Vec::new()) {
                return Err(VaultError::TokenError(e));
            }
            Ok(())
        }

        fn mint_shares(&mut self, amount: Balance, to: AccountId) -> Result<(), VaultError> {
            let mut token: contract_ref!(ShareToken) = self.data.shares_contract.into();
            self.data.total_shares_minted += amount;
            if let Err(e) = token.mint(to, amount) {
                return Err(VaultError::TokenError(e));
            }
            Ok(())
        }

        fn burn_shares(&mut self, amount: Balance) -> Result<(), VaultError> {
            let mut token: contract_ref!(PSP22Burnable) = self.data.shares_contract.into();
            self.data.total_shares_minted -= amount;
            if let Err(e) = token.burn(amount) {
                return Err(VaultError::TokenError(e));
            }
            Ok(())
        }
    }

    impl Vault {
        #[ink(constructor)]
        pub fn new(
            share_token_hash: Hash,
            registry_code_hash: Hash,
        ) -> Self {
            Self::custom_era(share_token_hash, registry_code_hash, DAY)
        }

        #[ink(constructor)]
        pub fn custom_era(
            share_token_hash: Hash,
            registry_code_hash: Hash,
            era: u64,
        ) -> Self {
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();

            let registry_ref = RegistryRef::new(caller, caller, caller)
                .endowment(0)
                .code_hash(registry_code_hash)
                .salt_bytes(
                    &[9_u8.to_le_bytes().as_ref(), caller.as_ref()].concat()[..4],
                )
                .instantiate();
            let share_token_ref = TokenRef::new(Some(String::from("sAZERO")), Some(String::from("SAZ")))
                .endowment(0)
                .code_hash(share_token_hash)
                .salt_bytes(
                    &[7_u8.to_le_bytes().as_ref(), caller.as_ref()].concat()[..4],
                )
                .instantiate();

            Self {
                data: VaultData::new(
                    caller,
                    TokenRef::to_account_id(&share_token_ref),
                    registry_ref,
                    now,
                    era,
                ),
            }
        }

        /// Allow users to convert AZERO into sAZERO
        /// Mints the caller sAZERO based on the redemption ratio
        ///
        /// Minimum AZERO amount is required to stake
        /// AZERO must be transferred via transferred_value
        #[ink(message, payable)]
        pub fn stake(&mut self) -> Result<Balance, VaultError> {
            let caller = Self::env().caller();
            let azero = Self::env().transferred_value();

            // Verify minimum AZERO is being staked
            if azero < self.data.minimum_stake {
                return Err(VaultError::MinimumStake);
            }

            // Update fees before calculating redemption ratio and minting shares
            self.data.update_fees(Self::env().block_timestamp());

            // Handle sAZERO
            let new_shares = self.get_shares_from_azero(azero);
            self.mint_shares(new_shares, caller)?;

            // Handle AZERO
            self.data.delegate_bonding(azero)?;

            Self::emit_event(
                Self::env(),
                Event::Staked(Staked {
                    staker: caller,
                    azero,
                    new_shares,
                    virtual_shares: self.data.total_shares_virtual, // updated in update_fees()
                }),
            );

            Ok(new_shares)
        }

        /// Allow user to begin the unlock process
        /// Transfers sAZERO specified in `shares` argument to the vault contract
        /// Unlock is batched into current two era batch request
        ///
        /// Caller must approve the psp22 token contract beforehand
        #[ink(message)]
        pub fn request_unlock(&mut self, shares: Balance) -> Result<(), VaultError> {
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();

            self.transfer_shares_from(&caller, &Self::env().account_id(), shares)?;

            let current_batch_unlock_id = self.data.get_batch_unlock_id(now);
            let current_batch_unlock_shares = self
                .data
                .batch_unlock_requests
                .get(current_batch_unlock_id)
                .map(|b| b.total_shares)
                .unwrap_or(0);

            // Update current batch unlock request
            self.data.batch_unlock_requests.insert(
                current_batch_unlock_id,
                &UnlockRequestBatch {
                    total_shares: current_batch_unlock_shares + shares,
                    value_at_redemption: None,
                    redemption_timestamp: None,
                },
            );

            // Update user's unlock requests
            let mut user_unlock_requests = self.data.user_unlock_requests.get(caller).unwrap_or(Vec::new());
            user_unlock_requests.push(UnlockRequest {
                creation_time: now,
                share_amount: shares,
                batch_id: current_batch_unlock_id,
            });
            self.data.user_unlock_requests.insert(caller, &user_unlock_requests);
            let unlock_id=(user_unlock_requests.len()-1) as u128;
            Self::emit_event(
                Self::env(),
                Event::UnlockRequested(UnlockRequested {
                    staker: caller,
                    shares,
                    unlock_id,
                    batch_id: current_batch_unlock_id,

                }),
            );

            Ok(())
        }

        /// Allow user to cancel their unlock request
        ///
        /// Must be done in the same batch interval in which the request was originally sent
        #[ink(message)]
        pub fn cancel_unlock_request(&mut self, user_unlock_id: u128) -> Result<(), VaultError> {
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();

            let current_batch_unlock_id = self.data.get_batch_unlock_id(now);
            let mut user_unlock_requests = self.data.user_unlock_requests.get(caller).unwrap_or(Vec::new());

            if user_unlock_id >= user_unlock_requests.len() as u128 {
                return Err(VaultError::InvalidUserUnlockRequest);
            }

            if current_batch_unlock_id != user_unlock_requests[user_unlock_id as usize].batch_id {
                return Err(VaultError::InvalidBatchUnlockRequest);
            }

            let share_amount = user_unlock_requests[user_unlock_id as usize].share_amount.clone();

            // Delete user's cancelled unlock request
            user_unlock_requests.remove(user_unlock_id as usize);
            self.data.user_unlock_requests.insert(caller, &user_unlock_requests);

            // Remove shares from current batch unlock request
            let mut current_batch = self.data.batch_unlock_requests.get(current_batch_unlock_id).unwrap();
            current_batch.total_shares -= share_amount;
            self.data.batch_unlock_requests.insert(current_batch_unlock_id, &current_batch);

            // Return shares to caller
            self.transfer_shares_to(&caller, &share_amount)?;

            Self::emit_event(
                Self::env(),
                Event::UnlockCanceled(UnlockCanceled {
                    staker: caller,
                    shares: share_amount,
                    unlock_id: user_unlock_id,
                    batch_id: current_batch_unlock_id,
                }),
            );

            Ok(())
        }

        /// Trigger unlock requests of previous batched requests
        /// Distributes unlock requests to nominators according to current stake imbalances
        /// Calculates a batch spot values for sAZERO in the batches
        /// Burns associated sAZERO
        ///
        /// Cannot be called for a batch that has not concluded
        /// Cannot be called for a batch that has already been redeemed
        /// Batch IDs must be specified in ascending order (for gas efficient duplicate check)
        #[ink(message)]
        pub fn send_batch_unlock_requests(&mut self, batch_ids: Vec<u64>) -> Result<(), VaultError> {
            let now = Self::env().block_timestamp();

            let current_batch_unlock_id = self.data.get_batch_unlock_id(now);

            // Validate batch_ids
            for (i, &batch_id) in batch_ids.iter().enumerate() {
                // Cannot send current batch unlock request
                if batch_id >= current_batch_unlock_id {
                    return Err(VaultError::InvalidBatchUnlockRequest);
                }
                // Cannot send duplicate batch id (requires `batch_ids` is sorted in asc order)
                if i > 0 && batch_id <= batch_ids[i-1] {
                    return Err(VaultError::Duplication);
                }
            }

            let batches: Vec<UnlockRequestBatch> = batch_ids
                .iter()
                .map(|batch_id| self.data.batch_unlock_requests.get(batch_id).unwrap())
                .collect();

            // Cannot re-send batch unlock request
            if batches.iter().any(|batch| batch.redemption_timestamp.is_some()) {
                return Err(VaultError::InvalidBatchUnlockRequest);
            }

            // Update fees before calculating redemption ratio and burning shares
            self.data.update_fees(now);

            let total_shares_virtual_ = self.data.total_shares_virtual; // shadow
            let mut aggregate_batch_spot_value: Balance = 0;
            let mut aggregate_total_shares: Balance = 0;

            for (batch_id, mut batch) in batch_ids.into_iter().zip(batches.into_iter()) {
                let batch_spot_value = self.get_azero_from_shares(batch.total_shares);

                aggregate_batch_spot_value += batch_spot_value;
                aggregate_total_shares += batch.total_shares;

                // Update batch request
                batch.value_at_redemption = Some(batch_spot_value);
                batch.redemption_timestamp = Some(now);
                self.data.batch_unlock_requests.insert(batch_id, &batch);

                // Optimistically emit events
                Self::emit_event(
                    Self::env(),
                    Event::BatchUnlockSent(BatchUnlockSent {
                        shares: batch.total_shares,
                        virtual_shares: total_shares_virtual_,
                        spot_value: batch_spot_value,
                        batch_id,
                    }),
                );
            }

            // Allocate unlock quantity across nomination pools
            self.data.delegate_unbonding(aggregate_batch_spot_value)?;

            self.burn_shares(aggregate_total_shares)?;

            Ok(())
        }


        /// Attempts to claim unbonded AZERO from all validators
        #[ink(message)]
        pub fn delegate_withdraw_unbonded(&mut self) -> Result<(), VaultError> {
            self.data.delegate_withdraw_unbonded()?;

            Ok(())
        }

        /// Allows a user to withdraw staked AZERO
        ///
        /// Returns original deposit amount plus interest to depositor address
        /// Queries the redeemable amount by user AccountId and Claim Vector index
        /// Associated batch unlock request must have been completed
        /// Deletes the user's unlock request
        /// Burns the associated sAZERO tokens
        #[ink(message)]
        pub fn redeem(&mut self, user: AccountId, unlock_id: u64) -> Result<(), VaultError> {
            let now = Self::env().block_timestamp();

            let mut user_unlock_requests = self.data.user_unlock_requests.get(user).unwrap();

            // Ensure user specified a valid unlock request index
            if unlock_id >= user_unlock_requests.len() as u64 {
                return Err(VaultError::InvalidUserUnlockRequest);
            }

            let batch_id = user_unlock_requests[unlock_id as usize].batch_id;
            let share_amount = user_unlock_requests[unlock_id as usize].share_amount;

            // Ensure batch unlock has been redeemed
            let batch_unlock_request = self.data.batch_unlock_requests.get(batch_id).unwrap();
            if batch_unlock_request.redemption_timestamp.is_none() || batch_unlock_request.value_at_redemption.is_none() {
                return Err(VaultError::InvalidBatchUnlockRequest);
            }

            // Ensure batch unlock has completed
            let time_since_redemption = now - batch_unlock_request.redemption_timestamp.unwrap();
            if time_since_redemption < self.data.cooldown_period {
                return Err(VaultError::CooldownPeriod);
            }

            // Delete completed user unlock request
            user_unlock_requests.remove(unlock_id as usize);
            self.data.user_unlock_requests.insert(user, &user_unlock_requests);

            // Send AZERO to user
            let azero = self.data.pro_rata(
                share_amount,
                batch_unlock_request.value_at_redemption.unwrap(),
                batch_unlock_request.total_shares,
            );
            Self::env().transfer(user, azero)?;

            Self::emit_event(
                Self::env(),
                Event::UnlockRedeemed(UnlockRedeemed {
                    staker: user,
                    azero,
                    unlock_id,
                    batch_id,
                }),
            );

            Ok(())
        }

        /// Alternative method for a user to withdraw staked AZERO
        ///
        /// This should be called instead of `redeem()` when insufficient AZERO exists in the Vault and
        /// validator(s) have unbonded AZERO which can be claimed
        #[ink(message)]
        pub fn redeem_with_withdraw(&mut self, user: AccountId, unlock_id: u64) -> Result<(), VaultError> {
            // Claim all unbonded AZERO into Vault
            self.data.delegate_withdraw_unbonded()?;

            self.redeem(user, unlock_id)?;

            Ok(())
        }

        /// Compound earned interest for all validators
        ///
        /// Can be called by anyone
        /// Caller receives an AZERO incentive based on the total AZERO amount compounded
        #[ink(message)]
        pub fn compound(&mut self) -> Result<Balance, VaultError> {
            let caller = Self::env().caller();

            // Delegate compounding to all nominator pools
            let (compounded, incentive) = self.data.delegate_compound()?;

            // Send AZERO incentive to caller
            if incentive > 0 {
                Self::env().transfer(caller, incentive)?;
            }

            Self::emit_event(
                Self::env(),
                Event::Compounded(Compounded {
                    caller,
                    azero: compounded,
                    incentive,
                    virtual_shares: self.get_current_virtual_shares(),
                }),
            );

            Ok(incentive)
        }

        /// =========================== Restricted Functions: Owner Role ===========================

        /// Claim fees by inflating sAZERO supply
        ///
        /// Caller must have the owner role (`role_owner`)
        /// Mints virtual shares as sAZERO to the owner
        /// Effectively serves as a compounding for protocol fee
        /// sets total_shares_virtual to 0
        #[ink(message)]
        pub fn withdraw_fees(&mut self) -> Result<(), VaultError> {
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();

            if caller != self.data.role_owner {
                return Err(VaultError::InvalidPermissions);
            }

            self.data.update_fees(now);

            let shares = self.data.total_shares_virtual;
            self.mint_shares(shares, self.data.role_owner)?;
            self.data.total_shares_virtual = 0;

            Self::emit_event(
                Self::env(),
                Event::FeesWithdrawn(FeesWithdrawn {
                    shares,
                }),
            );

            Ok(())
        }

        /// Update the minimum stake amount
        ///
        /// Caller must have the owner role (`role_owner`)
        #[ink(message)]
        pub fn adjust_minimum_stake(&mut self, new_minimum_stake: Balance) -> Result<(), VaultError> {
            let caller = Self::env().caller();

            if caller != self.data.role_owner {
                return Err(VaultError::InvalidPermissions);
            }
            if self.data.minimum_stake == new_minimum_stake {
                return Err(VaultError::NoChange);
            }

            self.data.minimum_stake = new_minimum_stake;

            Self::emit_event(
                Self::env(),
                Event::MinimumStakeAdjusted(MinimumStakeAdjusted {
                    new_minimum_stake,
                }),
            );

            Ok(())
        }

        /// Upgrade the contract by the ink env set_code_hash function
        ///
        /// Caller must have the owner role (`role_owner`)
        /// See ink documentation for details https://paritytech.github.io/ink/ink_env/fn.set_code_hash.html
        #[ink(message)]
        pub fn set_code(&mut self, code_hash: [u8; 32]) -> Result<(), VaultError> {
            let caller = Self::env().caller();

            if caller != self.data.role_owner {
                return Err(VaultError::InvalidPermissions);
            }

            ink::env::set_code_hash(&code_hash)?;

            Ok(())
        }

        /// Transfers ownership to a new account
        ///
        /// Caller must have the owner role (`role_owner`)
        #[ink(message)]
        pub fn transfer_role_owner(&mut self, new_account: AccountId) -> Result<(), VaultError> {
            let caller = Self::env().caller();

            if caller != self.data.role_owner {
                return Err(VaultError::InvalidPermissions);
            }
            if self.data.role_owner == new_account {
                return Err(VaultError::NoChange);
            }

            self.data.role_owner = new_account;

            Self::emit_event(
                Self::env(),
                Event::OwnershipTransferred(OwnershipTransferred {
                    new_account,
                }),
            );

            Ok(())
        }

        /// ======================== Restricted Functions: Adjust Fee Role ========================

        /// Update the protocol fee
        ///
        /// Caller must have the adjust fee role (`role_adjust_fee`)
        /// Updates the total_shares_virtual accumulator at the old fee level first
        #[ink(message)]
        pub fn adjust_fee(&mut self, new_fee: u16) -> Result<(), VaultError> {
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();

            if caller != self.data.role_adjust_fee {
                return Err(VaultError::InvalidPermissions);
            }
            if self.data.fee_percentage == new_fee {
                return Err(VaultError::NoChange);
            }
            if new_fee >= BIPS {
                return Err(VaultError::InvalidPercent);
            }

            self.data.update_fees(now);
            self.data.fee_percentage = new_fee;

            Self::emit_event(
                Self::env(),
                Event::FeesAdjusted(FeesAdjusted {
                    new_fee,
                    virtual_shares: self.data.total_shares_virtual, // updated in update_fees()
                }),
            );

            Ok(())
        }

        /// Update the compound incentive
        ///
        /// Caller must have the adjust fee role (`role_adjust_fee`)
        #[ink(message)]
        pub fn adjust_incentive(&mut self, new_incentive: u16) -> Result<(), VaultError> {
            let caller = Self::env().caller();

            if caller != self.data.role_adjust_fee {
                return Err(VaultError::InvalidPermissions);
            }
            if self.data.incentive_percentage == new_incentive {
                return Err(VaultError::NoChange);
            }
            if new_incentive >= BIPS {
                return Err(VaultError::InvalidPercent);
            }

            self.data.incentive_percentage = new_incentive;

            Self::emit_event(
                Self::env(),
                Event::IncentiveAdjusted(IncentiveAdjusted {
                    new_incentive,
                }),
            );

            Ok(())
        }

        /// Transfers adjust fee role to a new account
        ///
        /// Caller must be the admin for the adjust fee role (`role_adjust_fee_admin`)
        #[ink(message)]
        pub fn transfer_role_adjust_fee(&mut self, new_account: AccountId) -> Result<(), VaultError> {
            let caller = Self::env().caller();

            if caller != self.data.role_adjust_fee_admin {
                return Err(VaultError::InvalidPermissions);
            }
            if self.data.role_adjust_fee == new_account {
                return Err(VaultError::NoChange);
            }

            self.data.role_adjust_fee = new_account;

            Self::emit_event(
                Self::env(),
                Event::RoleSetFeesTransferred(RoleSetFeesTransferred {
                    new_account,
                }),
            );

            Ok(())
        }

        /// Transfers administration of adjust fee role to a new account
        ///
        /// Caller must be the admin for the adjust fee role (`role_adjust_fee_admin`)
        #[ink(message)]
        pub fn transfer_role_adjust_fee_admin(&mut self, new_account: AccountId) -> Result<(), VaultError> {
            let caller = Self::env().caller();

            if caller != self.data.role_adjust_fee_admin {
                return Err(VaultError::InvalidPermissions);
            }
            if self.data.role_adjust_fee_admin == new_account {
                return Err(VaultError::NoChange);
            }

            self.data.role_adjust_fee_admin = new_account;

            Self::emit_event(
                Self::env(),
                Event::RoleSetFeesAdminTransferred(RoleSetFeesAdminTransferred {
                    new_account,
                }),
            );

            Ok(())
        }

        /// ================================= Non Mutable Queries =================================

        #[ink(message)]
        pub fn get_batch_id(&self) -> u64 {
            self.data.get_batch_unlock_id(Self::env().block_timestamp())
        }

        #[ink(message)]
        pub fn get_creation_time(&self) -> u64 {
            self.data.creation_time
        }

        #[ink(message)]
        pub fn get_role_owner(&self) -> AccountId {
            self.data.role_owner
        }

        #[ink(message)]
        pub fn get_role_adjust_fee(&self) -> AccountId {
            self.data.role_adjust_fee
        }

        #[ink(message)]
        pub fn get_role_adjust_fee_admin(&self) -> AccountId {
            self.data.role_adjust_fee_admin
        }

        /// Returns the total amount of bonded AZERO
        #[ink(message)]
        pub fn get_total_pooled(&self) -> Balance {
            self.data.total_pooled
        }

        /// Returns the shares effectively in circulation by the protocol including:
        ///     1) sAZERO that has already been minted
        ///     2) sAZERO that could be minted (virtual) representing accumulating protocol fees
        #[ink(message)]
        pub fn get_total_shares(&self) -> Balance {
            self.data.total_shares_minted + self.get_current_virtual_shares()
        }

        /// Returns the protocol fees (sAZERO) which can be minted and withdrawn at the current block timestamp
        #[ink(message)]
        pub fn get_current_virtual_shares(&self) -> Balance {
            let now = Self::env().block_timestamp();
            self.data.get_virtual_shares_at_time(now)
        }

        #[ink(message)]
        pub fn get_minimum_stake(&self) -> Balance {
            self.data.minimum_stake
        }

        #[ink(message)]
        pub fn get_fee_percentage(&self) -> u16 {
            self.data.fee_percentage
        }

        #[ink(message)]
        pub fn get_incentive_percentage(&self) -> u16 {
            self.data.incentive_percentage
        }
        
        #[ink(message)]
        pub fn get_share_token_contract(&self) -> AccountId {
            self.data.shares_contract
        }

        #[ink(message)]
        pub fn get_registry_contract(&self) -> AccountId {
            RegistryRef::to_account_id(&self.data.registry_contract)
        }

        /// Calculate the value of AZERO in terms of sAZERO
        #[ink(message)]
        pub fn get_shares_from_azero(&self, azero: Balance) -> Balance {
            let total_pooled_ = self.data.total_pooled; // shadow
            if total_pooled_ == 0 {
                // This happens upon initial stake
                // Also known as 1:1 redemption ratio
                azero
            } else {
                self.data.pro_rata(azero, self.get_total_shares(), total_pooled_)
            }
        }

        /// Calculate the value of sAZERO in terms of AZERO
        #[ink(message)]
        pub fn get_azero_from_shares(&self, shares: Balance) -> Balance {
            let total_shares = self.get_total_shares();
            if total_shares == 0 {
                // This should never happen
                0
            } else {
                self.data.pro_rata(shares, self.data.total_pooled, total_shares)
            }
        }

        /// Returns the unlock requests for a given user
        #[ink(message)]
        pub fn get_unlock_requests(&self, user: AccountId) -> Vec<UnlockRequest> {
            self.data.user_unlock_requests.get(user).unwrap_or(Vec::new())
        }

        /// Returns the number of unlock requests made by a given user
        #[ink(message)]
        pub fn get_unlock_request_count(&self, user: AccountId) -> u128 {
            self.data.user_unlock_requests.get(user).unwrap_or(Vec::new()).len() as u128
        }

        /// Returns the information of a batch unlock request for the given batch id
        #[ink(message)]
        pub fn get_batch_unlock_requests(&self, batch_id: u64) -> (u128, Option<u128>, Option<Timestamp>) {
            let batch = self.data.batch_unlock_requests.get(batch_id).unwrap();
            (
                batch.total_shares,
                batch.value_at_redemption,
                batch.redemption_timestamp,
            )
        }

        #[ink(message)]
        pub fn get_weight_imbalances(&self, total_pooled: u128) -> (u128, u128, Vec<u128>, Vec<i128>) {
            let (total_weight, agents) = self.data.registry_contract.get_agents();
            self.data.get_weight_imbalances(&agents, total_weight, total_pooled)
        }
    }
}
