use std::collections::HashSet;

use async_trait::async_trait;
use blockifier::blockifier::config::TransactionExecutorConfig;
use blockifier::blockifier::transaction_executor::{
    TransactionExecutionOutput,
    TransactionExecutor,
};
use blockifier::blockifier_versioned_constants::VersionedConstants;
use blockifier::bouncer::BouncerConfig;
use blockifier::context::{BlockContext, ChainInfo};
use blockifier::execution::contract_class::RunnableCompiledClass;
use blockifier::state::cached_state::{CachedState, StateMaps};
use blockifier::state::contract_class_manager::ContractClassManager;
use blockifier::state::global_cache::CompiledClasses;
use blockifier::state::state_api::{StateReader, StateResult};
use blockifier::state::state_reader_and_contract_manager::{
    FetchCompiledClasses,
    StateReaderAndContractManager,
};
use blockifier::transaction::account_transaction::ExecutionFlags;
use blockifier::transaction::transaction_execution::Transaction as BlockifierTransaction;
use serde::{Deserialize, Serialize};
use serde_json::json;
use starknet_api::block::{BlockHash, BlockInfo};
use starknet_api::block_hash::block_hash_calculator::{concat_counts, BlockHeaderCommitments};
use starknet_api::contract_class::SierraVersion;
use starknet_api::core::{ClassHash, CompiledClassHash, ContractAddress, Nonce};
use starknet_api::rpc_transaction::{RpcInvokeTransaction, RpcInvokeTransactionV3, RpcTransaction};
use starknet_api::state::StorageKey;
use starknet_api::transaction::fields::Fee;
use starknet_api::transaction::{InvokeTransaction, MessageToL1, Transaction, TransactionHash};
use starknet_api::versioned_constants_logic::VersionedConstantsTrait;
use starknet_api::StarknetApiError;
use starknet_types_core::felt::Felt;
use tracing::{error, warn};

use crate::errors::VirtualBlockExecutorError;
use crate::rpc_compat::{BlockHeader, BlockId, RpcStateReader};
use crate::running::serde_utils::deserialize_rpc_initial_reads;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcVirtualBlockExecutorConfig {
    #[serde(default)]
    pub(crate) prefetch_state: bool,
    #[serde(default)]
    pub(crate) bouncer_config: BouncerConfig,
    #[serde(default = "default_use_latest_versioned_constants")]
    pub(crate) use_latest_versioned_constants: bool,
}

fn default_use_latest_versioned_constants() -> bool {
    true
}

impl Default for RpcVirtualBlockExecutorConfig {
    fn default() -> Self {
        Self {
            prefetch_state: true,
            bouncer_config: BouncerConfig::default(),
            use_latest_versioned_constants: true,
        }
    }
}

pub(crate) struct BaseBlockInfo {
    pub(crate) block_context: BlockContext,
    pub(crate) base_block_hash: BlockHash,
    pub(crate) base_block_header_commitments: BlockHeaderCommitments,
    pub(crate) prev_base_block_hash: BlockHash,
}

impl BaseBlockInfo {
    pub(crate) fn new(
        header: BlockHeader,
        chain_info: ChainInfo,
        use_latest_versioned_constants: bool,
    ) -> Result<Self, VirtualBlockExecutorError> {
        let base_block_hash = header.block_hash;
        let prev_base_block_hash = header.parent_hash;
        let base_block_header_commitments = BlockHeaderCommitments {
            transaction_commitment: header.transaction_commitment,
            event_commitment: header.event_commitment,
            receipt_commitment: header.receipt_commitment,
            state_diff_commitment: header.state_diff_commitment,
            concatenated_counts: concat_counts(
                header.transaction_count,
                header.event_count,
                header.state_diff_length,
                header.l1_da_mode,
            ),
        };

        let block_info: BlockInfo = header.try_into().map_err(|e| {
            VirtualBlockExecutorError::TransactionExecutionError(format!(
                "Failed to convert block header to block info: {e}"
            ))
        })?;
        let mut versioned_constants = if use_latest_versioned_constants {
            VersionedConstants::latest_constants().clone()
        } else {
            VersionedConstants::get(&block_info.starknet_version)
                .map_err(|e| {
                    VirtualBlockExecutorError::TransactionExecutionError(format!(
                        "Failed to get versioned constants: {e}"
                    ))
                })?
                .clone()
        };
        versioned_constants.enable_casm_hash_migration = false;
        versioned_constants.min_sierra_version_for_sierra_gas = SierraVersion::new(0, 0, 1);

        let block_context = BlockContext::new(
            block_info,
            chain_info,
            versioned_constants,
            BouncerConfig::default(),
        );

        Ok(BaseBlockInfo {
            block_context,
            base_block_hash,
            base_block_header_commitments,
            prev_base_block_hash,
        })
    }
}

pub(crate) struct VirtualBlockExecutionData {
    pub(crate) execution_outputs: Vec<TransactionExecutionOutput>,
    pub(crate) initial_reads: StateMaps,
    pub(crate) state_diff: StateMaps,
    pub(crate) executed_class_hashes: HashSet<ClassHash>,
    pub(crate) l2_to_l1_messages: Vec<MessageToL1>,
    pub(crate) base_block_info: BaseBlockInfo,
}

#[async_trait(?Send)]
pub(crate) trait VirtualBlockExecutor {
    async fn execute(
        &self,
        block_id: BlockId,
        contract_class_manager: ContractClassManager,
        txs: Vec<(InvokeTransaction, TransactionHash)>,
    ) -> Result<VirtualBlockExecutionData, VirtualBlockExecutorError>;
}

/// State reader backed by prefetched `StateMaps` from simulate.
///
/// Serves storage, nonce, class hash, and declared contract reads from the prefetched state.
/// Falls back to the inner `RpcStateReader` when a key is missing — on wasm the sync
/// `RpcStateReader` methods return errors, so a cache miss propagates as a state read error.
pub(crate) struct SimulatedStateReader {
    state_maps: StateMaps,
    rpc_state_reader: RpcStateReader,
}

impl StateReader for SimulatedStateReader {
    fn get_storage_at(
        &self,
        contract_address: ContractAddress,
        key: StorageKey,
    ) -> StateResult<Felt> {
        match self.state_maps.storage.get(&(contract_address, key)) {
            Some(value) => Ok(*value),
            None => {
                warn!(
                    "Storage key not found in prefetched state, falling back to RPC \
                     (contract_address: {contract_address}, key: {key:?})."
                );
                self.rpc_state_reader.get_storage_at(contract_address, key)
            }
        }
    }

    fn get_nonce_at(&self, contract_address: ContractAddress) -> StateResult<Nonce> {
        match self.state_maps.nonces.get(&contract_address) {
            Some(value) => Ok(*value),
            None => {
                warn!(
                    "Nonce not found in prefetched state, falling back to RPC (contract_address: \
                     {contract_address})."
                );
                self.rpc_state_reader.get_nonce_at(contract_address)
            }
        }
    }

    fn get_class_hash_at(&self, contract_address: ContractAddress) -> StateResult<ClassHash> {
        match self.state_maps.class_hashes.get(&contract_address) {
            Some(value) => Ok(*value),
            None => {
                warn!(
                    "Class hash not found in prefetched state, falling back to RPC \
                     (contract_address: {contract_address})."
                );
                self.rpc_state_reader.get_class_hash_at(contract_address)
            }
        }
    }

    fn get_compiled_class(&self, class_hash: ClassHash) -> StateResult<RunnableCompiledClass> {
        self.rpc_state_reader.get_compiled_class(class_hash)
    }

    fn get_compiled_class_hash(&self, class_hash: ClassHash) -> StateResult<CompiledClassHash> {
        self.rpc_state_reader.get_compiled_class_hash(class_hash)
    }
}

impl FetchCompiledClasses for SimulatedStateReader {
    fn get_compiled_classes(&self, class_hash: ClassHash) -> StateResult<CompiledClasses> {
        self.rpc_state_reader.get_compiled_classes(class_hash)
    }

    fn is_declared(&self, class_hash: ClassHash) -> StateResult<bool> {
        match self.state_maps.declared_contracts.get(&class_hash) {
            Some(value) => Ok(*value),
            None => self.rpc_state_reader.is_declared(class_hash),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RpcVirtualBlockExecutor {
    pub(crate) rpc_state_reader: RpcStateReader,
    pub(crate) validate_txs: bool,
    pub(crate) config: RpcVirtualBlockExecutorConfig,
}

impl RpcVirtualBlockExecutor {
    pub(crate) fn new(
        node_url: String,
        chain_info: ChainInfo,
        block_id: BlockId,
        config: RpcVirtualBlockExecutorConfig,
    ) -> Self {
        Self {
            rpc_state_reader: RpcStateReader::new_with_config_from_url(
                node_url, chain_info, block_id,
            ),
            validate_txs: true,
            config,
        }
    }

    async fn simulate_and_get_initial_reads(
        &self,
        block_id: BlockId,
        txs: &[(InvokeTransaction, TransactionHash)],
    ) -> Result<StateMaps, VirtualBlockExecutorError> {
        let rpc_txs: Vec<RpcTransaction> = txs
            .iter()
            .map(|(tx, _)| match tx {
                InvokeTransaction::V3(v3) => RpcInvokeTransactionV3::try_from(v3.clone())
                    .map(RpcInvokeTransaction::V3)
                    .map(RpcTransaction::Invoke)
                    .map_err(|e: StarknetApiError| {
                        VirtualBlockExecutorError::TransactionExecutionError(e.to_string())
                    }),
                _ => Err(VirtualBlockExecutorError::TransactionExecutionError(
                    "Only Invoke V3 transactions are supported for simulate".to_string(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut simulation_flags = vec!["RETURN_INITIAL_READS"];
        if !self.validate_txs {
            simulation_flags.push("SKIP_VALIDATE");
        }
        simulation_flags.push("SKIP_FEE_CHARGE");

        let params = json!({
            "block_id": block_id,
            "transactions": rpc_txs,
            "simulation_flags": simulation_flags
        });

        let result = self
            .rpc_state_reader
            .send_rpc_request_async("starknet_simulateTransactions", params)
            .await
            .map_err(VirtualBlockExecutorError::ReexecutionError)?;

        let initial_reads_value = result.get("initial_reads").cloned().ok_or_else(|| {
            VirtualBlockExecutorError::TransactionExecutionError(
                "simulateTransactions response missing initial_reads (ensure RETURN_INITIAL_READS \
                 and v0.10 endpoint)"
                    .to_string(),
            )
        })?;

        deserialize_rpc_initial_reads(initial_reads_value).map_err(|e| {
            VirtualBlockExecutorError::TransactionExecutionError(format!(
                "Failed to deserialize initial_reads: {e}"
            ))
        })
    }

    async fn base_block_info_async(
        &self,
    ) -> Result<BaseBlockInfo, VirtualBlockExecutorError> {
        let block_header = self
            .rpc_state_reader
            .get_block_header_async()
            .await
            .map_err(VirtualBlockExecutorError::ReexecutionError)?;
        let mut base_block_info = BaseBlockInfo::new(
            block_header,
            self.rpc_state_reader.chain_info.clone(),
            self.config.use_latest_versioned_constants,
        )?;
        base_block_info.block_context.bouncer_config = self.config.bouncer_config.clone();
        Ok(base_block_info)
    }

    /// Pre-populates the `ContractClassManager` cache with every compiled class referenced in
    /// the prefetched state (via either `class_hashes` or `declared_contracts`).
    ///
    /// The sync blockifier executor cannot call the async RPC, so we must resolve every class
    /// it might need up-front. Missing any class causes execution to fail with a state-read error.
    async fn prefetch_classes_into_cache(
        &self,
        state_maps: &StateMaps,
        contract_class_manager: &ContractClassManager,
    ) -> Result<(), VirtualBlockExecutorError> {
        let mut class_hashes_to_fetch: HashSet<ClassHash> =
            state_maps.class_hashes.values().copied().collect();
        class_hashes_to_fetch.extend(state_maps.declared_contracts.keys().copied());

        for class_hash in class_hashes_to_fetch {
            if class_hash == ClassHash::default() {
                continue;
            }
            let compiled = self
                .rpc_state_reader
                .get_compiled_classes_async(class_hash)
                .await
                .map_err(VirtualBlockExecutorError::ReexecutionError)?;
            contract_class_manager.set_and_compile(class_hash, compiled);
        }

        Ok(())
    }

    fn convert_invoke_txs(
        &self,
        txs: Vec<(InvokeTransaction, TransactionHash)>,
    ) -> Result<Vec<BlockifierTransaction>, VirtualBlockExecutorError> {
        txs.into_iter()
            .map(|(invoke_tx, tx_hash)| {
                let execution_flags = ExecutionFlags {
                    only_query: false,
                    charge_fee: invoke_tx.resource_bounds().max_possible_fee(invoke_tx.tip())
                        > Fee(0),
                    validate: self.validate_txs,
                    strict_nonce_check: false,
                };

                BlockifierTransaction::from_api(
                    Transaction::Invoke(invoke_tx),
                    tx_hash,
                    None,
                    None,
                    None,
                    execution_flags,
                )
                .map_err(|e| VirtualBlockExecutorError::TransactionExecutionError(e.to_string()))
            })
            .collect()
    }
}

#[async_trait(?Send)]
impl VirtualBlockExecutor for RpcVirtualBlockExecutor {
    async fn execute(
        &self,
        block_id: BlockId,
        contract_class_manager: ContractClassManager,
        txs: Vec<(InvokeTransaction, TransactionHash)>,
    ) -> Result<VirtualBlockExecutionData, VirtualBlockExecutorError> {
        let base_block_info = self.base_block_info_async().await?;

        let state_maps = if self.config.prefetch_state {
            self.simulate_and_get_initial_reads(block_id, &txs).await?
        } else {
            StateMaps::default()
        };

        self.prefetch_classes_into_cache(&state_maps, &contract_class_manager).await?;

        let state_reader = SimulatedStateReader {
            state_maps,
            rpc_state_reader: self.rpc_state_reader.clone(),
        };

        let tx_hashes: Vec<TransactionHash> = txs.iter().map(|(_, h)| *h).collect();
        let blockifier_txs = self.convert_invoke_txs(txs)?;

        let state_reader_and_contract_manager =
            StateReaderAndContractManager::new(state_reader, contract_class_manager, None);

        let block_state = CachedState::new(state_reader_and_contract_manager);

        let mut transaction_executor = TransactionExecutor::new(
            block_state,
            base_block_info.block_context.clone(),
            TransactionExecutorConfig::default(),
        );

        let execution_results = transaction_executor.execute_txs(&blockifier_txs, None);

        let execution_outputs: Vec<TransactionExecutionOutput> = execution_results
            .into_iter()
            .map(|result| {
                result.map_err(|e| {
                    VirtualBlockExecutorError::TransactionExecutionError(e.to_string())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        for (output, tx_hash) in execution_outputs.iter().zip(tx_hashes.iter()) {
            if let Some(revert_error) = &output.0.revert_error {
                return Err(VirtualBlockExecutorError::TransactionReverted(
                    *tx_hash,
                    revert_error.to_string(),
                ));
            }
        }

        let block_state = transaction_executor
            .block_state
            .as_mut()
            .ok_or(VirtualBlockExecutorError::StateUnavailable)?;

        let initial_reads = block_state.get_initial_reads().map_err(|e| {
            VirtualBlockExecutorError::TransactionExecutionError(format!(
                "Failed to get initial reads: {e}"
            ))
        })?;

        let state_diff = block_state
            .to_state_diff()
            .map_err(|e| {
                VirtualBlockExecutorError::TransactionExecutionError(format!(
                    "Failed to get state diff: {e}"
                ))
            })?
            .state_maps;

        let executed_class_hashes = transaction_executor
            .bouncer
            .lock()
            .map_err(|e| {
                error!(
                    "Unexpected error: failed to acquire bouncer lock after transaction \
                     execution: {}",
                    e
                );
                VirtualBlockExecutorError::BouncerLockError(e.to_string())
            })?
            .get_executed_class_hashes();

        let mut l2_to_l1_messages = Vec::new();
        for (execution_info, _state_diff) in &execution_outputs {
            let messages: Vec<MessageToL1> = execution_info
                .non_optional_call_infos()
                .flat_map(|call_info| call_info.get_sorted_l2_to_l1_messages())
                .collect();
            l2_to_l1_messages.extend(messages);
        }

        Ok(VirtualBlockExecutionData {
            execution_outputs,
            base_block_info,
            initial_reads,
            state_diff,
            l2_to_l1_messages,
            executed_class_hashes,
        })
    }
}
