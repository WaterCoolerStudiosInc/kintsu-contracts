#[cfg(test)]
mod sources;

#[cfg(test)]
mod tests {
    use drink::{
        chain_api::ChainApi,
        runtime::MinimalRuntime,
        session::Session,
        AccountId32,
    };
    use drink::session::NO_ARGS;
    use drink::session::contract_transcode::ContractMessageTranscoder;
    use std::error::Error;
    use crate::sources::*;
    use std::rc::Rc;
    
    pub fn call_function(
        mut sess: Session<MinimalRuntime>,
        contract: &AccountId32,
        sender: &AccountId32,
        func_name: String,
        args: Option<Vec<String>>,
        value: Option<u128>,
        transcoder: Option<Rc<ContractMessageTranscoder>>,
    ) -> Result<Session<MinimalRuntime>, Box<dyn Error>> {
        println!("Calling: {}()", func_name);
        if let Some(args) = args {
            sess.set_actor(sender.clone());
            sess.set_transcoder(contract.clone(), &transcoder.unwrap());
            sess.call_with_address(contract.clone(), &func_name, &args, value)?;
        } else {
            sess.set_actor(sender.clone());
            sess.set_transcoder(contract.clone(), &transcoder.unwrap());
            sess.call_with_address(contract.clone(), &func_name, NO_ARGS, value)?;
        }
    
        // Print debug logs
        let encoded = &sess.last_call_result().unwrap().debug_message;
        let decoded = encoded.iter().map(|b| *b as char).collect::<String>();
        let messages: Vec<String> = decoded.split('\n').map(|s| s.to_string()).collect();
        for line in messages {
            if line.len() > 0 {
                println!("LOG: {}", line);
            }
        }
    
        Ok(sess)
    }
    
   
    #[test]
    fn test_mint() -> Result<(), Box<dyn Error>> {
        let bob = AccountId32::new([1u8; 32]);
        let alice = AccountId32::new([2u8; 32]);
        let charlie = AccountId32::new([3u8; 32]);
        let dave = AccountId32::new([4u8; 32]);
        let ed = AccountId32::new([5u8; 32]);
        
        let mut sess: Session<MinimalRuntime> = Session::<MinimalRuntime>::new().unwrap();
        
        sess.upload(bytes_governance_nft()).expect("Session should upload registry bytes");
       

        sess.chain_api().add_tokens(alice.clone(), 100_000_000e10 as u128);
        sess.chain_api().add_tokens(bob.clone(), 100_000_000e10 as u128);
        sess.chain_api().add_tokens(charlie.clone(), 100_000_000e10 as u128);
        sess.chain_api().add_tokens(dave.clone(), 100_000_000e10 as u128);
        sess.chain_api().add_tokens(ed.clone(), 100_000_000e10 as u128);

        let ex_contract=sess.deploy(
            bytes_simple_ex(),
            "new",
            &[ 
                hash_governance_nft()
            ],
            vec![2],
            None,
            &transcoder_simple_ex().unwrap(),
        )?;
        let mut sess = call_function(
            sess,
            &ex_contract,
            &bob,
            String::from("get_governance_nft"),
            None,
            None,
            transcoder_governance_staking(),
        ).unwrap();
        let rr: Result<AccountId32, drink::errors::LangError> = sess.last_call_return().unwrap();
        let gov_nft = rr.unwrap();

        let  sess = call_function(
            sess,
            &ex_contract,
            &bob,
            String::from("mint_nft"),
            Some(vec![100_u128.to_string()]),
            None,
            transcoder_simple_ex(),
        ).unwrap();
        let rr: Result<(), drink::errors::LangError> = sess.last_call_return().unwrap();
        let  sess = call_function(
            sess,
            &ex_contract,
            &bob,
            String::from("mint_nft"),
            Some(vec![300_u128.to_string()]),
            None,
            transcoder_simple_ex(),
        ).unwrap();
        let mut sess = call_function(
            sess,
            &gov_nft,
            &bob,
            String::from("PSP34::total_supply"),
            Some(vec![]),
            None,
            transcoder_governance_nft(),
        ).unwrap();
        let rr: Result<u128, drink::errors::LangError> = sess.last_call_return().unwrap();
        let total_supply = rr.unwrap();
        println!("{:?}",total_supply);
        Ok(())
    }
}