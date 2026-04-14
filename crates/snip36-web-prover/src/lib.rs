use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use snip36_core::proof::parse_proof_facts_json;
use snip36_core::rpc::{receipt_block_number, RpcError, StarknetRpc};
use snip36_core::signing::{compute_invoke_v3_tx_hash, felt_from_hex, sign, sign_and_build_payload};
use snip36_core::types::{ResourceBounds, SubmitParams, STRK_TOKEN};
use snip36_core::Config;
use starknet_types_core::felt::Felt;
use tokio::io::AsyncBufReadExt;

#[derive(Debug, Clone)]
pub struct ProveBlockArgs {
    pub reference_block: u64,
    pub account_address: String,
    pub private_key: String,
    pub chain_id: String,
    pub rpc_url: String,
    pub calldata_strs: Vec<String>,
    pub output_dir: PathBuf,
    pub artifact_stem: String,
    pub snip36_bin: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProveBlockResult {
    pub tx_json: serde_json::Value,
    pub tx_path: PathBuf,
    pub proof_path: PathBuf,
    pub proof_facts_path: PathBuf,
    pub messages_file: PathBuf,
    pub proof_size: u64,
    pub local_tx_hash: String,
    pub accepted_tx_hash: String,
    pub included_block: Option<u64>,
    pub receipt: serde_json::Value,
    pub logs: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum WebProverError {
    #[error("failed to parse calldata: {0}")]
    ParseCalldata(String),
    #[error("invalid account address: {0}")]
    InvalidAccountAddress(String),
    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),
    #[error("invalid chain_id: {0}")]
    InvalidChainId(String),
    #[error("failed to get nonce at block {block}: {source}")]
    NonceAtBlock { block: u64, source: RpcError },
    #[error("signing failed: {0}")]
    Signing(String),
    #[error("failed to create output dir: {0}")]
    CreateOutputDir(std::io::Error),
    #[error("failed to write tx JSON: {0}")]
    WriteTxJson(std::io::Error),
    #[error("failed to serialize tx JSON: {0}")]
    SerializeTxJson(serde_json::Error),
    #[error("failed to spawn prover ({bin}): {source}")]
    SpawnProver { bin: String, source: std::io::Error },
    #[error("proof generation failed")]
    ProofGenerationFailed,
    #[error("failed to read proof: {0}")]
    ReadProof(std::io::Error),
    #[error("failed to read proof_facts: {0}")]
    ReadProofFacts(std::io::Error),
    #[error("invalid proof_facts: {0}")]
    InvalidProofFacts(String),
    #[error("failed to parse proof_facts: {0}")]
    ParseProofFacts(String),
    #[error("rpc submission failed: {0}")]
    RpcSubmission(RpcError),
    #[error("rpc did not accept after all retries")]
    RpcRetriesExhausted,
    #[error("tx not confirmed: {0}")]
    TxNotConfirmed(RpcError),
}

pub async fn prove_block(
    rpc: &StarknetRpc,
    args: ProveBlockArgs,
) -> Result<ProveBlockResult, WebProverError> {
    let mut logs = Vec::new();

    let calldata_felts: Vec<Felt> = args
        .calldata_strs
        .iter()
        .map(|h| felt_from_hex(h).map_err(WebProverError::ParseCalldata))
        .collect::<Result<Vec<_>, _>>()?;

    let sender_felt = felt_from_hex(&args.account_address)
        .map_err(WebProverError::InvalidAccountAddress)?;
    let private_key_felt = felt_from_hex(&args.private_key)
        .map_err(WebProverError::InvalidPrivateKey)?;
    let chain_id = Config {
        rpc_url: args.rpc_url.clone(),
        account_address: args.account_address.clone(),
        private_key: args.private_key.clone(),
        chain_id: args.chain_id.clone(),
        gateway_url: None,
        project_dir: PathBuf::new(),
        output_dir: PathBuf::new(),
        deps_dir: PathBuf::new(),
    }
    .chain_id_felt()
    .map_err(|e| WebProverError::InvalidChainId(e.to_string()))?;
    let resource_bounds = ResourceBounds::default();

    let nonce = rpc
        .get_nonce_at_block(
            &args.account_address,
            serde_json::json!({"block_number": args.reference_block}),
        )
        .await
        .map_err(|source| WebProverError::NonceAtBlock {
            block: args.reference_block,
            source,
        })?;
    let nonce_felt = Felt::from(nonce);

    let standard_tx_hash = compute_invoke_v3_tx_hash(
        sender_felt,
        &calldata_felts,
        chain_id,
        nonce_felt,
        Felt::ZERO,
        &resource_bounds,
        &[],
        &[],
        0,
        0,
        &[],
    );

    let sig = sign(private_key_felt, standard_tx_hash)
        .map_err(|e| WebProverError::Signing(e.to_string()))?;

    let tx_json = serde_json::json!({
        "type": "INVOKE",
        "version": "0x3",
        "sender_address": &args.account_address,
        "calldata": args.calldata_strs,
        "nonce": format!("{:#x}", nonce),
        "resource_bounds": resource_bounds.to_rpc_json(),
        "tip": "0x0",
        "paymaster_data": [],
        "account_deployment_data": [],
        "nonce_data_availability_mode": "L1",
        "fee_data_availability_mode": "L1",
        "signature": [format!("{:#x}", sig.r), format!("{:#x}", sig.s)],
    });

    tokio::fs::create_dir_all(&args.output_dir)
        .await
        .map_err(WebProverError::CreateOutputDir)?;

    let tx_path = args.output_dir.join(format!("{}_tx.json", args.artifact_stem));
    tokio::fs::write(
        &tx_path,
        serde_json::to_string_pretty(&tx_json).map_err(WebProverError::SerializeTxJson)?,
    )
    .await
    .map_err(WebProverError::WriteTxJson)?;

    logs.push(format!(
        "Transaction constructed (nonce: {nonce}, ref block: {})",
        args.reference_block
    ));

    let proof_path = args.output_dir.join(format!("{}.proof", args.artifact_stem));
    let proof_facts_path = proof_path.with_extension("proof_facts");
    let messages_file = proof_path.with_extension("raw_messages.json");

    let prove_args = vec![
        "prove".to_string(),
        "virtual-os".to_string(),
        "--block-number".to_string(),
        args.reference_block.to_string(),
        "--tx-json".to_string(),
        tx_path.to_string_lossy().to_string(),
        "--rpc-url".to_string(),
        args.rpc_url.clone(),
        "--output".to_string(),
        proof_path.to_string_lossy().to_string(),
        "--strk-fee-token".to_string(),
        STRK_TOKEN.to_string(),
    ];

    let child = tokio::process::Command::new(&args.snip36_bin)
        .args(&prove_args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(source) => {
            return Err(WebProverError::SpawnProver {
                bin: args.snip36_bin.display().to_string(),
                source,
            })
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_handle = tokio::spawn(async move {
        let mut out = Vec::new();
        if let Some(stdout) = stdout {
            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    out.push(line);
                }
            }
        }
        out
    });

    let stderr_handle = tokio::spawn(async move {
        let mut out = Vec::new();
        if let Some(stderr) = stderr {
            let reader = tokio::io::BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    out.push(line);
                }
            }
        }
        out
    });

    let mut stdout_lines = stdout_handle.await.unwrap_or_default();
    let mut stderr_lines = stderr_handle.await.unwrap_or_default();
    logs.append(&mut stdout_lines);
    logs.append(&mut stderr_lines);

    let status = child.wait().await.map_err(|source| WebProverError::SpawnProver {
        bin: args.snip36_bin.display().to_string(),
        source,
    })?;

    if !status.success() || !proof_path.exists() {
        return Err(WebProverError::ProofGenerationFailed);
    }

    let proof_size = tokio::fs::metadata(&proof_path)
        .await
        .map(|m| m.len())
        .map_err(WebProverError::ReadProof)?;

    let proof_b64 = tokio::fs::read_to_string(&proof_path)
        .await
        .map_err(WebProverError::ReadProof)?
        .trim()
        .to_string();

    let proof_facts_str = tokio::fs::read_to_string(&proof_facts_path)
        .await
        .map_err(WebProverError::ReadProofFacts)?;

    let proof_facts_hex = parse_proof_facts_json(&proof_facts_str)
        .map_err(|e| WebProverError::InvalidProofFacts(e.to_string()))?;

    let proof_facts: Vec<Felt> = proof_facts_hex
        .iter()
        .map(|h| felt_from_hex(h).map_err(WebProverError::ParseProofFacts))
        .collect::<Result<Vec<_>, _>>()?;

    let params = SubmitParams {
        sender_address: sender_felt,
        private_key: private_key_felt,
        calldata: calldata_felts,
        proof_base64: proof_b64,
        proof_facts,
        nonce: nonce_felt,
        chain_id,
        resource_bounds: ResourceBounds::default(),
    };

    let (local_tx_hash, invoke_tx) = sign_and_build_payload(&params)
        .map_err(|e| WebProverError::Signing(e.to_string()))?;

    let local_tx_hash_hex = format!("{:#x}", local_tx_hash);
    let max_attempts = 20;
    let mut rpc_tx_hash = None;

    for attempt in 1..=max_attempts {
        match rpc.add_invoke_transaction(invoke_tx.clone()).await {
            Ok(accepted_tx_hash) => {
                logs.push(format!(
                    "RPC accepted (attempt {attempt}/{max_attempts}): {accepted_tx_hash}"
                ));
                rpc_tx_hash = Some(accepted_tx_hash);
                break;
            }
            Err(RpcError::JsonRpc(msg)) if attempt < max_attempts => {
                logs.push(format!(
                    "RPC error (attempt {attempt}/{max_attempts}), waiting 10s... ({msg})"
                ));
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
            Err(e) => return Err(WebProverError::RpcSubmission(e)),
        }
    }

    let Some(accepted_tx_hash) = rpc_tx_hash else {
        return Err(WebProverError::RpcRetriesExhausted);
    };

    let receipt = rpc
        .wait_for_tx(&accepted_tx_hash, 180, 5)
        .await
        .map_err(WebProverError::TxNotConfirmed)?;

    Ok(ProveBlockResult {
        tx_json,
        tx_path,
        proof_path,
        proof_facts_path,
        messages_file,
        proof_size,
        local_tx_hash: local_tx_hash_hex,
        accepted_tx_hash,
        included_block: receipt_block_number(&receipt),
        receipt,
        logs,
    })
}
