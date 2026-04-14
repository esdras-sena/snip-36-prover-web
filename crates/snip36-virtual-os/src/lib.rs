use std::path::{Path, PathBuf};

use blockifier_reexecution::state_reader::rpc_objects::BlockId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use starknet_api::core::ChainId;
use starknet_api::rpc_transaction::RpcTransaction;
use starknet_transaction_prover::config::ProverConfig;
use starknet_transaction_prover::proving::virtual_snos_prover::{
    ProveTransactionResult, RpcVirtualSnosProver,
};

#[derive(Debug, Clone)]
pub struct VirtualOsConfig {
    pub rpc_url: String,
    pub block_number: u64,
    pub proof_output: PathBuf,
    pub strk_fee_token: Option<String>,
    pub chain_id: ChainId,
    pub transaction: RpcTransaction,
}

impl VirtualOsConfig {
    pub fn proof_facts_output(&self) -> PathBuf {
        proof_facts_path(&self.proof_output)
    }

    pub fn raw_messages_output(&self) -> PathBuf {
        raw_messages_path(&self.proof_output)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualOsArtifacts {
    pub proof_path: PathBuf,
    pub proof_facts_path: PathBuf,
    pub raw_messages_path: PathBuf,
    pub result: ProveTransactionResult,
}

pub async fn run_virtual_os(config: VirtualOsConfig) -> color_eyre::Result<VirtualOsArtifacts> {
    let mut prover_config = ProverConfig {
        rpc_node_url: config.rpc_url.clone(),
        chain_id: config.chain_id,
        ..ProverConfig::default()
    };

    if let Some(address) = config.strk_fee_token.as_deref() {
        prover_config.strk_fee_token_address = Some(address.parse()?);
    }

    let prover = RpcVirtualSnosProver::new(&prover_config);
    let result = prover
        .prove_transaction(
            BlockId::Number(config.block_number.into()),
            config.transaction,
        )
        .await?;

    if let Some(parent) = config.proof_output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&config.proof_output, &result.proof)?;
    let proof_facts_path = config.proof_facts_output();
    std::fs::write(
        &proof_facts_path,
        serde_json::to_vec_pretty(&result.proof_facts)?,
    )?;

    let raw_messages_path = config.raw_messages_output();
    let raw_messages_json = serde_json::json!({
        "l2_to_l1_messages": result.l2_to_l1_messages,
    });
    std::fs::write(
        &raw_messages_path,
        serde_json::to_vec_pretty(&raw_messages_json)?,
    )?;

    Ok(VirtualOsArtifacts {
        proof_path: config.proof_output,
        proof_facts_path,
        raw_messages_path,
        result,
    })
}

pub fn parse_transaction_json(value: Value) -> color_eyre::Result<RpcTransaction> {
    Ok(serde_json::from_value(value)?)
}

pub fn load_transaction_json(path: &Path) -> color_eyre::Result<RpcTransaction> {
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    parse_transaction_json(value)
}

fn proof_facts_path(proof_path: &Path) -> PathBuf {
    let s = proof_path.to_string_lossy();
    if let Some(stripped) = s.strip_suffix(".proof") {
        PathBuf::from(format!("{stripped}.proof_facts"))
    } else {
        PathBuf::from(format!("{}.proof_facts", s))
    }
}

fn raw_messages_path(proof_path: &Path) -> PathBuf {
    let s = proof_path.to_string_lossy();
    if let Some(stripped) = s.strip_suffix(".proof") {
        PathBuf::from(format!("{stripped}.raw_messages.json"))
    } else {
        PathBuf::from(format!("{}.raw_messages.json", s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_paths_follow_expected_suffixes() {
        assert_eq!(
            proof_facts_path(Path::new("output/virtual_os.proof")),
            PathBuf::from("output/virtual_os.proof_facts")
        );
        assert_eq!(
            proof_facts_path(Path::new("output/virtual_os")),
            PathBuf::from("output/virtual_os.proof_facts")
        );
        assert_eq!(
            raw_messages_path(Path::new("output/virtual_os.proof")),
            PathBuf::from("output/virtual_os.raw_messages.json")
        );
        assert_eq!(
            raw_messages_path(Path::new("output/virtual_os")),
            PathBuf::from("output/virtual_os.raw_messages.json")
        );
    }
}
