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
    struct TestContext {
        sess: Session<MinimalRuntime>,
        gov_token:AccountId32,
        gov_nft:AccountId32,
        stake_contract:AccountId32,
        alice: AccountId32,
        bob: AccountId32,
        charlie: AccountId32,
        dave: AccountId32,
        ed: AccountId32,
    }
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
    
    fn setup() -> Result<TestContext, Box<dyn Error>> {
        let bob = AccountId32::new([1u8; 32]);
        let alice = AccountId32::new([2u8; 32]);
        let charlie = AccountId32::new([3u8; 32]);
        let dave = AccountId32::new([4u8; 32]);
        let ed = AccountId32::new([5u8; 32]);

        let mut sess: Session<MinimalRuntime> = Session::<MinimalRuntime>::new().unwrap();
        let gov_token = sess.deploy::<String>(
            bytes_governance_token(),
            "new",
            &[],
            vec![2],
            None,
            &transcoder_governance_token().unwrap(),
        )?;

        sess.upload(bytes_governance_nft()).expect("Session should upload registry bytes");

        sess.chain_api().add_tokens(alice.clone(), 100_000_000e10 as u128);
        sess.chain_api().add_tokens(bob.clone(), 100_000_000e10 as u128);
        sess.chain_api().add_tokens(charlie.clone(), 100_000_000e10 as u128);
        sess.chain_api().add_tokens(dave.clone(), 100_000_000e10 as u128);
        sess.chain_api().add_tokens(ed.clone(), 100_000_000e10 as u128);

     

        let stake_contract=sess.deploy(
            bytes_governance_staking(),
            "new",
            &[  gov_token.to_string(),
                hash_governance_token()
            ],
            vec![2],
            None,
            &transcoder_governance_staking().unwrap(),
        )?;
        sess.set_transcoder(stake_contract.clone(),&transcoder_governance_staking().unwrap());

        let mut sess = call_function(
            sess,
            &stake_contract,
            &bob,
            String::from("get_governance_nft"),
            None,
            None,
            transcoder_governance_staking(),
        ).unwrap();
        let rr: Result<AccountId32, drink::errors::LangError> = sess.last_call_return().unwrap();
        let gov_nft = rr.unwrap();


        println!("{:?}",stake_contract.to_string());
        println!("{:?}",gov_token.to_string());
        println!("{:?}",gov_nft.to_string());
        let sess=call_function(
            sess,
            &gov_token,
            &bob, // not bob
            String::from("PSP22::transfer_from"),
            Some(vec![ bob.to_string(),alice.to_string(), 100_000_000_000_000_u128.to_string(), "[]".to_string()]),
            None,
            transcoder_governance_token(),
        )?;
        let sess=call_function(
            sess,
            &gov_token,
            &bob, // not bob
            String::from("PSP22::transfer_from"),
            Some(vec![bob.to_string(), charlie.to_string(), 100_000_000_000_000_u128.to_string(), "[]".to_string()]),
            None,
            transcoder_governance_token(),
        )?;
        let sess=call_function(
            sess,
            &gov_token,
            &bob, // not bob
            String::from("PSP22::transfer_from"),
            Some(vec![bob.to_string(), dave.to_string(), 100_000_000_000_000_u128.to_string(), "[]".to_string()]),
            None,
            transcoder_governance_token(),
        )?;
        let sess=call_function(
            sess,
            &gov_token,
            &bob, // not bob
            String::from("PSP22::transfer_from"),
            Some(vec![bob.to_string(), ed.to_string(), 100_000_000_000_000_u128.to_string(), "[]".to_string()]),
            None,
            transcoder_governance_token(),
        )?;
        
        //sess.upload(bytes_governance_token()).expect("Session should upload token bytes");
        Ok(TestContext {
            sess,
            gov_token,
            gov_nft,
            stake_contract,            
            alice,
            bob,
            charlie,
            dave,
            ed,
        })
        
    }
    #[test]
    fn test_wrap() -> Result<(), Box<dyn Error>> {
        let ctx = setup().unwrap();
        // Bob approves Ed to transfer 1k sAZERO
        
        let mut sess = call_function(
            ctx.sess,
            &ctx.gov_token,
            &ctx.bob,
            String::from("PSP22::approve"),
            Some(vec![ctx.stake_contract.to_string(), 100_000_000_000_000_u128.to_string()]),
            None,
            transcoder_governance_token(),
        ).unwrap();

         
        let  sess = call_function(
            sess,
            &ctx.stake_contract,
            &ctx.bob,
            String::from("wrap_tokens"),
            Some(vec![100_u128.to_string()]),
            None,
            transcoder_governance_staking(),
        ).unwrap();


        Ok(())
    }
}