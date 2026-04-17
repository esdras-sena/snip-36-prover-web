use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use starknet_api::core::ChainId;
use starknet_api::rpc_transaction::RpcTransaction;
use starknet_transaction_prover::config::ProverConfig;
use starknet_transaction_prover::proving::virtual_snos_prover::ProveTransactionResult;
use starknet_transaction_prover::server::config::ServiceConfig;
use starknet_transaction_prover::server::rpc_api::ProvingRpcServer;
use starknet_transaction_prover::server::rpc_impl::ProvingRpcServerImpl;

#[derive(Debug, Clone)]
pub struct VirtualOsConfig {
    pub rpc_url: String,
    pub block_number: u64,
    pub strk_fee_token: Option<String>,
    pub chain_id: ChainId,
    pub transaction: RpcTransaction,
}

// impl VirtualOsConfig {
//     pub fn proof_facts_output(&self) -> PathBuf {
//         proof_facts_path(&self.proof_output)
//     }

//     pub fn raw_messages_output(&self) -> PathBuf {
//         raw_messages_path(&self.proof_output)
//     }
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualOsArtifacts {
    pub proof: String,
    pub proof_facts: serde_json::Value,
    pub l2_to_l1_messages: serde_json::Value,
}

pub async fn run_virtual_os(config: VirtualOsConfig) -> color_eyre::Result<VirtualOsArtifacts> {
    let VirtualOsConfig {
        rpc_url,
        block_number,
        strk_fee_token,
        chain_id,
        transaction,
    } = config;

    let mut prover_config = ProverConfig {
        rpc_node_url: rpc_url,
        chain_id,
        ..ProverConfig::default()
    };

    if let Some(address) = strk_fee_token.as_deref() {
        prover_config.strk_fee_token_address = Some(address.parse()?);
    }

    let result = prove_transaction(&prover_config, block_number, transaction).await?;

    // if let Some(parent) = proof_output.parent() {
    //     std::fs::create_dir_all(parent)?;
    // }

    // std::fs::write(&proof_output, &*result.proof.0)?;
    // let proof_facts_path = proof_facts_path(&proof_output);
    // std::fs::write(
    //     &proof_facts_path,
    //     serde_json::to_vec_pretty(&result.proof_facts)?,
    // )?;

    // let raw_messages_path = raw_messages_path(&proof_output);
    // let raw_messages_json = serde_json::json!({
    //     "l2_to_l1_messages": result.l2_to_l1_messages,
    // });
    // std::fs::write(
    //     &raw_messages_path,
    //     serde_json::to_vec_pretty(&raw_messages_json)?,
    // )?;

    let proof = String::from_utf8(result.proof.0.to_vec())?.trim().to_string();
    let proof_facts = serde_json::to_value(&result.proof_facts)?;
    let l2_to_l1_messages = serde_json::to_value(&result.l2_to_l1_messages)?;

    Ok(VirtualOsArtifacts {
        proof,
        proof_facts,
        l2_to_l1_messages,
    })
}

pub async fn prove_transaction(
    prover_config: &ProverConfig,
    block_number: u64,
    transaction: RpcTransaction,
) -> color_eyre::Result<ProveTransactionResult> {
    let service = ProvingRpcServerImpl::from_config(&ServiceConfig {
        prover_config: prover_config.clone(),
        ip: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port: 0,
        max_concurrent_requests: 1,
        max_connections: 1,
        cors_allow_origin: Vec::new(),
        transport: starknet_transaction_prover::server::config::TransportMode::Http,
    });

    Ok(service
        .prove_transaction(
            blockifier_reexecution::state_reader::rpc_objects::BlockId::Number(
                starknet_api::block::BlockNumber(block_number),
            ),
            transaction,
        )
        .await
        .map_err(|e| color_eyre::eyre::eyre!(e.to_string()))?)
}

pub fn parse_transaction_json(value: Value) -> color_eyre::Result<RpcTransaction> {
    Ok(serde_json::from_value(value)?)
}

pub fn load_transaction_json(path: &Path) -> color_eyre::Result<RpcTransaction> {
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    parse_transaction_json(value)
}

// fn proof_facts_path(proof_path: &Path) -> PathBuf {
//     let s = proof_path.to_string_lossy();
//     if let Some(stripped) = s.strip_suffix(".proof") {
//         PathBuf::from(format!("{stripped}.proof_facts"))
//     } else {
//         PathBuf::from(format!("{}.proof_facts", s))
//     }
// }

// fn raw_messages_path(proof_path: &Path) -> PathBuf {
//     let s = proof_path.to_string_lossy();
//     if let Some(stripped) = s.strip_suffix(".proof") {
//         PathBuf::from(format!("{stripped}.raw_messages.json"))
//     } else {
//         PathBuf::from(format!("{}.raw_messages.json", s))
//     }
// }

