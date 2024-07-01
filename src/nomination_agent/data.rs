use ink::prelude::vec::Vec;
use ink::primitives::AccountId;

#[allow(dead_code)]
#[derive(scale::Encode)]
pub enum MultiAddress<AccountId, AccountIndex> {
    // It's an account ID (pubkey).
    Id(AccountId),
    // It's an account index.
    Index(#[codec(compact)] AccountIndex),
    // It's some arbitrary raw bytes.
    Raw(Vec<u8>),
    // It's a 32 byte representation.
    Address32([u8; 32]),
    // Its a 20 byte representation.
    Address20([u8; 20]),
}
#[allow(dead_code)]
#[derive(scale::Encode)]
pub enum ConfigOp<_0> {
    #[codec(index = 0)]
    Noop,
    #[codec(index = 1)]
    Set(_0),
    #[codec(index = 2)]
    Remove,
}
#[allow(dead_code)]
#[derive(scale::Encode)]
pub enum PoolState {
    #[codec(index = 0)]
    Open,
    #[codec(index = 1)]
    Blocked,
    #[codec(index = 2)]
    Destroying,
}
#[derive(scale::Encode)]
pub enum BondExtra {
    FreeBalance { balance: u128 },
}
#[derive(scale::Encode)]
pub enum NominationCall {
    #[codec(index = 1)]
    BondExtra { extra: BondExtra },
    #[codec(index = 2)]
    ClaimPayout {},
    #[codec(index = 3)]
    Unbond {
        member_account: MultiAddress<AccountId, ()>,
        #[codec(compact)]
        unbonding_points: u128,
    },
    #[codec(index = 5)]
    WithdrawUnbonded {
        member_account: MultiAddress<AccountId, ()>,
        num_slashing_spans: u32,
    },
    #[codec(index = 6)]
    Create {
        #[codec(compact)]
        amount: u128,
        root: MultiAddress<AccountId, ()>,
        nominator: MultiAddress<AccountId, ()>,
        bouncer: MultiAddress<AccountId, ()>,
    },
    #[codec(index = 8)]
    Nominate {
        pool_id: u32,
        validators: Vec<AccountId>,
    },
    #[codec(index = 9)]
    SetState {
        pool_id: u32,
        state: PoolState,
    },
    #[codec(index = 12)]
    UpdateRoles {
        pool_id: u32,
        new_root: ConfigOp<AccountId>,
        new_nominator: ConfigOp<AccountId>,
        new_bouncer: ConfigOp<AccountId>,
    },
    #[codec(index = 13)]
    Chill {
        pool_id: u32,
    },
}
#[derive(scale::Encode)]
pub enum RuntimeCall {
    #[codec(index = 19)]
    NominationPools(NominationCall),
}
