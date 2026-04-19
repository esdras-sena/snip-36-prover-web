use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use starknet_api::block::BlockNumber;
use starknet_api::core::ChainId;
use starknet_api::rpc_transaction::RpcTransaction;
use starknet_transaction_prover_wasm::config::ProverConfig;
use starknet_transaction_prover_wasm::proving::virtual_snos_prover::{
    ProveTransactionResult, VirtualSnosProver,
};
use starknet_transaction_prover_wasm::rpc_compat::BlockId;

#[derive(Debug, Clone)]
pub struct VirtualOsConfig {
    pub rpc_url: String,
    pub block_number: u64,
    pub strk_fee_token: Option<String>,
    pub chain_id: ChainId,
    pub transaction: RpcTransaction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualOsArtifacts {
    pub proof: String,
    pub proof_facts: serde_json::Value,
    pub l2_to_l1_messages: serde_json::Value,
}

pub async fn run_virtual_os(config: VirtualOsConfig) -> color_eyre::Result<VirtualOsArtifacts> {
    let VirtualOsConfig { rpc_url, block_number, strk_fee_token, chain_id, transaction } = config;

    let mut prover_config =
        ProverConfig { rpc_node_url: rpc_url, chain_id, ..ProverConfig::default() };

    if let Some(address) = strk_fee_token.as_deref() {
        prover_config.strk_fee_token_address = Some(address.parse()?);
    }

    let result = prove_transaction(&prover_config, block_number, transaction).await?;

    let proof = String::from_utf8(result.proof.0.to_vec())?.trim().to_string();
    let proof_facts = serde_json::to_value(&result.proof_facts)?;
    let l2_to_l1_messages = serde_json::to_value(&result.l2_to_l1_messages)?;

    Ok(VirtualOsArtifacts { proof, proof_facts, l2_to_l1_messages })
}

pub async fn prove_transaction(
    prover_config: &ProverConfig,
    block_number: u64,
    transaction: RpcTransaction,
) -> color_eyre::Result<ProveTransactionResult> {
    let prover = VirtualSnosProver::new(prover_config);
    prover
        .prove_transaction(BlockId::Number(BlockNumber(block_number)), transaction)
        .await
        .map_err(|e| color_eyre::eyre::eyre!(e.to_string()))
}

pub fn parse_transaction_json(value: Value) -> color_eyre::Result<RpcTransaction> {
    Ok(serde_json::from_value(value)?)
}

pub fn load_transaction_json(path: &Path) -> color_eyre::Result<RpcTransaction> {
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    parse_transaction_json(value)
}
