use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use snip36_core::rpc::{RpcError, StarknetRpc};
use snip36_core::signing::{chain_id_felt, compute_invoke_v3_tx_hash, felt_from_hex, sign};
use snip36_core::types::{ResourceBounds, STRK_TOKEN};
use snip36_virtual_os::{parse_transaction_json, run_virtual_os, VirtualOsConfig};
use starknet_api::core::ChainId;
use starknet_types_core::felt::Felt;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProveBlockArgs {
    pub reference_block: u64,
    pub account_address: String,
    pub private_key: String,
    pub chain_id: String,
    pub rpc_url: String,
    pub calldata_strs: Vec<String>,
    // pub output_dir: PathBuf,
    // pub artifact_stem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProveBlockResult {
    pub tx_json: serde_json::Value,
    pub proof: String,
    pub proof_facts: serde_json::Value,
    pub messages: serde_json::Value,
    pub tx_hash: String,
    pub signature: [String; 2],
}

#[derive(Debug, Clone)]
pub struct SubmitProofArgs {
    pub account_address: String,
    pub private_key: String,
    pub chain_id: String,
    pub calldata_strs: Vec<String>,
    pub proof_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitProofResult {
    pub proof_path: PathBuf,
    pub proof_facts_path: PathBuf,
    pub local_tx_hash: String,
    pub accepted_tx_hash: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WebProverError {
    #[error("failed to parse calldata: {0}")]
    ParseCalldata(String),
    #[error("invalid account address: {0}")]
    InvalidAccountAddress(String),
    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),
    #[error("failed to get nonce at block {block}: {source}")]
    NonceAtBlock { block: u64, source: RpcError },
    #[error("signing failed: {0}")]
    Signing(String),
    #[error("failed to run virtual os prover: {0}")]
    RunVirtualOs(String),
    #[error("failed to read proof file path {path}: {source}")]
    ReadProofFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("proof file does not exist: {0}")]
    MissingProofFile(PathBuf),
    #[error("rpc submission failed: {0}")]
    RpcSubmission(RpcError),
}

pub async fn prove_block(
    rpc: &StarknetRpc,
    args: ProveBlockArgs,
) -> Result<ProveBlockResult, WebProverError> {
    let _ = rpc;
    let calldata_felts: Vec<Felt> = args
        .calldata_strs
        .iter()
        .map(|h| felt_from_hex(h).map_err(WebProverError::ParseCalldata))
        .collect::<Result<Vec<_>, _>>()?;

    let sender_felt =
        felt_from_hex(&args.account_address).map_err(WebProverError::InvalidAccountAddress)?;
    let private_key_felt =
        felt_from_hex(&args.private_key).map_err(WebProverError::InvalidPrivateKey)?;
    let chain_id = args.chain_id.clone();
    let chain_id_felt = chain_id_felt(&chain_id);
    let resource_bounds = ResourceBounds::zero_fee();

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
        chain_id_felt,
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

    // tokio::fs::create_dir_all(&args.output_dir)
    //     .await
    //     .map_err(WebProverError::CreateOutputDir)?;

    // let tx_path = args
    //     .output_dir
    //     .join(format!("{}_tx.json", args.artifact_stem));
    // tokio::fs::write(
    //     &tx_path,
    //     serde_json::to_string_pretty(&tx_json).map_err(WebProverError::SerializeTxJson)?,
    // )
    // .await
    // .map_err(WebProverError::WriteTxJson)?;

    // let proof_path = args
    //     .output_dir
    //     .join(format!("{}.proof", args.artifact_stem));
    // let proof_facts_path = proof_path.with_extension("proof_facts");
    // let messages_file = proof_path.with_extension("raw_messages.json");

    let transaction = parse_transaction_json(tx_json.clone())
        .map_err(|e| WebProverError::RunVirtualOs(e.to_string()))?;

    let chain_id = ChainId::from(args.chain_id.clone());
    let artifacts = run_virtual_os(VirtualOsConfig {
        rpc_url: args.rpc_url.clone(),
        block_number: args.reference_block,
        strk_fee_token: Some(STRK_TOKEN.to_string()),
        chain_id,
        transaction,
    })
    .await
    .map_err(|e| WebProverError::RunVirtualOs(e.to_string()))?;

    // if !artifacts.proof_path.exists() {
    //     return Err(WebProverError::ProofGenerationFailed);
    // }

    // let proof_size = tokio::fs::metadata(&artifacts.proof_path)
    //     .await
    //     .map(|m| m.len())
    //     .map_err(WebProverError::ReadProof)?;

    let proof_b64 = artifacts.proof.trim().to_string();

    // let proof_facts_str = tokio::fs::read_to_string(&artifacts.proof_facts_path)
    //     .await
    //     .map_err(WebProverError::ReadProofFacts)?;

    // let proof_facts_hex = parse_proof_facts_json(&proof_facts_str)
    //     .map_err(|e| WebProverError::InvalidProofFacts(e.to_string()))?;

    // let proof_facts: Vec<Felt> = proof_facts_hex
    //     .iter()
    //     .map(|h| felt_from_hex(h).map_err(WebProverError::ParseProofFacts))
    //     .collect::<Result<Vec<_>, _>>()?;


    // let params = SubmitParams {
    //     sender_address: sender_felt,
    //     private_key: private_key_felt,
    //     calldata: calldata_felts,
    //     proof_base64: proof_b64,
    //     proof_facts,
    //     nonce: nonce_felt,
    //     chain_id,
    //     resource_bounds: ResourceBounds::default(),
    // };

    // let (local_tx_hash, invoke_tx) =
    //     sign_and_build_payload(&params).map_err(|e| WebProverError::Signing(e.to_string()))?;

    // let local_tx_hash_hex = format!("{:#x}", local_tx_hash);
    // let max_attempts = 20;
    // let mut rpc_tx_hash = None;

    // for attempt in 1..=max_attempts {
    //     match rpc.add_invoke_transaction(invoke_tx.clone()).await {
    //         Ok(accepted_tx_hash) => {
    //             logs.push(format!(
    //                 "RPC accepted (attempt {attempt}/{max_attempts}): {accepted_tx_hash}"
    //             ));
    //             rpc_tx_hash = Some(accepted_tx_hash);
    //             break;
    //         }
    //         Err(RpcError::JsonRpc(msg)) if attempt < max_attempts => {
    //             logs.push(format!(
    //                 "RPC error (attempt {attempt}/{max_attempts}), waiting 10s... ({msg})"
    //             ));
    //             tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    //         }
    //         Err(e) => return Err(WebProverError::RpcSubmission(e)),
    //     }
    // }

    // let Some(accepted_tx_hash) = rpc_tx_hash else {
    //     return Err(WebProverError::RpcRetriesExhausted);
    // };

    // let receipt = rpc
    //     .wait_for_tx(&accepted_tx_hash, 180, 5)
    //     .await
    //     .map_err(WebProverError::TxNotConfirmed)?;

    Ok(ProveBlockResult {
        tx_json,
        proof: proof_b64,
        proof_facts: artifacts.proof_facts,
        messages: artifacts.l2_to_l1_messages,
        tx_hash: format!("{:#x}", standard_tx_hash),
        signature: [format!("{:#x}", sig.r), format!("{:#x}", sig.s)],
    })
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = proveBlock)]
pub async fn prove_block_wasm(args: JsValue) -> Result<JsValue, JsError> {
    let args: ProveBlockArgs = serde_wasm_bindgen::from_value(args)
        .map_err(|e| JsError::new(&format!("invalid args: {e}")))?;
    let rpc = StarknetRpc::new(&args.rpc_url);
    let result = prove_block(&rpc, args)
        .await
        .map_err(|e| JsError::new(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsError::new(&e.to_string()))
}

// pub async fn submit_proof(
//     rpc: &StarknetRpc,
//     args: SubmitProofArgs,
// ) -> Result<SubmitProofResult, WebProverError> {
//     let proof_path = args.proof_path;
//     if !proof_path.exists() {
//         return Err(WebProverError::MissingProofFile(proof_path));
//     }

//     let calldata: Vec<Felt> = args
//         .calldata_strs
//         .iter()
//         .map(|h| felt_from_hex(h).map_err(WebProverError::ParseCalldata))
//         .collect::<Result<Vec<_>, _>>()?;

//     let sender_address =
//         felt_from_hex(&args.account_address).map_err(WebProverError::InvalidAccountAddress)?;
//     let private_key =
//         felt_from_hex(&args.private_key).map_err(WebProverError::InvalidPrivateKey)?;
//     let chain_id = Config {
//         rpc_url: String::new(),
//         account_address: args.account_address.clone(),
//         private_key: args.private_key.clone(),
//         chain_id: args.chain_id.clone(),
//         gateway_url: None,
//         project_dir: PathBuf::new(),
//         output_dir: PathBuf::new(),
//         deps_dir: PathBuf::new(),
//     }
//     .chain_id_felt()
//     .map_err(|e| WebProverError::InvalidChainId(e.to_string()))?;

//     let proof_base64 = tokio::fs::read_to_string(&proof_path)
//         .await
//         .map_err(|source| WebProverError::ReadProofFile {
//             path: proof_path.clone(),
//             source,
//         })?
//         .trim()
//         .to_string();

//     let proof_facts_path = proof_path.with_extension("proof_facts");
//     let proof_facts_str = tokio::fs::read_to_string(&proof_facts_path)
//         .await
//         .map_err(|source| WebProverError::ReadProofFile {
//             path: proof_facts_path.clone(),
//             source,
//         })?;

//     let proof_facts_hex = parse_proof_facts_json(&proof_facts_str)
//         .map_err(|e| WebProverError::InvalidProofFacts(e.to_string()))?;

//     let proof_facts: Vec<Felt> = proof_facts_hex
//         .iter()
//         .map(|h| felt_from_hex(h).map_err(WebProverError::ParseProofFacts))
//         .collect::<Result<Vec<_>, _>>()?;

//     let nonce = rpc
//         .get_nonce(&args.account_address)
//         .await
//         .map_err(WebProverError::RpcSubmission)?;

//     let params = SubmitParams {
//         sender_address,
//         private_key,
//         calldata,
//         proof_base64,
//         proof_facts,
//         nonce: Felt::from(nonce),
//         chain_id,
//         resource_bounds: ResourceBounds::default(),
//     };

//     let (local_tx_hash, invoke_tx) =
//         sign_and_build_payload(&params).map_err(|e| WebProverError::Signing(e.to_string()))?;

//     let local_tx_hash_hex = format!("{:#x}", local_tx_hash);
//     let accepted_tx_hash = rpc
//         .add_invoke_transaction(invoke_tx)
//         .await
//         .map_err(WebProverError::RpcSubmission)?;

//     Ok(SubmitProofResult {
//         proof_path,
//         proof_facts_path,
//         local_tx_hash: local_tx_hash_hex,
//         accepted_tx_hash,
//     })
// }
