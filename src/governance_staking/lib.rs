#![cfg_attr(not(feature = "std"), no_std, no_main)]
mod traits;
pub use crate::staking::StakingRef;
pub use crate::traits::Staking;
#[ink::contract]
mod staking{
    
   
    use ink::reflect::ContractEventBase;
    use ink::ToAccountId;
    use ink::{contract_ref, env::call};
    use ink::{
        codegen::EmitEvent,
        env::{debug_println, DefaultEnvironment},
        prelude::{string::String, vec::Vec},
        storage::Mapping,
    };
    use psp22::{PSP22Error, PSP22};
    use psp34::{Id, PSP34Error, PSP34};

    use governance_nft::GovernanceNFTRef;

   

    pub const DAY: u64 = 86400 * 1000;
    pub const WITHDRAW_DELAY: u64 = 14 * DAY;
    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum StakingError {
        Invalid,
        Unauthorized,
        InvalidTimeWindow,
        NFTError(PSP34Error),
        TokenError(PSP22Error),
    }
    #[ink(storage)]
  
    pub struct Staking {
        creation_time: u64,
        governor:AccountId,
        reward_token_balance: u128,
        staked_token_balance: u128,
        rewards_per_second: u128,
        reward_stake_accumulation: u128,
        accumulated_rewards:u128,
        lst_accumulation_update: u64,
        owner: AccountId,
        governance_token: AccountId,
        nft: GovernanceNFTRef,
        governance_nfts: Mapping<AccountId, Vec<u128>>,
        unstake_requests: Mapping<u128, UnstakeRequest>,
        last_reward_claim:Mapping<u128,u64>
    }
   
    #[derive(Debug, PartialEq, Eq, Clone, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    struct UnstakeRequest {
        pub time: u64,
        pub token_value: u128,
        pub owner:AccountId
    }
    #[ink(event)]
    pub struct TokensWrapped{
        staker: AccountId,
        amount:u128,
        nft:u128
    }
    #[ink(event)]
    pub struct StakeAdded{
        staker: AccountId,
        amount:u128,
        nft:u128
    }
    #[ink(event)]
    pub struct StakeRemoved{
        staker: AccountId,
        amount:u128,
        nft:u128
    }
    #[ink(event)]
    pub struct UnwrapRequestCreated{
        staker: AccountId,        
        nft:u128
    }
    type Event = <Staking as ContractEventBase>::Type;
    impl Staking {
        fn emit_event<EE>(emitter: EE, event: Event)
        where
            EE: EmitEvent<Staking>,
        {
            emitter.emit_event(event);
        }
        fn transfer_psp22_from(
            &self,
            from: &AccountId,
            to: &AccountId,
            amount: Balance,
        ) -> Result<(), StakingError> {
            let mut token: contract_ref!(PSP22) = self.governance_token.into();
            if let Err(e) = token.transfer_from(*from, *to, amount, Vec::new()) {
                return Err(StakingError::TokenError(e));
            }
            Ok(())
        }
        fn burn_psp34(&mut self, from: AccountId, id: u128) -> Result<(), StakingError> {
            if let Err(e) = self.nft.burn(from, id) {
                return Err(StakingError::NFTError(e));
            }
            Ok(())
        }
        fn mint_psp34(&mut self, to: AccountId, weight: u128) -> Result<u128, StakingError> {
            let result =self.nft.mint(to, weight); 
            match result{
                Err(e)=>return Err(StakingError::NFTError(e)),
                Ok(r)=>Ok(r)
            }
            
        }
        fn update_stake_accumulation(&mut self, curr_time: u64) -> Result<(), StakingError> {
            self.accumulated_rewards+=((curr_time - self.lst_accumulation_update) as u128)*self.rewards_per_second;
            self.reward_stake_accumulation +=
                self.staked_token_balance*((curr_time - self.lst_accumulation_update) as u128);
            self.lst_accumulation_update=curr_time;
            Ok(())
        }
        fn calculate_reward_share(&mut self,curr_time: u64,last_update:u64,stake_balance:u128)-> u128{
            debug_println!("{}{}",curr_time," CURRTIME");
            debug_println!("{}{}",last_update," UPDATE");
            debug_println!("{}{}",stake_balance," STAKE");
            debug_println!("{}{}",self.accumulated_rewards," ACCUMULATED");
            debug_println!("{}{}",self.reward_stake_accumulation," REWARD");
            let user_stake_weight=stake_balance*((curr_time-last_update) as u128);
            (self.accumulated_rewards*user_stake_weight)/self.reward_stake_accumulation
            //0_u128
        }

        #[ink(constructor)]
        pub fn new(
            governance_token: AccountId,
            governor:AccountId,
            governance_nft:GovernanceNFTRef,
            interest_rate: u128,
        ) -> Self {
          
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();

            

            Self {
                creation_time: now,
                governor:governor,
                reward_token_balance: 0_u128,
                staked_token_balance: 0_u128,
                rewards_per_second: interest_rate,
                reward_stake_accumulation: 0,
                accumulated_rewards:0,
                lst_accumulation_update: now,
                owner: caller,
                governance_token: governance_token,
                nft: governance_nft,
                governance_nfts: Mapping::new(),
                unstake_requests: Mapping::new(),
                last_reward_claim:Mapping::new()
            }
        }
        #[ink(message)]
        pub fn get_governance_nft(&self) -> AccountId {
            GovernanceNFTRef::to_account_id(&self.nft)
        }
        #[ink(message)]
        pub fn update_rewards_rate(&mut self,new_rate:u128) ->   Result<(), StakingError> {
            let caller = Self::env().caller();
            if caller != self.governor{
                return Err(StakingError::Unauthorized)
            }
            let now = Self::env().block_timestamp();
            self.update_stake_accumulation(now)?;
            self.rewards_per_second=new_rate;
            Ok(())
        }
        #[ink(message)]
        pub fn wrap_tokens(
            &mut self,
            token_value: u128,
            to: Option<AccountId>,
        ) -> Result<(), StakingError> {
            debug_println!("ADDing Value {}",token_value);
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();
            self.transfer_psp22_from(&caller, &Self::env().account_id(), token_value)?;
            self.update_stake_accumulation(now)?;
            self.staked_token_balance+=token_value;
            
           
            let recipient:AccountId;
            if to.is_some() {
                recipient=to.unwrap();
            } else {
                recipient=caller;
            }
            let minted_nft=self.mint_psp34(recipient, token_value).unwrap();
            Self::emit_event(
                Self::env(),
                Event::TokensWrapped(TokensWrapped{
                    staker: caller,
                    amount:token_value,
                    nft:minted_nft
                }),
            );
            Ok(())
        }

        #[ink(message)]
        pub fn add_token_value(
            &mut self,
            token_value: u128,
            nft_id: u128,
        ) -> Result<(), StakingError> {
            debug_println!("ADDing Value {}",token_value);
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();
            self.transfer_psp22_from(&caller, &Self::env().account_id(), token_value)?;
            self.update_stake_accumulation(now)?;
            self.staked_token_balance+=token_value;
            
            if let Err(e) = self.nft.increment_weight(nft_id, token_value) {
                return Err(StakingError::NFTError(e));
            }
            Ok(())
        }
        /*#[ink(message)]
        pub fn remove_token_value(
            &mut self,
            token_value: u128,
            nft_id: u128,
        ) -> Result<(), StakingError> {
            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();
            if let Err(e) = self.nft.decrement_weight(nft_id, token_value) {
                return Err(StakingError::NFTError(e));
            }
            self.update_stake_accumulation(now)?;
            self.transfer_psp22_from(&Self::env().account_id(), &caller, token_value)?;
            
            self.staked_token_balance-=token_value;
            Ok(())
        }
        */
        #[ink(message)]
        pub fn claim_staking_rewards(&mut self,token_id: u128)->Result<(), StakingError> {
            let now = Self::env().block_timestamp();
            
            self.update_stake_accumulation(now)?;

            let data = self.nft.get_governance_data(token_id);
            let last_claim= self.last_reward_claim.get(token_id).unwrap_or(data.block_created);
            let reward= self.calculate_reward_share(now,last_claim,data.vote_weight);
            self.last_reward_claim.insert(token_id,&now);
            if let Err(e) = self.nft.increment_weight(token_id,reward) {
                return Err(StakingError::NFTError(e));
            }
            Ok(())
        }
        #[ink(message)]
        pub fn create_unwrap_request(&mut self, token_id: u128) -> Result<(), StakingError> {
            let now = Self::env().block_timestamp();
            let caller = Self::env().caller();
            let data = self.nft.get_governance_data(token_id);
            
            self.update_stake_accumulation(now);
             
            let last_claim= self.last_reward_claim.get(token_id).unwrap_or(data.block_created);
            let reward= self.calculate_reward_share(now,last_claim,data.vote_weight);
           
            self.staked_token_balance-= data.vote_weight;
            self.unstake_requests
               .insert(token_id, &UnstakeRequest{time:now, token_value:data.vote_weight+reward,owner:caller});
            self.burn_psp34(caller, token_id)?;
            Ok(())
        }
        #[ink(message)]
        pub fn complete_request(&mut self, token_id: u128) -> Result<(), StakingError> {
            let now = Self::env().block_timestamp();
            let caller = Self::env().caller();
            let data=  self.unstake_requests.get(token_id).unwrap();
            if now< data.time + WITHDRAW_DELAY{
                return Err(StakingError::InvalidTimeWindow);
            }
            if data.owner!= caller {
                return Err(StakingError::Unauthorized);
            }
            self.transfer_psp22_from(&Self::env().account_id(), &caller, data.token_value)?;
            self.unstake_requests.remove(token_id);
            Ok(())
        }
    }
}