use drink::session::contract_transcode::ContractMessageTranscoder;
use serde::Deserialize;
use serde_json::from_str;
use std::{
    fs::{read, read_to_string},
    path::PathBuf,
    rc::Rc,
};

// Fetch deployed hashes

#[derive(Deserialize)]
struct Source {
    hash: String,
}
#[derive(Deserialize)]
struct Artifact {
    source: Source,
}
pub fn hash_governance_nft() -> String {
    let json = read_to_string("../deployments/governance_nft/governance_nft.json").unwrap();
    let artifact: Artifact = from_str(&json).unwrap();
    artifact.source.hash
}
pub fn transcoder_governance_nft() -> Option<Rc<ContractMessageTranscoder>> {
    Some(Rc::new(
        ContractMessageTranscoder::load(PathBuf::from(
            "../deployments/governance_nft/governance_nft.json",
        ))
            .expect("Failed to create transcoder"),
    ))
}
pub fn bytes_governance_nft() -> Vec<u8> {
    read("../deployments/governance_nft/governance_nft.wasm")
        .expect("Failed to find or read contract file")
}

pub fn hash_governance_token() -> String {
    let json = read_to_string("../deployments/governance_token/governance_token.json").unwrap();
    let artifact: Artifact = from_str(&json).expect("Should extract hash from share_token.json");
    artifact.source.hash
}

// Transcoders for making contract calls

pub fn transcoder_governance_token() -> Option<Rc<ContractMessageTranscoder>> {
    Some(Rc::new(
        ContractMessageTranscoder::load(PathBuf::from(
            "../deployments/governance_token/governance_token.json",
        ))
            .expect("Failed to create transcoder"),
    ))
}
pub fn bytes_governance_token() -> Vec<u8> {
    read("../deployments/governance_token/governance_token.wasm")
        .expect("Failed to find or read contract file")
}

pub fn hash_governance_staking() -> String {
    let json = read_to_string("../deployments/governance_staking/governance_staking.json").unwrap();
    let artifact: Artifact = from_str(&json).expect("Should extract hash from share_token.json");
    artifact.source.hash
}

// Transcoders for making contract calls

pub fn transcoder_governance_staking() -> Option<Rc<ContractMessageTranscoder>> {
    Some(Rc::new(
        ContractMessageTranscoder::load(PathBuf::from(
            "../deployments/governance_staking/governance_staking.json",
        ))
            .expect("Failed to create transcoder"),
    ))
}
pub fn bytes_governance_staking() -> Vec<u8> {
    read("../deployments/governance_staking/governance_staking.wasm")
        .expect("Failed to find or read contract file")
}
pub fn simple_ex() -> String {
    let json = read_to_string("../deployments/simple_ex/simple_ex.json").unwrap();
    let artifact: Artifact = from_str(&json).expect("Should extract hash from share_token.json");
    artifact.source.hash
}
pub fn transcoder_simple_ex() -> Option<Rc<ContractMessageTranscoder>> {
    Some(Rc::new(
        ContractMessageTranscoder::load(PathBuf::from(
            "../deployments/simple_ex/simple_ex.json",
        ))
            .expect("Failed to create transcoder"),
    ))
}
pub fn bytes_simple_ex() -> Vec<u8> {
    read("../deployments/simple_ex/simple_ex.wasm")
        .expect("Failed to find or read contract file")
}
