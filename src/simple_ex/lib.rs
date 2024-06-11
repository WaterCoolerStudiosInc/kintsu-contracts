#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
mod example{
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
    pub struct Example {
      
        nft:AccountId,
      
    }
    impl Example {
     
       
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
            
            nft_hash: Hash,
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
                  
                    nft: GovernanceNFTRef::to_account_id(&nft_ref),
                                    
            }

        }
        #[ink(message)]
        pub fn get_governance_nft(&self)->AccountId{
            self.nft
        }
       #[ink(message)]
       pub fn mint_nft(&mut self, token_value:u128)-> Result<(), StakingError>{
        let caller = Self::env().caller();
        
        self.mint_psp34(caller,token_value)?;
        Ok(())
        }
      
    }
}