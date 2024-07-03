#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
pub mod governance {
    
    use vault::Vault;
    use governance_nft::GovernanceNFT;
    use multisig::MultiSig;
    use ink::{
        codegen::EmitEvent,
        contract_ref,
        env::{
            debug_println,
            hash::{HashOutput, Sha2x256},
            hash_encoded, Error as InkEnvError,
        },
        prelude::{format, string::String, vec::Vec},
        reflect::ContractEventBase,
        storage::Mapping,
        ToAccountId,
    };
    use psp22::{PSP22Error, PSP22};
    use psp34::{Id, PSP34};

    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum GovernanceError {
        RegistryFailure,
        MultiSigError,
        VaultFailure,
        Unauthorized,
        InvalidInput,
        InvalidVotePeriodUpdate,
        InvalidVoteWeight,
        MaxProposals,
        ExistingProposal,
        NonExistingProposal,
        ProposalInactive,
        DoubleVote,
        TokenError(PSP22Error),
    }
    #[derive(Debug, PartialEq, Eq, scale::Encode, Clone, scale::Decode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum PropType {
        TransferFunds(TokenTransfer),
        UpdateStakingRewards(u128),
        AddCouncilMember(AccountId),
        RemoveCouncilMember(AccountId),
        ThresholdChange(u16),
        FeeChange(u16),
        VoteDelayUpdate(u64),
        VotePeriodUpdate(u64),
    }
    #[derive(Debug, PartialEq, Eq, scale::Encode, Clone, scale::Decode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum ProposalState {
        Created,
        Active,
        Expired,
    }
    #[derive(Debug, PartialEq, Eq, Clone, scale::Encode, scale::Decode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct TokenTransfer {
        token: AccountId,
        amount: u128,
        to: AccountId,
    }
    #[derive(Debug,PartialEq, Eq, Clone, scale::Encode, scale::Decode)]
   
   
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct Proposal {
        pub creation_timestamp: u64,
        pub creator_id: u128,
        pub prop_id: String,
        pub prop_type: PropType,
        pub pro_vote_count: u128,
        pub con_vote_count: u128,
        pub vote_start: u64,
        pub vote_end: u64,
    }
    #[ink(storage)]
    pub struct Governance {
        pub gov_nft: AccountId,
        pub vault: AccountId,
        pub multisig:AccountId,
        pub execution_threshold: u128,
        pub rejection_threshold: u128,  // threshold of votes to pass
        pub acceptance_threshold: u128, //
        pub creation_time: u64,
        pub voting_delay: u64,
        pub voting_period: u64,
        pub proposals: Vec<Proposal>,
        pub last_proposal: Mapping<u128, u64>,
        pub voted: Mapping<(String, u128),bool>,
    }
    pub const DAY: u64 = 86400 * 1000;
    pub const MIN_VOTING_DELAY: u64 = 1 * DAY;
    pub const MAX_VOTING_DELAY: u64 = 7 * DAY;
    pub const MIN_VOTING_PERIOD:u64 = 5 * DAY;
    pub const MAX_VOTING_PERIOD:u64 = 30 * DAY;

    type Event = <Governance as ContractEventBase>::Type;

    #[ink(event)]
    pub struct ProposlCreated {
        proposal: Proposal,
    }
    #[ink(event)]
    pub struct VoteSubmitted {
        proposal_id:String,
        nft_id:u128,
        pro_vote:bool
    }
    #[ink(event)]
    pub struct ProposlRejected {
        proposal: Proposal,
    }
    #[ink(event)]
    pub struct ProposlExecuted {
        proposal: Proposal,
    }
    #[ink(event)]
    pub struct ProposalsExpired {
        proposals: Vec<Proposal>,
    }
    impl Governance {
        fn emit_event<EE>(emitter: EE, event: Event)
        where
            EE: EmitEvent<Governance>,
        {
            emitter.emit_event(event);
        }
        fn validate_vote_delay_update(&mut self,update:u64)->bool {
            update>MIN_VOTING_DELAY && update< MAX_VOTING_DELAY
        }
        fn validate_vote_period_update(&mut self,update:u64)->bool {
            update>MIN_VOTING_PERIOD&& update <MAX_VOTING_PERIOD
        }
        fn check_ownership(&self, id: u128, user: AccountId) -> bool {
            let mut nft: contract_ref!(PSP34) = self.gov_nft.into();
            let owner = nft.owner_of(psp34::Id::U128((id))).unwrap();
            owner == user
        }
        fn query_weight(&self, id: u128) -> u128 {
            let mut nft: contract_ref!(GovernanceNFT) = self.gov_nft.into();
            let data = nft.get_governance_data(id);
            data.vote_weight
        }
        fn get_proposal_state(&self, prop: Proposal, current_time: u64) -> ProposalState {
            match current_time {
                current_time if current_time < prop.vote_start => ProposalState::Created,
                current_time if current_time > prop.vote_start && current_time < prop.vote_end => {
                    ProposalState::Active
                }
                _ => ProposalState::Expired,
            }
        }
        fn generate_proposal_id(&self,time_stamp:u64,creator_id:u128)->String{
            let encodable = (time_stamp, creator_id);
            let mut output = <Sha2x256 as HashOutput>::Type::default();
            hash_encoded::<Sha2x256, _>(&encodable, &mut output);
            String::from_utf8(output.to_vec()).unwrap()
        }
        fn update_vault_fee(&self, new_fee: &u16) -> Result<(), GovernanceError> {
            let mut vault: contract_ref!(Vault) = self.vault.into();
            if let Err(e) = vault.adjust_fee(*new_fee) {
                return Err(GovernanceError::VaultFailure);
            }
            Ok(())
        }
        fn remove_expired_proposals(&mut self, current_time: u64) -> Vec<Proposal> {
            let (active, expired) = self
                .proposals
                .clone()
                .into_iter()
                .partition(|p| p.vote_end <= current_time);
            self.proposals = active;
           
            expired
        }
        fn remove_council_member(&self,member:&AccountId)->Result<(),GovernanceError>{
            let mut multisig: contract_ref!(MultiSig) = self.multisig.into();
            if let Err(e) = multisig.remove_signer(*member) {
                return Err(GovernanceError::MultiSigError);
            }
            Ok(())
        }
        fn add_council_member(&self,member:&AccountId)->Result<(),GovernanceError>{
            let mut multisig: contract_ref!(MultiSig) = self.multisig.into();
            if let Err(e) = multisig.remove_signer(*member) {
                return Err(GovernanceError::MultiSigError);
            }
            Ok(())
        }
        
        /**
         * TransferFunds(TokenTransfer),
            AddCouncilMember(AccountId),
            RemoveCouncilMember(AccountId),
            ThresholdChange(u16),
            FeeChange(u16),
            VoteDelayUpdate(u64),
            VotePeriodUpdate(u64),
         */
        fn handle_pro_vote(&mut self, index: usize, weight: u128) ->  Result<(), GovernanceError> {
            if self.proposals[index].pro_vote_count + weight >= self.execution_threshold {
                match &self.proposals[index].prop_type {
                    PropType::TransferFunds(TokenTransfer) => self.transfer_psp22_from(
                        TokenTransfer.token,
                        &Self::env().account_id(),
                        &TokenTransfer.to,
                        TokenTransfer.amount,
                    )?,
                    PropType::VoteDelayUpdate(update)=>self.voting_delay=*update,
                    PropType::VotePeriodUpdate(update)=>self.voting_period=*update,
                    PropType::AddCouncilMember(member)=>self.add_council_member(member)?,
                    PropType::RemoveCouncilMember(member)=>self.remove_council_member(member)?,
                    PropType::FeeChange(new_fee)=>self.update_vault_fee(new_fee)?,
                    PropType::UpdateStakingRewards(new_rate)=>(),
                    _ => (),
                };
                Self::emit_event(
                    Self::env(),
                    Event::ProposlExecuted(ProposlExecuted {
                       proposal:self.proposals[index].clone()
                    }),
                );
                self.proposals.swap_remove(index);
            } else {
                self.proposals[index].pro_vote_count += weight;
            }
            Ok(())
        }
        fn handle_con_vote(&mut self, index: usize, weight: u128) -> Result<(), GovernanceError>{
            if self.proposals[index].pro_vote_count + weight > self.rejection_threshold {
                Self::emit_event(
                    Self::env(),
                    Event::ProposlRejected(ProposlRejected {
                       proposal:self.proposals[index].clone()
                    }),
                );
                self.proposals.swap_remove(index);
            } else {
                self.proposals[index].con_vote_count += weight;
            }
            Ok(())
        }
        fn transfer_psp22_from(
            &self,
            contract: AccountId,
            from: &AccountId,
            to: &AccountId,
            amount: Balance,
        ) -> Result<(), GovernanceError> {
            let mut token: contract_ref!(PSP22) = contract.into();
            if let Err(e) = token.transfer_from(*from, *to, amount, Vec::new()) {
                return Err(GovernanceError::TokenError(e));
            }
            Ok(())
        }
        #[ink(constructor)]
        pub fn new(
            vault: AccountId,
            _multisig:AccountId,
            _gov_nft: AccountId,
            exec_threshold: u128,
            reject_threshold: u128,
            acc_threshold: u128,           
        ) -> Self {
            Self {
                gov_nft: _gov_nft,
                vault: vault,
                multisig:_multisig,
                execution_threshold: exec_threshold,
                rejection_threshold: reject_threshold,
                acceptance_threshold: acc_threshold,
                creation_time: Self::env().block_timestamp(),
                voting_delay: 2 * DAY,
                voting_period: 7 * DAY,
                proposals: Vec::new(),
                last_proposal: Mapping::new(),
                voted: Mapping::new(),
            }
        }
        #[ink(message)]
        pub fn get_proposal_by_id(&self,id:String) -> Option<Proposal>{
            self.proposals.clone().into_iter()
            .find(|p| p.prop_id == id)    
        }
        #[ink(message)]
        pub fn get_proposal_by_nft(&self,id:u128) -> Option<Proposal>{
            self.proposals.clone().into_iter()
            .find(|p| p.creator_id == id)    
        }
        #[ink(message)]
        pub fn create_proposal(
            &mut self,
            prop: PropType,
            nft_id: u128,
        ) -> Result<(), GovernanceError> {
            let current_time = Self::env().block_timestamp();
            let expired=self.remove_expired_proposals(current_time);
            if expired.len() >1{
                Self::emit_event(
                    Self::env(),
                    Event::ProposalsExpired(ProposalsExpired {
                       proposals:expired
                    }),
                );
            }            
            if self.check_ownership(nft_id, Self::env().caller()) != true {
                return Err(GovernanceError::Unauthorized);
            }
            if self.query_weight(nft_id) < self.acceptance_threshold {
                return Err(GovernanceError::InvalidVoteWeight);
            }
            if self.proposals.len() == 100 {
                return Err(GovernanceError::MaxProposals);
            }
            let vote_update_check= match prop{
                PropType::VoteDelayUpdate(update)=>self.validate_vote_delay_update(update),
                PropType::VotePeriodUpdate(update)=>self.validate_vote_period_update(update),
                _ =>true
            };
            if !vote_update_check{
                return Err(GovernanceError::InvalidVotePeriodUpdate);
            }
            if self
                .proposals
                .clone()
                .into_iter()
                .find(|p| p.creator_id == nft_id).is_some()         
               

            {
                return Err(GovernanceError::ExistingProposal);
            }
            // Generate Unique ID for proposals 
            let encodable = (Self::env().block_timestamp(), nft_id);
            let mut output = <Sha2x256 as HashOutput>::Type::default();
            hash_encoded::<Sha2x256, _>(&encodable, &mut output);
            let key_string = String::from_utf8(output.to_vec()).unwrap();
            let new_prop=Proposal {
                creation_timestamp: Self::env().block_timestamp(),
                creator_id: nft_id,
                prop_type: prop,
                prop_id: key_string,
                pro_vote_count: 0u128,
                con_vote_count: 0u128,
                vote_start: Self::env().block_timestamp() + self.voting_delay,
                vote_end: Self::env().block_timestamp() + self.voting_delay + self.voting_period,
            };
            self.proposals.push(new_prop.clone());
            Self::emit_event(
                Self::env(),
                Event::ProposlCreated(ProposlCreated {
                   proposal:new_prop
                }),
            );
            Ok(())
        }

        #[ink(message)]
        pub fn vote(
            &mut self,
            prop_id: String,
            nft_id: u128,
            pro: bool,
        ) -> Result<(), GovernanceError> {
            let current_time = Self::env().block_timestamp();
            if self.check_ownership(nft_id, Self::env().caller()) != true {
                return Err(GovernanceError::Unauthorized);
            }
            let weight = self.query_weight(nft_id);
            let index = self
                .proposals
                .clone()
                .into_iter()
                .position(|p| p.prop_id == prop_id)
                .unwrap();
            let  proposal = self.proposals[index].clone();

            if self.get_proposal_state(proposal, current_time) != ProposalState::Active {
                return Err(GovernanceError::ProposalInactive);
            }
            if self.voted.get((prop_id.clone(),nft_id)).unwrap() {
                return Err(GovernanceError::DoubleVote);
            }
            self.voted.insert((prop_id.clone(),nft_id),&true);
            match pro {
                true => self.handle_pro_vote(index,weight)?,
                false => self.handle_con_vote(index,weight)?,
            };
            Self::emit_event(
                Self::env(),
                Event::VoteSubmitted(VoteSubmitted{
                   proposal_id:prop_id,
                   nft_id,
                   pro_vote:pro
                }),
            );
            Ok(())
        }
    }
}