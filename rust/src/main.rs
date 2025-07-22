#![allow(unused)]
#![allow(clippy::uninlined_format_args)]
use bitcoin::hex::DisplayHex;
use bitcoincore_rpc::bitcoin::{Amount, Network};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::Deserialize;
use serde_json::json;
use std::fs::File;
use std::io::Write;

// Node access params
const RPC_URL: &str = "http://127.0.0.1:18443"; // Default regtest RPC port
const RPC_USER: &str = "alice";
const RPC_PASS: &str = "password";

// You can use calls not provided in RPC lib API using the generic `call` function.
fn send(rpc: &Client, addr: &str) -> bitcoincore_rpc::Result<String> {
    let args = [
        json!([{addr : 100 }]), // recipient address
        json!(null),            // conf target
        json!(null),            // estimate mode
        json!(null),            // fee rate in sats/vb
        json!(null),            // Empty option object
    ];

    #[derive(Deserialize)]
    struct SendResult {
        complete: bool,
        txid: String,
    }
    let send_result = rpc.call::<SendResult>("send", &args)?;
    assert!(send_result.complete);
    Ok(send_result.txid)
}

fn main() -> bitcoincore_rpc::Result<()> {
    // Connect to Bitcoin Core RPC
    let rpc = Client::new(
        RPC_URL,
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Get blockchain info
    let blockchain_info = rpc.get_blockchain_info()?;
    println!("Blockchain Info: {:?}", blockchain_info);

    // Create/Load the wallets, named 'Miner' and 'Trader'
    // For Miner wallet
    match rpc.create_wallet("Miner", None, None, None, None) {
        Ok(_) => {
            println!("Created Miner wallet");
        }
        Err(_) => {
            println!("Miner wallet already exists, attempting to load...");
            match rpc.load_wallet("Miner") {
                Ok(_) => println!("Loaded Miner wallet"),
                Err(e) => println!("Miner wallet load result: {:?}", e),
            }
        }
    };

    // For Trader wallet
    match rpc.create_wallet("Trader", None, None, None, None) {
        Ok(_) => {
            println!("Created Trader wallet");
        }
        Err(_) => {
            println!("Trader wallet already exists, attempting to load...");
            match rpc.load_wallet("Trader") {
                Ok(_) => println!("Loaded Trader wallet"),
                Err(e) => println!("Trader wallet load result: {:?}", e),
            }
        }
    };

    // Connect to specific wallet contexts
    let miner_rpc = Client::new(
        &format!("{}/wallet/Miner", RPC_URL),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    let trader_rpc = Client::new(
        &format!("{}/wallet/Trader", RPC_URL),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Generate one address from the Miner wallet with label "Mining Reward"
    let mining_address_unchecked = miner_rpc.get_new_address(Some("Mining Reward"), None)?;
    // Validate the address for regtest network
    let mining_address = mining_address_unchecked
        .require_network(Network::Regtest)
        .map_err(|e| {
            bitcoincore_rpc::Error::JsonRpc(bitcoincore_rpc::jsonrpc::Error::Transport(
                format!("Address validation error: {}", e).into(),
            ))
        })?;
    println!("Mining address: {}", mining_address);

    // Mine blocks until we get spendable balance
    // In Bitcoin, coinbase rewards need 100 confirmations to be spendable
    // So we need to mine at least 101 blocks to have spendable coins
    let initial_balance = miner_rpc.get_balance(None, None)?;
    println!("Initial Miner balance: {}", initial_balance);

    let mut blocks_mined = 0;
    loop {
        // Mine 10 blocks at a time to the mining address
        let _block_hashes = rpc.generate_to_address(10, &mining_address)?;
        blocks_mined += 10;

        let balance = miner_rpc.get_balance(None, None)?;
        println!("After {} blocks, Miner balance: {}", blocks_mined, balance);

        if balance > Amount::ZERO {
            break;
        }

        if blocks_mined >= 110 {
            break;
        }
    }

    // Print the balance of the Miner wallet
    let final_miner_balance = miner_rpc.get_balance(None, None)?;
    println!(
        "Final Miner wallet balance: {} BTC",
        final_miner_balance.to_btc()
    );

    /*
    Comment on wallet balance behavior:
    In Bitcoin, coinbase rewards (mining rewards) have a maturity period of 100 blocks.
    This means that newly mined coins cannot be spent until 100 blocks have been mined
    after the block containing the coinbase transaction. This is why we need to mine
    at least 101 blocks to see a positive spendable balance.
    */

    // Create receiving address from Trader wallet with label "Received"
    let trader_address_unchecked = trader_rpc.get_new_address(Some("Received"), None)?;
    // Validate the address for regtest network
    let trader_address = trader_address_unchecked
        .require_network(Network::Regtest)
        .map_err(|e| {
            bitcoincore_rpc::Error::JsonRpc(bitcoincore_rpc::jsonrpc::Error::Transport(
                format!("Address validation error: {}", e).into(),
            ))
        })?;
    println!("Trader receiving address: {}", trader_address);

    // Send 20 BTC from Miner to Trader
    let send_amount = Amount::from_btc(20.0)?;
    let txid = miner_rpc.send_to_address(
        &trader_address,
        send_amount,
        None, // comment
        None, // comment_to
        None, // subtract_fee_from_amount
        None, // replaceable
        None, // conf_target
        None, // estimate_mode
    )?;

    println!("Transaction sent with ID: {}", txid);

    // Fetch the unconfirmed transaction from mempool
    let mempool_entry =
        rpc.call::<serde_json::Value>("getmempoolentry", &[json!(txid.to_string())])?;
    println!(
        "Mempool entry: {}",
        serde_json::to_string_pretty(&mempool_entry)?
    );

    // Mine 1 block to confirm the transaction
    let confirmation_blocks = rpc.generate_to_address(1, &mining_address)?;
    let confirmation_block_hash = confirmation_blocks[0];
    println!(
        "Transaction confirmed in block: {}",
        confirmation_block_hash
    );

    // Extract transaction details
    let raw_tx_info = rpc.get_raw_transaction_info(&txid, Some(&confirmation_block_hash))?;
    let block_info = rpc.get_block_info(&confirmation_block_hash)?;
    let block_height = block_info.height;

    // Extract input details (from the first input)
    let first_input = &raw_tx_info.vin[0];
    let input_txid = first_input.txid.as_ref().unwrap();
    let input_vout = first_input.vout.unwrap();

    // Get the previous transaction to find input details
    let prev_tx_info = rpc.get_raw_transaction_info(input_txid, None)?;
    let input_output = &prev_tx_info.vout[input_vout as usize];
    let miner_input_amount_sats = input_output.value.to_sat();
    let miner_input_amount = input_output.value.to_btc();
    let miner_input_address = input_output
        .script_pub_key
        .address
        .as_ref()
        .unwrap()
        .clone()
        .require_network(Network::Regtest)
        .map_err(|e| {
            bitcoincore_rpc::Error::JsonRpc(bitcoincore_rpc::jsonrpc::Error::Transport(
                format!("Address validation error: {}", e).into(),
            ))
        })?
        .to_string();

    // Extract output details
    let mut trader_output_address = String::new();
    let mut trader_output_amount = 0.0;
    let mut trader_output_amount_sats = 0u64;
    let mut miner_change_address = String::new();
    let mut miner_change_amount = 0.0;
    let mut miner_change_amount_sats = 0u64;

    for output in &raw_tx_info.vout {
        if let Some(ref address) = output.script_pub_key.address {
            let addr_str = address
                .clone()
                .require_network(Network::Regtest)
                .map_err(|e| {
                    bitcoincore_rpc::Error::JsonRpc(bitcoincore_rpc::jsonrpc::Error::Transport(
                        format!("Address validation error: {}", e).into(),
                    ))
                })?
                .to_string();
            let amount = output.value.to_btc();
            let amount_sats = output.value.to_sat();

            // Check if this output goes to the trader (should be 20.0 BTC)
            if addr_str == trader_address.to_string() {
                trader_output_address = addr_str;
                trader_output_amount = amount;
                trader_output_amount_sats = amount_sats;
            } else {
                // This is the change output back to miner
                miner_change_address = addr_str;
                miner_change_amount = amount;
                miner_change_amount_sats = amount_sats;
            }
        }
    }

    // Calculate transaction fees using satoshis for precision
    let transaction_fees_sats =
        miner_input_amount_sats - trader_output_amount_sats - miner_change_amount_sats;
    let transaction_fees = Amount::from_sat(transaction_fees_sats).to_btc();

    // Write data to ../out.txt
    let mut file = File::create("../out.txt")?;
    writeln!(file, "{}", txid)?;
    writeln!(file, "{}", miner_input_address)?;
    writeln!(file, "{}", miner_input_amount)?;
    writeln!(file, "{}", trader_output_address)?;
    writeln!(file, "{}", trader_output_amount)?;
    writeln!(file, "{}", miner_change_address)?;
    writeln!(file, "{}", miner_change_amount)?;
    writeln!(file, "{}", transaction_fees)?;
    writeln!(file, "{}", block_height)?;
    writeln!(file, "{}", confirmation_block_hash)?;

    println!("Transaction details written to ../out.txt");
    println!("Program completed successfully!");

    Ok(())
}
