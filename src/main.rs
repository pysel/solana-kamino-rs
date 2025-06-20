use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::message::VersionedMessage;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use solana_transaction_status::UiTransactionEncoding;
use std::env;
use std::str::FromStr;

// Kamino Lend Program ID (same for mainnet and devnet)
const KAMINO_LEND_PROGRAM_ID: &str = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD";

const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

const FLASH_LOAN_DATA_BORROW_SIG: &[u8] = &[0x87, 0xe7, 0x34, 0xa7];
const FLASH_LOAN_TOKEN_ACCOUNT_KEY: usize = 4;

const BORROW_OBLIGATION_DATA_SIG: &[u8] = &[0xa1, 0x80, 0x8f, 0xf5];
const BORROW_OBLIGATION_TOKEN_ACCOUNT_KEY: usize = 5;


#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file
    dotenv::dotenv().ok();
    
    println!("🚀 Starting Kamino Lend Transaction Parser");
    println!("📋 Program ID: {}", KAMINO_LEND_PROGRAM_ID);
    
    // Initialize RPC client (using devnet for testing, change to mainnet as needed)
    let client = RpcClient::new_with_commitment(
        String::from(env::var("RPC_URL").unwrap()),
        CommitmentConfig::finalized(),
    );
    
    let program_id = Pubkey::from_str(KAMINO_LEND_PROGRAM_ID)?;    
    println!("🔍 Fetching recent transactions for Kamino Lend program...\n");
    // Get recent signatures for the program
    let successful_signatures = match client.get_signatures_for_address(&program_id).await {
        Ok(all_signatures) => {
            println!("📊 Found {} total recent transactions", all_signatures.len());
            // Filter out failed transactions - only keep successful ones
            let sucsigs: Vec<_> = all_signatures
                .iter()
                .filter(|sig_info| sig_info.err.is_none())
                .cloned()
                .collect::<Vec<_>>();
            let failed_count = all_signatures.len() - sucsigs.len();
            
            println!("✅ {} successful transactions", sucsigs.len());
            println!("❌ {} failed transactions (filtered out)", failed_count);
            println!("{}", "=".repeat(60));
            
            for (i, sig_info) in sucsigs.iter().enumerate() {
                println!("{}. Transaction Hash: {}", i + 1, sig_info.signature);
                println!("   Slot: {}", sig_info.slot);
                if let Some(block_time) = sig_info.block_time {
                    println!("   Time: {}", block_time);
                }
                println!("   Status: Success ✅");
                println!();
            }

            sucsigs
        }
        Err(e) => {
            eprintln!("❌ Error fetching signatures: {}", e);
            vec![]
        }
    };

    let block_time_first = successful_signatures[0].block_time.unwrap();
    let block_time_last = successful_signatures[successful_signatures.len() - 1].block_time.unwrap();
    let block_time_diff = block_time_first - block_time_last;

    let config = RpcTransactionConfig {
        commitment: CommitmentConfig::finalized().into(),
        encoding: UiTransactionEncoding::Base64.into(),
        max_supported_transaction_version: Some(0),
    };


    let sol_mint_key = Pubkey::from_str(SOL_MINT).unwrap();
    let usdc_mint_key = Pubkey::from_str(USDC_MINT).unwrap();
    let mut flash_loan_borrow_sol_amount: u64 = 0;
    let mut flash_loan_borrow_usdc_amount: u64 = 0;
    let mut loan_borrow_usdc_amount: u64 = 0;
    let mut loan_borrow_sol_amount: u64 = 0;

    let mut flash_loan_txs_count: u64 = 0;
    let mut loan_txs_count: u64 = 0;

    // Print just the successful hashes for easy copying
    println!("\n🔗 Successful transaction hashes only:");
    for (i, sig_info) in successful_signatures.iter().enumerate() {
        println!("Processing transaction {}/{}: {}", i + 1, successful_signatures.len(), sig_info.signature);
        
        let transaction = match client.get_transaction_with_config(&sig_info.signature.parse()?, config).await {
            Ok(tx) => tx,
            Err(e) => {
                println!("ERROR: Failed to get transaction {}: {}", sig_info.signature, e);
                continue;
            }
        };

        let versioned_tx = transaction.transaction.transaction.clone().decode().unwrap();

        if let VersionedMessage::V0(msg) = versioned_tx.message {
            let mut all_accounts: Vec<solana_sdk::pubkey::Pubkey> = Vec::new();
            
            // Add static accounts
            all_accounts.extend_from_slice(&msg.account_keys);
            
            // Collect all writable lookup accounts first
            let mut writable_lookup_accounts: Vec<solana_sdk::pubkey::Pubkey> = Vec::new();
            let mut readonly_lookup_accounts: Vec<solana_sdk::pubkey::Pubkey> = Vec::new();
            
            for (_, lookup) in msg.address_table_lookups.iter().enumerate() {
                // println!("  Fetching lookup table {}/{}: {}", lookup_idx + 1, msg.address_table_lookups.len(), lookup.account_key);
                match client.get_account(&lookup.account_key).await {
                    Ok(account_info) => {
                        // Parse lookup table data (skip 56-byte header)
                        if account_info.data.len() >= 56 {
                            let addresses_data = &account_info.data[56..];
                            let num_addresses = addresses_data.len() / 32;
                            
                            // Collect writable accounts from this lookup table
                            for &index in &lookup.writable_indexes {
                                if (index as usize) < num_addresses {
                                    let start = (index as usize) * 32;
                                    let end = start + 32;
                                    if end <= addresses_data.len() {
                                        let pubkey_bytes: [u8; 32] = addresses_data[start..end].try_into().unwrap();
                                        writable_lookup_accounts.push(solana_sdk::pubkey::Pubkey::new_from_array(pubkey_bytes));
                                    }
                                }
                            }
                            
                            // Collect readonly accounts from this lookup table
                            for &index in &lookup.readonly_indexes {
                                if (index as usize) < num_addresses {
                                    let start = (index as usize) * 32;
                                    let end = start + 32;
                                    if end <= addresses_data.len() {
                                        let pubkey_bytes: [u8; 32] = addresses_data[start..end].try_into().unwrap();
                                        readonly_lookup_accounts.push(solana_sdk::pubkey::Pubkey::new_from_array(pubkey_bytes));
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("  ERROR: Failed to fetch lookup table {}: {}", lookup.account_key, e);
                        println!("  Continuing without this lookup table...");
                    }
                }
            }
            
            // Add all writable lookup accounts
            all_accounts.extend(writable_lookup_accounts);
            
            // Add all readonly lookup accounts  
            all_accounts.extend(readonly_lookup_accounts);
            
            for instruction in msg.instructions {
                if instruction.data.starts_with(FLASH_LOAN_DATA_BORROW_SIG) {
                    let reserve_token_index: usize = instruction.accounts[FLASH_LOAN_TOKEN_ACCOUNT_KEY].into();
                    
                    let reserve_token = all_accounts[reserve_token_index];
                    let le_amount_bytes: [u8; 8] = instruction.data[8..].try_into().unwrap();
                    let amount = u64::from_le_bytes([le_amount_bytes[0], le_amount_bytes[1], le_amount_bytes[2], le_amount_bytes[3], le_amount_bytes[4], le_amount_bytes[5], le_amount_bytes[6], le_amount_bytes[7]]);

                    if reserve_token == usdc_mint_key {
                        flash_loan_borrow_usdc_amount += amount;
                        println!("Flash loan borrow USDC: {:?}", amount);
                    }

                    if reserve_token == sol_mint_key {
                        flash_loan_borrow_sol_amount += amount;
                        println!("Flash loan borrow SOL: {:?}", amount);
                    }

                    flash_loan_txs_count += 1;
                }

                if instruction.data.starts_with(BORROW_OBLIGATION_DATA_SIG) {
                    let reserve_token_index: usize = instruction.accounts[BORROW_OBLIGATION_TOKEN_ACCOUNT_KEY].into();
                    let reserve_token = all_accounts[reserve_token_index];

                    let le_amount_bytes: [u8; 8] = instruction.data[8..].try_into().unwrap();
                    let amount = u64::from_le_bytes([le_amount_bytes[0], le_amount_bytes[1], le_amount_bytes[2], le_amount_bytes[3], le_amount_bytes[4], le_amount_bytes[5], le_amount_bytes[6], le_amount_bytes[7]]);

                    if reserve_token == usdc_mint_key {
                        loan_borrow_usdc_amount += amount;
                        println!("Borrow obligation USDC: {:?}", amount);
                    }

                    if reserve_token == sol_mint_key {
                        loan_borrow_sol_amount += amount;
                        println!("Borrow obligation SOL: {:?}", amount);
                    }

                    loan_txs_count += 1;
                }
            }
        }
    }

    println!();
    println!("Breakdown for USDC and SOL loans on Kamino in {:?} latest blocks", block_time_diff);

    println!("Flash Loan Borrow USDC: {:?}", flash_loan_borrow_usdc_amount as f64 / 1e6);
    println!("Borrow obligation USDC: {:?}", loan_borrow_usdc_amount as f64 / 1e6);
    println!("Flash Loan Borrow SOL: {:?}", flash_loan_borrow_sol_amount as f64 / 1e9);
    println!("Borrow obligation SOL: {:?}", loan_borrow_sol_amount as f64 / 1e9);

    println!("Flash loan txs count: {:?}", flash_loan_txs_count);
    println!("Loan txs count: {:?}", loan_txs_count);
     
     // Display summary of instruction types found
     println!("\n{}", "=".repeat(60));
     println!("📊 KAMINO LEND INSTRUCTION SUMMARY");
     println!("{}", "=".repeat(60));
     
     Ok(())
}

/*
=== OUTLINE FOR ENHANCED KAMINO LEND PARSER ===

This is a basic outline. To build a comprehensive parser, you would need to:

1. TRANSACTION TYPE IDENTIFICATION:
   - Decode transaction instructions using the Kamino Lend program IDL
   - Identify instruction discriminators for:
     * Borrow operations
     * Repay operations  
     * Flash loan borrow
     * Flash loan repay
     * Deposit/Supply operations
     * Withdraw operations
     * Liquidation operations
     * Interest rate updates
     * Collateral management

2. INSTRUCTION PARSING:
   - Parse instruction data based on program interface
   - Extract relevant parameters (amounts, tokens, accounts)
   - Identify the specific lending pool involved
   - Extract user addresses and token amounts

3. ENHANCED FETCHING STRATEGIES:
   - Block-by-block scanning for historical data
   - WebSocket subscriptions for real-time monitoring
   - Pagination for large datasets
   - Multiple RPC endpoint support with failover

4. DATA STORAGE AND ANALYTICS:
   - Database integration for persistent storage
   - Transaction indexing and search capabilities
   - Analytics and reporting features
   - Rate limiting and caching

5. EXAMPLE ENHANCED STRUCTURES:

   pub struct KaminoTransaction {
       pub signature: String,
       pub slot: u64,
       pub block_time: Option<i64>,
       pub transaction_type: TransactionType,
       pub user: Pubkey,
       pub amount: u64,
       pub token_mint: Pubkey,
       pub lending_market: Pubkey,
       pub reserve: Pubkey,
   }

   pub enum TransactionType {
       Borrow { amount: u64, collateral_amount: u64 },
       Repay { amount: u64, interest_paid: u64 },
       FlashBorrow { amount: u64, fee: u64 },
       FlashRepay { amount: u64 },
       Deposit { amount: u64 },
       Withdraw { amount: u64 },
       Liquidate { 
           liquidated_amount: u64, 
           collateral_seized: u64,
           liquidator: Pubkey 
       },
   }

6. IMPLEMENTATION STEPS:
   a. Get the Kamino Lend program IDL/interface
   b. Create instruction decoder based on program schema
   c. Implement transaction type classification
   d. Add proper error handling and logging
   e. Add configuration for different networks
   f. Implement batch processing for efficiency
   g. Add filtering and search capabilities
   h. Create data export functionality

7. USEFUL KAMINO LEND RESOURCES:
   - GitHub: https://github.com/Kamino-Finance/klend
   - Program IDL and interface documentation
   - Solana Explorer for transaction examples
   - Kamino documentation and API references
*/
