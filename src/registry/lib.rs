#![cfg_attr(not(feature = "std"), no_std, no_main)]
pub use crate::registry::RegistryRef;

#[ink::contract]
pub mod registry {

    use ink::{
        contract_ref,
        env::{Error as InkEnvError},
        prelude::{format, string::String, vec::Vec},
        storage::Mapping,
        ToAccountId,
    };
    use nomination_agent::{NominationAgentRef, traits::INominationAgent};

    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum RegistryError {
        InvalidInput,
        DuplicateAgent,
        AgentNotFound,
        ActiveAgent,
        Initialization,
        InvalidPermissions,
        InvalidRole,
        NoChange,
        /// An interaction with ink! environment has failed
        // NOTE: We're representing the `ink::env::Error` as `String` b/c the
        // type does not have Encode/Decode implemented.
        InkEnvError(String),
    }

    impl From<InkEnvError> for RegistryError {
        fn from(e: InkEnvError) -> Self {
            RegistryError::InkEnvError(format!("{:?}", e))
        }
    }

    #[derive(Debug, PartialEq, Eq, Clone, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    pub struct Role {
        admin: AccountId,
        account: AccountId,
    }

    #[derive(Debug, PartialEq, Eq, Clone, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    pub enum RoleType {
        // Permission to add new agents
        AddAgent,
        // Permission to update agent weights
        UpdateAgents,
        // Permission to remove deprecated agents
        RemoveAgent,
    }

    #[derive(Debug, PartialEq, Eq, Clone, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    pub struct Agent {
        pub address: AccountId,
        pub weight: u64,
        pub initialized: bool,
    }

    #[ink(event)]
    pub struct AgentAdded {
        #[ink(topic)]
        agent: AccountId,
    }
    #[ink(event)]
    pub struct AgentInitialized {
        #[ink(topic)]
        agent: AccountId,
    }
    #[ink(event)]
    pub struct AgentUpdated {
        #[ink(topic)]
        agent: AccountId,
        old_weight: u64,
        new_weight: u64,
    }
    #[ink(event)]
    pub struct AgentDeleted {
        #[ink(topic)]
        agent: AccountId,
    }
    #[ink(event)]
    pub struct RoleAccountChanged {
        role_type: RoleType,
        new_account: AccountId,
    }
    #[ink(event)]
    pub struct RoleAdminChanged {
        role_type: RoleType,
        new_account: AccountId,
    }

    #[ink(storage)]
    pub struct Registry {
        // List of all nomination agents including their deployment and relative weight
        pub agents: Vec<Agent>,
        // Sum of nomination agent relative weights
        pub total_weight: u64,
        // Permissions for adding agents, updating weights, and removing agents
        pub roles: Mapping<RoleType, Role>,
        // Used for instantiating agents
        pub vault: AccountId,
        pub nomination_agent_hash: Hash,
        pub nomination_agent_counter: u128,
    }

    impl Registry {
        #[ink(constructor)]
        pub fn deploy_hash() -> Self {
            Self {
                agents: Vec::new(),
                total_weight: 0,
                roles: Mapping::default(),
                vault: Self::env().caller(),
                nomination_agent_hash: Hash::default(),
                nomination_agent_counter: 0,
            }
        }

        #[ink(constructor)]
        pub fn new(
            role_add: AccountId,
            role_update: AccountId,
            role_remove: AccountId,
            nomination_agent_hash: Hash,
        ) -> Self {
            let mut initial_roles = Mapping::default();
            initial_roles.insert(RoleType::AddAgent, &Role { admin: role_add, account: role_add });
            initial_roles.insert(RoleType::UpdateAgents, &Role { admin: role_update, account: role_update });
            initial_roles.insert(RoleType::RemoveAgent, &Role { admin: role_remove, account: role_remove });

            Self {
                agents: Vec::new(),
                total_weight: 0,
                roles: initial_roles,
                vault: Self::env().caller(),
                nomination_agent_hash,
                nomination_agent_counter: 0,
            }
        }

        /// Add a new nomination agent
        ///
        /// Caller must have the AddAgent role.
        /// Cannot add the same nomination agent twice.
        #[ink(message, payable)]
        pub fn add_agent(
            &mut self,
            admin: AccountId,
            validator: AccountId,
            pool_create_amount: Balance,
            existential_deposit: Balance,
        ) -> Result<AccountId, RegistryError> {
            let caller = Self::env().caller();

            if caller != self.roles.get(RoleType::AddAgent).unwrap().account {
                return Err(RegistryError::InvalidPermissions);
            }

            let nomination_agent_counter = self.nomination_agent_counter; // shadow

            let agent_ref = NominationAgentRef::new(
                    self.vault,
                    admin,
                    validator,
                    pool_create_amount,
                    existential_deposit,
                )
                .endowment(pool_create_amount + existential_deposit)
                .code_hash(self.nomination_agent_hash)
                .salt_bytes(nomination_agent_counter.to_le_bytes())
                .instantiate();

            let agent_address = NominationAgentRef::to_account_id(&agent_ref);

            self.nomination_agent_counter = nomination_agent_counter + 1;

            self.agents.push(Agent {
                address: agent_address,
                weight: 0,
                initialized: false,
            });

            Self::env().emit_event(
                AgentAdded {
                    agent: agent_address,
                }
            );

            Ok(agent_address)
        }

        /// Configures an agent with the necessary information to integrate with Kintsu.
        /// Ensures agent has all nomination pool roles (Root, Nominator, Bouncer).
        /// Sets nomination pool status to Blocked to disallow others from joining.
        /// Nominates to the validator specified in `create_agent`.
        ///
        /// Caller must have the AddAgent role.
        /// Agent must NOT be initialized.
        #[ink(message)]
        pub fn initialize_agent(
            &mut self,
            agent: AccountId,
            pool_id: u32,
        ) -> Result<(), RegistryError> {
            let caller = Self::env().caller();

            if caller != self.roles.get(RoleType::AddAgent).unwrap().account {
                return Err(RegistryError::InvalidPermissions);
            }

            if let Some(index) = self.agents.iter().position(|a| a.address == agent) {
                // Must be un-initialized
                if self.agents[index].initialized == true {
                    return Err(RegistryError::Initialization);
                }

                let mut agent_contract: contract_ref!(INominationAgent) = agent.into();
                agent_contract.initialize(pool_id).expect("Agent becomes initialized");

                self.agents[index].initialized = true;

                Self::env().emit_event(
                    AgentInitialized {
                        agent,
                    }
                );
            } else {
                return Err(RegistryError::AgentNotFound);
            }

            Ok(())
        }

        /// Update weight of existing nomination agents
        ///
        /// Caller must have the UpdateAgents role.
        /// Agent must be initialized.
        #[ink(message)]
        pub fn update_agents(
            &mut self,
            agents: Vec<AccountId>,
            new_weights: Vec<u64>,
        ) -> Result<(), RegistryError> {
            let caller = Self::env().caller();

            if caller != self.roles.get(RoleType::UpdateAgents).unwrap().account {
                return Err(RegistryError::InvalidPermissions);
            }

            if agents.len() != new_weights.len() {
                return Err(RegistryError::InvalidInput);
            }

            for (args_index, &agent) in agents.iter().enumerate() {
                if let Some(index) = self.agents.iter().position(|a| a.address == agent) {
                    // Must be initialized
                    if self.agents[index].initialized == false {
                        return Err(RegistryError::Initialization);
                    }

                    let old_weight = self.agents[index].weight;
                    let new_weight = new_weights[args_index];

                    self.total_weight -= old_weight;
                    self.total_weight += new_weight;

                    self.agents[index].weight = new_weight;

                    Self::env().emit_event(
                        AgentUpdated {
                            agent,
                            old_weight,
                            new_weight,
                        }
                    );
                } else {
                    return Err(RegistryError::AgentNotFound);
                }
            }

            Ok(())
        }

        /// Removes a nomination agent
        /// This is intended to remove fully deprecated agents to save gas during iteration.
        ///
        /// Caller must have the RemoveAgent role.
        /// Agent must have no AZERO staked (excludes initial bond).
        /// Agent must have no AZERO unbonding.
        /// Agent must be initialized.
        #[ink(message)]
        pub fn remove_agent(
            &mut self,
            agent: AccountId,
        ) -> Result<(), RegistryError> {
            let caller = Self::env().caller();

            if caller != self.roles.get(RoleType::RemoveAgent).unwrap().account {
                return Err(RegistryError::InvalidPermissions);
            }

            if let Some(index) = self.agents.iter().position(|a| a.address == agent) {
                // Must be initialized
                if self.agents[index].initialized == false {
                    return Err(RegistryError::Initialization);
                }

                let mut agent_contract: contract_ref!(INominationAgent) = agent.into();

                // Do not delete agents with AZERO staked
                if agent_contract.get_staked_value() > 0 {
                    return Err(RegistryError::ActiveAgent);
                }

                // Do not delete agents with AZERO unbonding
                if agent_contract.get_unbonding_value() > 0 {
                    return Err(RegistryError::ActiveAgent);
                }

                let weight = self.agents[index].weight;
                if weight > 0 {
                    self.total_weight -= weight;
                }

                self.agents.remove(index);

                agent_contract.destroy().expect("Agent begins the destruction process");

                Self::env().emit_event(
                    AgentDeleted {
                        agent,
                    }
                );
            } else {
                return Err(RegistryError::AgentNotFound);
            }

            Ok(())
        }

        #[ink(message)]
        pub fn get_agents(
            &self,
        ) -> (u64, Vec<Agent>) {
            (self.total_weight, self.agents.clone())
        }

        /// ================================ Update Role Methods ================================

        /// Transfers role to a new account
        ///
        /// Caller must be the admin for the role
        #[ink(message)]
        pub fn transfer_role(
            &mut self,
            role_type: RoleType,
            new_account: AccountId,
        ) -> Result<(), RegistryError> {
            if let Some(mut role) = self.roles.get(role_type.clone()) {
                if Self::env().caller() != role.admin {
                    return Err(RegistryError::InvalidPermissions);
                }
                if role.account == new_account {
                    return Err(RegistryError::NoChange);
                }

                // Update role account
                role.account = new_account;
                self.roles.insert(role_type.clone(), &role);

                Self::env().emit_event(
                    RoleAccountChanged {
                        role_type,
                        new_account,
                    }
                );
            } else {
                return Err(RegistryError::InvalidRole);
            }

            Ok(())
        }

        /// Transfers administration of role to a new account
        ///
        /// Caller must be the admin for the role
        #[ink(message)]
        pub fn transfer_role_admin(
            &mut self,
            role_type: RoleType,
            new_account: AccountId,
        ) -> Result<(), RegistryError> {
            if let Some(mut role) = self.roles.get(role_type.clone()) {
                if Self::env().caller() != role.admin {
                    return Err(RegistryError::InvalidPermissions);
                }
                if role.admin == new_account {
                    return Err(RegistryError::NoChange);
                }

                // Update role admin
                role.admin = new_account;
                self.roles.insert(role_type.clone(), &role);

                Self::env().emit_event(
                    RoleAdminChanged {
                        role_type,
                        new_account,
                    }
                );
            } else {
                return Err(RegistryError::InvalidRole);
            }

            Ok(())
        }

        /// ================================ View Only Role Methods ================================

        #[ink(message)]
        pub fn get_role(&self, role_type: RoleType) -> AccountId {
            self.roles.get(role_type).unwrap().account
        }

        #[ink(message)]
        pub fn get_role_admin(&self, role_type: RoleType) -> AccountId {
            self.roles.get(role_type).unwrap().admin
        }
    }
}
