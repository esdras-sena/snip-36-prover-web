use blockifier::context::ChainInfo;
use blockifier::execution::contract_class::RunnableCompiledClass;
use blockifier::state::errors::StateError;
use blockifier::state::global_cache::CompiledClasses;
use blockifier::state::state_api::{StateReader, StateResult};
use blockifier::state::state_reader_and_contract_manager::FetchCompiledClasses;
use serde::Serialize;
use serde_json::{json, Value};
use starknet_api::block::BlockInfo;
use starknet_api::core::{ClassHash, CompiledClassHash, ContractAddress, Nonce};
use starknet_api::state::StorageKey;
use starknet_types_core::felt::Felt;
use url::Url;

use crate::errors::{ReexecutionError, ReexecutionResult};
use crate::rpc_objects::{
    BlockHeader, BlockId, GetBlockWithTxHashesParams, GetClassHashAtParams, GetNonceParams,
    GetStorageAtParams, RpcResponse, RPC_CLASS_HASH_NOT_FOUND, RPC_ERROR_BLOCK_NOT_FOUND,
    RPC_ERROR_CONTRACT_ADDRESS_NOT_FOUND, RPC_ERROR_INVALID_PARAMS,
    RPC_TRANSACTION_EXECUTION_ERROR,
};

#[derive(Clone)]
pub struct RpcStateReader {
    pub url: Url,
    pub block_id: BlockId,
    pub chain_info: ChainInfo,
}

impl RpcStateReader {
    pub fn new_with_config_from_url(node_url: String, chain_info: ChainInfo, block_id: BlockId) -> Self {
        Self { url: Url::parse(&node_url).expect("Invalid RPC node URL"), block_id, chain_info }
    }

    pub async fn send_rpc_request_async(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> ReexecutionResult<Value> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": method,
            "params": json!(params),
        });

        let response = reqwest::Client::new()
            .post(self.url.clone())
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ReexecutionError::RpcStatus(response.status().to_string()));
        }

        let rpc_response = response.json::<RpcResponse>().await.map_err(|e| {
            ReexecutionError::RpcParse(e.to_string())
        })?;

        match rpc_response {
            RpcResponse::Success(ok) => Ok(ok.result),
            RpcResponse::Error(err) => match err.error.code {
                RPC_ERROR_BLOCK_NOT_FOUND => Err(ReexecutionError::BlockNotFound),
                RPC_ERROR_CONTRACT_ADDRESS_NOT_FOUND => Err(ReexecutionError::ContractAddressNotFound),
                RPC_CLASS_HASH_NOT_FOUND => Err(ReexecutionError::ClassHashNotFound),
                RPC_TRANSACTION_EXECUTION_ERROR => Err(ReexecutionError::TransactionExecution(err.error.message)),
                RPC_ERROR_INVALID_PARAMS => Err(ReexecutionError::InvalidParams(err.error.message)),
                code => Err(ReexecutionError::UnexpectedRpc { code, message: err.error.message }),
            },
        }
    }

    pub fn get_block_header(&self) -> ReexecutionResult<BlockHeader> {
        #[cfg(target_arch = "wasm32")]
        {
            return Err(ReexecutionError::State(
                "blocking get_block_header is unavailable on wasm; use async RPC flow".to_string(),
            ));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let request_body = json!({
                "jsonrpc": "2.0",
                "id": 0,
                "method": "starknet_getBlockWithTxHashes",
                "params": json!(GetBlockWithTxHashesParams { block_id: self.block_id }),
            });
            let response = reqwest::blocking::Client::new()
                .post(self.url.clone())
                .header("Content-Type", "application/json")
                .json(&request_body)
                .send()
                .map_err(|e| ReexecutionError::RpcRequest(e.to_string()))?;
            if !response.status().is_success() {
                return Err(ReexecutionError::RpcStatus(response.status().to_string()));
            }
            let rpc_response = response
                .json::<RpcResponse>()
                .map_err(|e| ReexecutionError::RpcParse(e.to_string()))?;
            let value = match rpc_response {
                RpcResponse::Success(ok) => ok.result,
                RpcResponse::Error(err) => {
                    return Err(ReexecutionError::UnexpectedRpc {
                        code: err.error.code,
                        message: err.error.message,
                    })
                }
            };
            serde_json::from_value::<BlockHeader>(value).map_err(Into::into)
        }
    }

    pub fn get_block_info(&self) -> ReexecutionResult<BlockInfo> {
        self.get_block_header()?
            .try_into()
            .map_err(ReexecutionError::State)
    }
}

impl StateReader for RpcStateReader {
    fn get_storage_at(&self, _contract_address: ContractAddress, _key: StorageKey) -> StateResult<Felt> {
        Err(StateError::StateReadError("wasm rpc state reader storage access must use async path".into()))
    }

    fn get_nonce_at(&self, _contract_address: ContractAddress) -> StateResult<Nonce> {
        Err(StateError::StateReadError("wasm rpc state reader nonce access must use async path".into()))
    }

    fn get_class_hash_at(&self, _contract_address: ContractAddress) -> StateResult<ClassHash> {
        Err(StateError::StateReadError("wasm rpc state reader class hash access must use async path".into()))
    }

    fn get_compiled_class(&self, _class_hash: ClassHash) -> StateResult<RunnableCompiledClass> {
        Err(StateError::StateReadError("wasm rpc state reader compiled class access not implemented yet".into()))
    }

    fn get_compiled_class_hash(&self, _class_hash: ClassHash) -> StateResult<CompiledClassHash> {
        Err(StateError::StateReadError("wasm rpc state reader compiled class hash access not implemented yet".into()))
    }
}

impl FetchCompiledClasses for RpcStateReader {
    fn get_compiled_classes(&self, _class_hash: ClassHash) -> StateResult<CompiledClasses> {
        Err(StateError::StateReadError("wasm rpc state reader compiled classes access not implemented yet".into()))
    }

    fn is_declared(&self, _class_hash: ClassHash) -> StateResult<bool> {
        Err(StateError::StateReadError("wasm rpc state reader declaration access not implemented yet".into()))
    }
}
