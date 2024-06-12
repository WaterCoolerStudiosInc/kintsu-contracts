#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
mod staking{
    use ink::{contract_ref, env::call};
    use psp22::{ PSP22,PSP22Error};
    use psp34::{PSP34,PSP34Error,Id};
    use ink::{
        env::{
            debug_println,
            DefaultEnvironment,
         
        },
        prelude::{string::String, vec::Vec},
      
        storage::Mapping,
    };
    
    use governance_nft::{GovernanceNFT,GovernanceNFTRef};
    
    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum StakingError {
        Invalid,    
        NFTError(PSP34Error),
        TokenError(PSP22Error),     
    }
    #[ink(storage)]
    pub struct Staking {
        owner:AccountId,
        governance_token:AccountId,
        nft:AccountId,
        governance_nfts:Mapping::<AccountId,Vec<u128>>
    }
    impl Staking {
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
        fn burn_psp34(
            &self,
            from: AccountId,
            id:u128
        ) -> Result<(), StakingError> {
            let mut token: contract_ref!(GovernanceNFT) = self.nft.into();
            if let Err(e) = token.burn(from,id) {
                return Err(StakingError::NFTError(e));
            }
            Ok(())
        }
        fn mint_psp34(
            &self,
            to: AccountId,
            weight:u128
        ) -> Result<(), StakingError> {
            let mut token: contract_ref!(GovernanceNFT) = self.nft.into();
            if let Err(e) = token.mint(to,weight) {
                return Err(StakingError::NFTError(e));
            }
            Ok(())
        }
        #[ink(constructor)]
        pub fn new(
            governance_token: AccountId,
            governance_nft_hash: Hash,
        ) -> Self {
            use ink::{storage::Mapping, ToAccountId};

            let caller = Self::env().caller();
            let now = Self::env().block_timestamp();

            let nft_ref = GovernanceNFTRef::new(Self::env().account_id())
                .endowment(0)
                .code_hash(governance_nft_hash)
                .salt_bytes(
                    &[9_u8.to_le_bytes().as_ref(), caller.as_ref()].concat()[..4],
                )
                .instantiate();
          

            Self {                
                    owner:caller,
                    governance_token:governance_token,
                    nft: GovernanceNFTRef::to_account_id(&nft_ref),
                    governance_nfts:Mapping::new()
                
            }

        }
        #[ink(message)]
        pub fn get_governance_nft(&self)->AccountId{
            self.nft
        }
       #[ink(message)]
       pub fn mint_nft(&mut self, token_value:u128)-> Result<(), StakingError>{
        let caller = Self::env().caller();
        //let now = Self::env().block_timestamp();

        self.transfer_psp22_from(&caller, &Self::env().account_id(), token_value)?;
        
        //self.mint_psp34(caller,token_value)?;
        Ok(())
        }
      #[ink(message)]
       pub fn add_token_value(&mut self, token_value:u128,nft_id:u128)-> Result<(), StakingError>{
        let caller = Self::env().caller();
        self.transfer_psp22_from(&caller, &Self::env().account_id(), token_value)?;
        let mut token: contract_ref!(GovernanceNFT) = self.nft.into();
        if let Err(e) = token.increment_weight(nft_id,token_value) {
                return Err(StakingError::NFTError(e));
        }
        Ok(())
        }
        #[ink(message)]
        pub fn remove_token_value(&mut self, token_value:u128,nft_id:u128)-> Result<(), StakingError>{
         let caller = Self::env().caller();         
         let mut token: contract_ref!(GovernanceNFT) = self.nft.into();
         if let Err(e) = token.decrement_weight(nft_id,token_value) {
                return Err(StakingError::NFTError(e));
         }
         self.transfer_psp22_from(&Self::env().account_id(), &caller,token_value)?;
         Ok(())
        }
       #[ink(message)]
       pub fn unwrap(&mut self, token_id:u128)-> Result<(), StakingError>{
        let caller = Self::env().caller();
        let mut gtoken: contract_ref!(GovernanceNFT) = self.nft.into();
        let data= gtoken.get_governance_data(token_id);
        self.transfer_psp22_from( &Self::env().account_id(),&caller, data.vote_weight)?;        
        self.burn_psp34(caller,token_id)?;
        Ok(())
        }       
    }
}