#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
pub mod governance {

    use hex::*;

    use vault::traits::IVault;

    use governance_nft::{GovernanceNFT, GovernanceNFTRef};
    use governance_staking::{Staking, StakingRef};
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
    use multisig::{MultiSig, MultiSigRef};

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
        TransferError,
        StakingError,
        TokenError(PSP22Error),
    }
    #[derive(Debug, PartialEq, Eq, scale::Encode, Clone, scale::Decode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum PropType {
        // Transfer Azero from governance
        TransferFunds(TokenTransfer),
        // Transfer psp22 token from governance
        NativeTokenTransfer(AccountId, u128),
        // update tokens per second for staker in staking contract
        ChangeStakingRewardRate(u128),
        // Add to multisig
        AddCouncilMember(AccountId),
        // remove then add to multisig
        ReplaceCouncilMember(AccountId, AccountId),
        // remove from multisig
        RemoveCouncilMember(AccountId),
        // change threshold for multisig acceptance
        ChangeMultiSigThreshold(u16),
        // change vault fee
        FeeChange(u16),
        // change vault compound acceptance
        CompoundIncentiveChange(u16),
        // change  governance proposal acceptance weight requirement
        AcceptanceWeightUpdate(u128),
        // change vote periodi delay
        VoteDelayUpdate(u64),
        // update voting perioud
        VotePeriodUpdate(u64),
        // update threshold proposals
        UpdateRejectThreshhold(u128),
        // upddate execution threshhold for proposals
        UpdateExecThreshhold(u128),
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
    #[derive(Debug, PartialEq, Eq, Clone, scale::Encode, scale::Decode)]
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
        pub temp_admin: AccountId,
        pub gov_nft: AccountId,
        pub vault: AccountId,
        pub staking: AccountId,
        pub multisig: Option<AccountId>,
        pub execution_threshold: u128,
        pub rejection_threshold: u128,  // threshold of votes to pass
        pub acceptance_threshold: u128, //
        pub creation_time: u64,
        pub voting_delay: u64,
        pub voting_period: u64,
        pub proposals: Vec<Proposal>,
        pub last_proposal: Mapping<u128, u64>,
        pub voted: Mapping<(String, u128), bool>,
    }
    pub const DAY: u64 = 86400 * 1000;
    pub const MIN_VOTING_DELAY: u64 = 1 * DAY;
    pub const MAX_VOTING_DELAY: u64 = 7 * DAY;
    pub const MIN_VOTING_PERIOD: u64 = 5 * DAY;
    pub const MAX_VOTING_PERIOD: u64 = 30 * DAY;

    type Event = <Governance as ContractEventBase>::Type;

    #[ink(event)]
    pub struct ProposlCreated {
        proposal: Proposal,
    }
    #[ink(event)]
    pub struct VoteSubmitted {
        proposal_id: String,
        nft_id: u128,
        pro_vote: bool,
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
        fn validate_vote_delay_update(&mut self, update: u64) -> bool {
            update > MIN_VOTING_DELAY && update < MAX_VOTING_DELAY
        }
        fn validate_vote_period_update(&mut self, update: u64) -> bool {
            update > MIN_VOTING_PERIOD && update < MAX_VOTING_PERIOD
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
        fn generate_proposal_id(&self, time_stamp: u64, creator_id: u128) -> String {
            let encodable = (time_stamp, creator_id);
            let mut output = <Sha2x256 as HashOutput>::Type::default();
            hash_encoded::<Sha2x256, _>(&encodable, &mut output);
            String::from_utf8(output.to_vec()).unwrap()
        }
        fn update_vault_fee(&self, new_fee: &u16) -> Result<(), GovernanceError> {
            let mut vault: contract_ref!(IVault) = self.vault.into();
            if let Err(e) = vault.adjust_fee(*new_fee) {
                return Err(GovernanceError::VaultFailure);
            }
            Ok(())
        }
        fn update_incentive(&self, new_inventive: &u16) -> Result<(), GovernanceError> {
            let mut vault: contract_ref!(IVault) = self.vault.into();
            if let Err(e) = vault.adjust_incentive(*new_inventive) {
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
        fn remove_council_member(&self, member: &AccountId) -> Result<(), GovernanceError> {
            let mut multisig: contract_ref!(MultiSig) = self.multisig.unwrap().into();
            if let Err(e) = multisig.remove_signer(*member) {
                return Err(GovernanceError::MultiSigError);
            }
            Ok(())
        }
        fn add_council_member(&self, member: &AccountId) -> Result<(), GovernanceError> {
            let mut multisig: contract_ref!(MultiSig) = self.multisig.unwrap().into();
            if let Err(e) = multisig.remove_signer(*member) {
                return Err(GovernanceError::MultiSigError);
            }
            Ok(())
        }

        fn change_multisig_threshold(&self, update: u16) -> Result<(), GovernanceError> {
            let mut multisig: contract_ref!(MultiSig) = self.multisig.unwrap().into();
            if let Err(e) = multisig.update_threshold(update) {
                return Err(GovernanceError::MultiSigError);
            }
            Ok(())
        }
        fn replace_council_member(
            &self,
            member: &AccountId,
            new_member: AccountId,
        ) -> Result<(), GovernanceError> {
            let mut multisig: contract_ref!(MultiSig) = self.multisig.unwrap().into();
            if let Err(e) = multisig.replace_signer(*member, new_member) {
                return Err(GovernanceError::MultiSigError);
            }
            Ok(())
        }
        fn update_staking_rewards(&self, new_reward: u128) -> Result<(), GovernanceError> {
            let mut staking: contract_ref!(Staking) = self.staking.into();
            if let Err(e) = staking.update_rewards_rate(new_reward) {
                return Err(GovernanceError::StakingError);
            }
            Ok(())
        }
        fn update_reject_threshold(&mut self, update: u128) {
            self.rejection_threshold = update;
        }
        fn update_execution_threshold(&mut self, update: u128) {
            self.execution_threshold = update;
        }
        fn update_acceptance_threshold(&mut self, update: u128) {
            self.acceptance_threshold = update;
        }
        fn transfer_native_funds(
            &self,
            to: AccountId,
            amount: u128,
        ) -> Result<(), GovernanceError> {
            if let Err(e) = Self::env().transfer(to, amount) {
                return Err(GovernanceError::TransferError);
            }
            Ok(())
        }
        /**
         // Transfer Azero from governance
        TransferFunds(TokenTransfer),
        // Transfer psp22 token from governance
        NativeTokenTransfer(u128),
        // update tokens per second for staker in staking contract
         // change  governance proposal acceptance weight requirement
        AcceptanceWeightUpdate(u128),
        // update rejection proposals
        UpdateRejectThreshhold(u128),
        // upddate execution threshhold for proposals
        UpdateExecThreshhold(u128),
         // change vote periodi delay
        VoteDelayUpdate(u64),
        // update voting perioud
        VotePeriodUpdate(u64),


         // Add to multisig
        AddCouncilMember(AccountId),
        // remove then add to multisig
        ReplaceCouncilMember(AccountId,AccountId),
        // remove from multisig
        RemoveCouncilMember(AccountId),
        // change threshold for multisig acceptance
        ChangeMultiSigThreshold(u16),

        // change vault fee
        FeeChange(u16),
        // change vault compound acceptance
        CompoundIncentiveChange(u16),






           ChangeStakingRewardRate(u128),
         **/

        fn handle_pro_vote(&mut self, index: usize, weight: u128) -> Result<(), GovernanceError> {
            if self.proposals[index].pro_vote_count + weight >= self.execution_threshold {
                match &self.proposals[index].prop_type {
                    PropType::TransferFunds(TokenTransfer) => self.transfer_psp22_from(
                        TokenTransfer.token,
                        &Self::env().account_id(),
                        &TokenTransfer.to,
                        TokenTransfer.amount,
                    )?,
                    PropType::NativeTokenTransfer(to, funds) => {
                        self.transfer_native_funds(*to, *funds)?
                    }
                    PropType::AcceptanceWeightUpdate(update) => {
                        self.update_acceptance_threshold(*update)
                    }
                    PropType::UpdateRejectThreshhold(update) => {
                        self.update_reject_threshold(*update)
                    }
                    PropType::UpdateExecThreshhold(update) => {
                        self.update_execution_threshold(*update)
                    }
                    PropType::VoteDelayUpdate(update) => self.voting_delay = *update,
                    PropType::VotePeriodUpdate(update) => self.voting_period = *update,

                    PropType::AddCouncilMember(member) => self.add_council_member(member)?,
                    PropType::ReplaceCouncilMember(member, replacement) => {
                        self.replace_council_member(member, *replacement)?
                    }
                    PropType::RemoveCouncilMember(member) => self.remove_council_member(member)?,
                    PropType::ChangeMultiSigThreshold(update) => {
                        self.change_multisig_threshold(*update)?
                    }

                    PropType::FeeChange(new_fee) => self.update_vault_fee(new_fee)?,
                    PropType::CompoundIncentiveChange(update) => self.update_incentive(update)?,

                    PropType::ChangeStakingRewardRate(new_rate) => {
                        self.update_staking_rewards(*new_rate)?
                    }

                    _ => (),
                };
                Self::emit_event(
                    Self::env(),
                    Event::ProposlExecuted(ProposlExecuted {
                        proposal: self.proposals[index].clone(),
                    }),
                );
                self.proposals.swap_remove(index);
            } else {
                self.proposals[index].pro_vote_count += weight;
            }
            Ok(())
        }
        fn handle_con_vote(&mut self, index: usize, weight: u128) -> Result<(), GovernanceError> {
            if self.proposals[index].pro_vote_count + weight > self.rejection_threshold {
                Self::emit_event(
                    Self::env(),
                    Event::ProposlRejected(ProposlRejected {
                        proposal: self.proposals[index].clone(),
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
            registry: AccountId,
            governance_token: AccountId,
            gov_nft_hash: Hash,
            staking_hash: Hash,
            exec_threshold: u128,
            reject_threshold: u128,
            acc_threshold: u128,
            interest_rate: u128,
        ) -> Self {
            let caller = Self::env().caller();

            let mut nft_ref: GovernanceNFTRef = GovernanceNFTRef::new(Self::env().account_id())
                .endowment(0)
                .code_hash(gov_nft_hash)
                .salt_bytes(&[7_u8.to_le_bytes().as_ref(), caller.as_ref()].concat()[..4])
                .instantiate();

            let staking_ref = StakingRef::new(
                governance_token,
                Self::env().account_id(),
                nft_ref.clone(),
                interest_rate,
            )
            .endowment(0)
            .code_hash(staking_hash)
            .salt_bytes(&[9_u8.to_le_bytes().as_ref(), caller.as_ref()].concat()[..4])
            .instantiate();

            nft_ref.set_admin(StakingRef::to_account_id(&staking_ref));

            let _gov_nft = GovernanceNFTRef::to_account_id(&nft_ref);

            Self {
                temp_admin: caller,
                gov_nft: _gov_nft,
                vault: vault,
                multisig: None,
                staking: StakingRef::to_account_id(&staking_ref),
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
        pub fn get_multsig(&self) -> Option<AccountId> {
            self.multisig
        }
        #[ink(message)]
        pub fn set_multsig(&mut self, new_contract: AccountId) -> Result<(), GovernanceError> {
            if Self::env().caller() != self.temp_admin {
                return Err(GovernanceError::Unauthorized);
            }
            self.temp_admin = Self::env().account_id();
            self.multisig = Some(new_contract);
            Ok(())
        }
        #[ink(message)]
        pub fn get_staking(&self) -> AccountId {
            self.staking
        }
        #[ink(message)]
        pub fn get_proposal_by_id(&self, id: String) -> Proposal {
            self.proposals
                .clone()
                .into_iter()
                .find(|p| p.prop_id == id)
                .unwrap_or(Proposal {
                    creation_timestamp: 0,
                    creator_id: 0,
                    prop_type: PropType::FeeChange(0),
                    prop_id: String::from("EMPTY"),
                    pro_vote_count: 0u128,
                    con_vote_count: 0u128,
                    vote_start: 0,
                    vote_end: 0,
                })
        }
        #[ink(message)]
        pub fn get_all_proposals(&self) -> Vec<Proposal> {
            self.proposals.clone()
        }
        #[ink(message)]
        pub fn get_proposal_by_nft(&self, id: u128) -> Proposal {
            self.proposals
                .clone()
                .into_iter()
                .find(|p| p.creator_id == id)
                .unwrap_or(Proposal {
                    creation_timestamp: 0,
                    creator_id: 0,
                    prop_type: PropType::FeeChange(0),
                    prop_id: String::from("EMPTY"),
                    pro_vote_count: 0u128,
                    con_vote_count: 0u128,
                    vote_start: 0,
                    vote_end: 0,
                })
        }
        #[ink(message)]
        pub fn create_proposal(
            &mut self,
            prop: PropType,
            nft_id: u128,
        ) -> Result<(), GovernanceError> {
            let current_time = Self::env().block_timestamp();
            let expired = self.remove_expired_proposals(current_time);
            if expired.len() > 1 {
                Self::emit_event(
                    Self::env(),
                    Event::ProposalsExpired(ProposalsExpired { proposals: expired }),
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
            let vote_update_check = match prop {
                PropType::VoteDelayUpdate(update) => self.validate_vote_delay_update(update),
                PropType::VotePeriodUpdate(update) => self.validate_vote_period_update(update),
                _ => true,
            };
            if !vote_update_check {
                return Err(GovernanceError::InvalidVotePeriodUpdate);
            }
            if self
                .proposals
                .clone()
                .into_iter()
                .find(|p| p.creator_id == nft_id)
                .is_some()
            {
                return Err(GovernanceError::ExistingProposal);
            }

            // Generate Unique ID for proposals
            let encodable = (Self::env().block_timestamp(), nft_id);
            let mut output = <Sha2x256 as HashOutput>::Type::default();
            hash_encoded::<Sha2x256, _>(&encodable, &mut output);
            debug_println!("{:?}{}", output.to_vec(), "Hash VALUE");
            // encode as hex string
            let key_string = encode(output);

            let new_prop = Proposal {
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
                Event::ProposlCreated(ProposlCreated { proposal: new_prop }),
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
            let proposal = self.proposals[index].clone();

            if self.get_proposal_state(proposal, current_time) != ProposalState::Active {
                return Err(GovernanceError::ProposalInactive);
            }
            if self.voted.get((prop_id.clone(), nft_id)).unwrap() {
                return Err(GovernanceError::DoubleVote);
            }
            self.voted.insert((prop_id.clone(), nft_id), &true);
            match pro {
                true => self.handle_pro_vote(index, weight)?,
                false => self.handle_con_vote(index, weight)?,
            };
            Self::emit_event(
                Self::env(),
                Event::VoteSubmitted(VoteSubmitted {
                    proposal_id: prop_id,
                    nft_id,
                    pro_vote: pro,
                }),
            );
            Ok(())
        }
    }
}
