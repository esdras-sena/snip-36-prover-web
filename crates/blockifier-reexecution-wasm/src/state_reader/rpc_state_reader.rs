use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use blockifier::context::ChainInfo;
use blockifier::execution::contract_class::{CompiledClassV0, CompiledClassV1, RunnableCompiledClass};
use blockifier::state::errors::StateError;
use blockifier::state::global_cache::CompiledClasses;
use blockifier::state::state_api::{StateReader, StateResult};
use blockifier::state::state_reader_and_contract_manager::FetchCompiledClasses;
use flate2::read::GzDecoder;
use serde::Serialize;
use serde_json::{json, Value};
use starknet_api::block::BlockInfo;
use starknet_api::contract_class::{EntryPointType, SierraVersion};
use starknet_api::core::{ClassHash, CompiledClassHash, ContractAddress, EntryPointSelector, Nonce};
use starknet_api::deprecated_contract_class::{
    ContractClass as DeprecatedContractClass, EntryPointOffset, EntryPointV0, Program,
};
use starknet_api::state::{SierraContractClass, StorageKey};
use starknet_core::types::{
    CompressedLegacyContractClass, ContractClass as StarknetContractClass,
    LegacyContractEntryPoint, LegacyEntryPointsByType,
};
use starknet_types_core::felt::Felt;

use crate::errors::{ReexecutionError, ReexecutionResult};
use crate::state_reader::config::RpcStateReaderConfig;
use crate::state_reader::rpc_objects::{
    BlockHeader, BlockId, GetBlockWithTxHashesParams, GetClassHashAtParams, GetCompiledClassParams,
    GetNonceParams, GetStorageAtParams, RpcResponse, RPC_CLASS_HASH_NOT_FOUND,
    RPC_ERROR_BLOCK_NOT_FOUND, RPC_ERROR_CONTRACT_ADDRESS_NOT_FOUND, RPC_ERROR_INVALID_PARAMS,
    RPC_TRANSACTION_EXECUTION_ERROR,
};

#[derive(Clone)]
pub struct RpcStateReader {
    pub config: RpcStateReaderConfig,
    pub block_id: BlockId,
    pub chain_info: ChainInfo,
}

impl RpcStateReader {
    pub fn new_with_config_from_url(node_url: String, chain_info: ChainInfo, block_id: BlockId) -> Self {
        Self { config: RpcStateReaderConfig::from_url(node_url), block_id, chain_info }
    }

    pub async fn send_rpc_request_async(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> ReexecutionResult<Value> {
        let request_body = json!({
            "jsonrpc": self.config.json_rpc_version,
            "id": 0,
            "method": method,
            "params": json!(params),
        });

        let response = reqwest::Client::new()
            .post(&self.config.url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ReexecutionError::RpcStatus(response.status().to_string()));
        }

        let rpc_response = response
            .json::<RpcResponse>()
            .await
            .map_err(|e| ReexecutionError::RpcParse(e.to_string()))?;

        match rpc_response {
            RpcResponse::Success(ok) => Ok(ok.result),
            RpcResponse::Error(err) => match err.error.code {
                RPC_ERROR_BLOCK_NOT_FOUND => Err(ReexecutionError::BlockNotFound),
                RPC_ERROR_CONTRACT_ADDRESS_NOT_FOUND => Err(ReexecutionError::ContractAddressNotFound),
                RPC_CLASS_HASH_NOT_FOUND => Err(ReexecutionError::ClassHashNotFound),
                RPC_TRANSACTION_EXECUTION_ERROR => {
                    Err(ReexecutionError::TransactionExecution(err.error.message))
                }
                RPC_ERROR_INVALID_PARAMS => Err(ReexecutionError::InvalidParams(err.error.message)),
                code => Err(ReexecutionError::UnexpectedRpc { code, message: err.error.message }),
            },
        }
    }

    pub fn get_block_header(&self) -> ReexecutionResult<BlockHeader> {
        Err(ReexecutionError::State(
            "blocking rpc_state_reader::get_block_header is not browser-safe; wasm callers must use async flow"
                .to_string(),
        ))
    }

    pub async fn get_block_header_async(&self) -> ReexecutionResult<BlockHeader> {
        let json = self
            .send_rpc_request_async(
                "starknet_getBlockWithTxHashes",
                GetBlockWithTxHashesParams { block_id: self.block_id },
            )
            .await?;
        Ok(serde_json::from_value::<BlockHeader>(json)?)
    }

    pub async fn get_block_info_async(&self) -> ReexecutionResult<BlockInfo> {
        self.get_block_header_async()
            .await?
            .try_into()
            .map_err(ReexecutionError::State)
    }

    pub async fn get_nonce_at_async(
        &self,
        contract_address: ContractAddress,
    ) -> ReexecutionResult<Nonce> {
        let params = GetNonceParams { block_id: self.block_id, contract_address };
        match self.send_rpc_request_async("starknet_getNonce", params).await {
            Ok(value) => Ok(serde_json::from_value(value)?),
            Err(ReexecutionError::ContractAddressNotFound) => Ok(Nonce::default()),
            Err(e) => Err(e),
        }
    }

    pub async fn get_storage_at_async(
        &self,
        contract_address: ContractAddress,
        key: StorageKey,
    ) -> ReexecutionResult<Felt> {
        let params = GetStorageAtParams { block_id: self.block_id, contract_address, key };
        match self.send_rpc_request_async("starknet_getStorageAt", params).await {
            Ok(value) => Ok(serde_json::from_value(value)?),
            Err(ReexecutionError::ContractAddressNotFound) => Ok(Felt::default()),
            Err(e) => Err(e),
        }
    }

    pub async fn get_class_hash_at_async(
        &self,
        contract_address: ContractAddress,
    ) -> ReexecutionResult<ClassHash> {
        let params = GetClassHashAtParams { contract_address, block_id: self.block_id };
        match self.send_rpc_request_async("starknet_getClassHashAt", params).await {
            Ok(value) => Ok(serde_json::from_value(value)?),
            Err(ReexecutionError::ContractAddressNotFound) => Ok(ClassHash::default()),
            Err(e) => Err(e),
        }
    }

    pub async fn get_contract_class_async(
        &self,
        class_hash: &ClassHash,
    ) -> ReexecutionResult<StarknetContractClass> {
        let params = json!({
            "block_id": self.block_id,
            "class_hash": class_hash.0.to_hex_string(),
        });
        let raw = self
            .send_rpc_request_async("starknet_getClass", params)
            .await
            .map_err(|e| match e {
                ReexecutionError::ClassHashNotFound => {
                    ReexecutionError::ClassHashNotFound
                }
                other => other,
            })?;
        serde_json::from_value(raw).map_err(|e| ReexecutionError::RpcParse(e.to_string()))
    }

    pub async fn get_runnable_compiled_class_async(
        &self,
        class_hash: ClassHash,
    ) -> ReexecutionResult<RunnableCompiledClass> {
        let contract_class = self.get_contract_class_async(&class_hash).await?;
        match contract_class {
            StarknetContractClass::Sierra(sierra) => {
                let sierra_api = SierraContractClass::from(sierra);
                let sierra_version =
                    SierraVersion::extract_from_program(&sierra_api.sierra_program).map_err(
                        |e| ReexecutionError::CompiledClass(format!("extract sierra version: {e}")),
                    )?;
                let params = GetCompiledClassParams { class_hash, block_id: self.block_id };
                let casm_json =
                    self.send_rpc_request_async("starknet_getCompiledCasm", params).await?;
                let compiled = CompiledClassV1::try_from_json_string(
                    &serde_json::to_string(&casm_json)?,
                    sierra_version,
                )
                .map_err(|e| ReexecutionError::CompiledClass(e.to_string()))?;
                Ok(RunnableCompiledClass::V1(compiled))
            }
            StarknetContractClass::Legacy(legacy) => {
                let compiled = legacy_to_compiled_class_v0(legacy)?;
                Ok(RunnableCompiledClass::V0(compiled))
            }
        }
    }

    pub async fn get_compiled_classes_async(
        &self,
        class_hash: ClassHash,
    ) -> ReexecutionResult<CompiledClasses> {
        let contract_class = self.get_contract_class_async(&class_hash).await?;
        match contract_class {
            StarknetContractClass::Sierra(sierra) => {
                let sierra_api = SierraContractClass::from(sierra);
                let sierra_version =
                    SierraVersion::extract_from_program(&sierra_api.sierra_program).map_err(
                        |e| ReexecutionError::CompiledClass(format!("extract sierra version: {e}")),
                    )?;
                let params = GetCompiledClassParams { class_hash, block_id: self.block_id };
                let casm_json =
                    self.send_rpc_request_async("starknet_getCompiledCasm", params).await?;
                let compiled = CompiledClassV1::try_from_json_string(
                    &serde_json::to_string(&casm_json)?,
                    sierra_version,
                )
                .map_err(|e| ReexecutionError::CompiledClass(e.to_string()))?;
                Ok(CompiledClasses::V1(compiled, Arc::new(sierra_api)))
            }
            StarknetContractClass::Legacy(legacy) => {
                let compiled = legacy_to_compiled_class_v0(legacy)?;
                Ok(CompiledClasses::V0(compiled))
            }
        }
    }

    pub async fn is_declared_async(&self, class_hash: ClassHash) -> ReexecutionResult<bool> {
        match self.get_contract_class_async(&class_hash).await {
            Ok(_) => Ok(true),
            Err(ReexecutionError::ClassHashNotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

impl StateReader for RpcStateReader {
    fn get_storage_at(&self, _contract_address: ContractAddress, _key: StorageKey) -> StateResult<Felt> {
        Err(StateError::StateReadError(
            "browser-safe rpc reader does not expose blocking storage access; use get_storage_at_async"
                .into(),
        ))
    }

    fn get_nonce_at(&self, _contract_address: ContractAddress) -> StateResult<Nonce> {
        Err(StateError::StateReadError(
            "browser-safe rpc reader does not expose blocking nonce access; use get_nonce_at_async"
                .into(),
        ))
    }

    fn get_class_hash_at(&self, _contract_address: ContractAddress) -> StateResult<ClassHash> {
        Err(StateError::StateReadError(
            "browser-safe rpc reader does not expose blocking class hash access; use get_class_hash_at_async"
                .into(),
        ))
    }

    fn get_compiled_class(&self, _class_hash: ClassHash) -> StateResult<RunnableCompiledClass> {
        Err(StateError::StateReadError(
            "browser-safe rpc reader does not expose blocking compiled class access; use get_runnable_compiled_class_async"
                .into(),
        ))
    }

    fn get_compiled_class_hash(&self, _class_hash: ClassHash) -> StateResult<CompiledClassHash> {
        Err(StateError::StateReadError(
            "browser-safe rpc reader does not support get_compiled_class_hash".into(),
        ))
    }
}

impl FetchCompiledClasses for RpcStateReader {
    fn get_compiled_classes(&self, _class_hash: ClassHash) -> StateResult<CompiledClasses> {
        Err(StateError::StateReadError(
            "browser-safe rpc reader does not expose blocking compiled classes access; use get_compiled_classes_async"
                .into(),
        ))
    }

    fn is_declared(&self, _class_hash: ClassHash) -> StateResult<bool> {
        // In the virtual OS execution context, all classes referenced are assumed declared.
        // Use is_declared_async for an actual RPC check.
        Ok(true)
    }
}

fn legacy_to_compiled_class_v0(
    legacy: CompressedLegacyContractClass,
) -> ReexecutionResult<CompiledClassV0> {
    let mut decoder = GzDecoder::new(&legacy.program[..]);
    let mut program_json = String::new();
    decoder.read_to_string(&mut program_json).map_err(|e| {
        ReexecutionError::CompiledClass(format!("decompress legacy program: {e}"))
    })?;
    let program: Program = serde_json::from_str(&program_json).map_err(|e| {
        ReexecutionError::CompiledClass(format!("parse legacy program: {e}"))
    })?;
    let entry_points_by_type = map_legacy_entry_points(legacy.entry_points_by_type);
    let deprecated_class =
        DeprecatedContractClass { program, entry_points_by_type, abi: None };
    CompiledClassV0::try_from(deprecated_class)
        .map_err(|e| ReexecutionError::CompiledClass(e.to_string()))
}

fn map_legacy_entry_points(
    entry_points: LegacyEntryPointsByType,
) -> HashMap<EntryPointType, Vec<EntryPointV0>> {
    let to_v0 = |ep: &LegacyContractEntryPoint| EntryPointV0 {
        offset: EntryPointOffset(
            usize::try_from(ep.offset).expect("entry point offset overflow"),
        ),
        selector: EntryPointSelector(
            Felt::from_bytes_be(&ep.selector.to_bytes_be()),
        ),
    };
    HashMap::from([
        (EntryPointType::Constructor, entry_points.constructor.iter().map(to_v0).collect()),
        (EntryPointType::External, entry_points.external.iter().map(to_v0).collect()),
        (EntryPointType::L1Handler, entry_points.l1_handler.iter().map(to_v0).collect()),
    ])
}
