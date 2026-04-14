use std::path::PathBuf;

use clap::Parser;
use color_eyre::Result;
use starknet_api::core::ChainId;
use tracing_subscriber::EnvFilter;

use snip36_virtual_os::{load_transaction_json, run_virtual_os, VirtualOsConfig};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    block_number: u64,
    #[arg(long)]
    tx_json: PathBuf,
    #[arg(long)]
    rpc_url: String,
    #[arg(long, default_value = "output/virtual_os.proof")]
    output: PathBuf,
    #[arg(long)]
    strk_fee_token: Option<String>,
    #[arg(long, default_value = "SN_SEPOLIA")]
    chain_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let chain_id = ChainId::from(cli.chain_id);
    let transaction = load_transaction_json(&cli.tx_json)?;

    let artifacts = run_virtual_os(VirtualOsConfig {
        rpc_url: cli.rpc_url,
        block_number: cli.block_number,
        proof_output: cli.output,
        strk_fee_token: cli.strk_fee_token,
        chain_id,
        transaction,
    })
    .await?;

    println!("Proof: {}", artifacts.proof_path.display());
    println!("Proof facts: {}", artifacts.proof_facts_path.display());
    println!("Raw messages: {}", artifacts.raw_messages_path.display());
    Ok(())
}
