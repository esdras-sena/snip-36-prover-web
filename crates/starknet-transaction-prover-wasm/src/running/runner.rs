use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use blockifier::context::ChainInfo;
use blockifier::state::cached_state::StateMaps;
use blockifier::state::contract_class_manager::ContractClassManager;
use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;
use cairo_vm::vm::runners::cairo_pie::CairoPie;
use serde::{Deserialize, Serialize};
use shared_execution_objects::central_objects::CentralTransactionExecutionInfo;
use starknet_api::block::{BlockHash, BlockInfo};
use starknet_api::block_hash::block_hash_calculator::BlockHeaderCommitments;
use starknet_api::core::{ChainId, CompiledClassHash, ContractAddress, OsChainInfo};
use starknet_api::transaction::{
    InvokeTransaction,
    MessageToL1,
    TransactionHash,
    TransactionHasher,
};
use starknet_os::commitment_infos::CommitmentInfo;
use starknet_os::io::os_input::{OsBlockInput, OsHints, OsHintsConfig, StarknetOsInput};
use starknet_os::runner::run_virtual_os;
use tracing::field::display;
use tracing::{info, Span};
use url::Url;

use crate::errors::RunnerError;
use crate::rpc_compat::BlockId;
use crate::running::classes_provider::{ClassesProvider, WasmClassesProvider};
use crate::running::storage_proofs::{
    RpcStorageProofsProvider,
    StorageProofConfig,
    StorageProofProvider,
};
use crate::running::virtual_block_executor::{
    RpcVirtualBlockExecutor,
    RpcVirtualBlockExecutorConfig,
    VirtualBlockExecutionData,
    VirtualBlockExecutor,
};
use crate::rpc_compat::RpcStateReader;

pub(crate) struct VirtualOsBlockInput {
    contract_state_commitment_info: CommitmentInfo,
    address_to_storage_commitment_info: HashMap<ContractAddress, CommitmentInfo>,
    contract_class_commitment_info: CommitmentInfo,
    chain_info: OsChainInfo,
    transactions: Vec<(InvokeTransaction, TransactionHash)>,
    tx_execution_infos: Vec<CentralTransactionExecutionInfo>,
    block_info: BlockInfo,
    initial_reads: StateMaps,
    base_block_hash: BlockHash,
    base_block_header_commitments: BlockHeaderCommitments,
    prev_base_block_hash: BlockHash,
    compiled_classes: BTreeMap<CompiledClassHash, CasmContractClass>,
}

impl From<VirtualOsBlockInput> for OsHints {
    fn from(virtual_os_block_input: VirtualOsBlockInput) -> Self {
        let os_block_input = OsBlockInput {
            block_hash_commitments: virtual_os_block_input.base_block_header_commitments,
            contract_state_commitment_info: virtual_os_block_input.contract_state_commitment_info,
            address_to_storage_commitment_info: virtual_os_block_input
                .address_to_storage_commitment_info,
            contract_class_commitment_info: virtual_os_block_input.contract_class_commitment_info,
            transactions: virtual_os_block_input
                .transactions
                .into_iter()
                .map(|(invoke_tx, tx_hash)| {
                    starknet_api::executable_transaction::Transaction::Account(
                        starknet_api::executable_transaction::AccountTransaction::Invoke(
                            starknet_api::executable_transaction::InvokeTransaction {
                                tx: invoke_tx,
                                tx_hash,
                            },
                        ),
                    )
                })
                .collect(),
            tx_execution_infos: virtual_os_block_input.tx_execution_infos,
            prev_block_hash: virtual_os_block_input.prev_base_block_hash,
            block_info: virtual_os_block_input.block_info,
            initial_reads: virtual_os_block_input.initial_reads,
            declared_class_hash_to_component_hashes: HashMap::new(),
            new_block_hash: virtual_os_block_input.base_block_hash,
            old_block_number_and_hash: None,
            class_hashes_to_migrate: Vec::new(),
        };

        let os_input = StarknetOsInput {
            os_block_inputs: vec![os_block_input],
            deprecated_compiled_classes: BTreeMap::new(),
            compiled_classes: virtual_os_block_input.compiled_classes,
        };

        OsHints {
            os_input,
            os_hints_config: OsHintsConfig {
                debug_mode: false,
                full_output: false,
                use_kzg_da: false,
                chain_info: virtual_os_block_input.chain_info,
                public_keys: None,
                rng_seed_salt: None,
            },
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct RunnerConfig {
    #[serde(default)]
    pub(crate) storage_proof_config: StorageProofConfig,
    #[serde(default)]
    pub(crate) virtual_block_executor_config: RpcVirtualBlockExecutorConfig,
}

pub struct RunnerOutput {
    pub cairo_pie: CairoPie,
    pub l2_to_l1_messages: Vec<MessageToL1>,
}

/// Runner for executing transactions and generating OS input for the virtual Starknet OS.
pub(crate) struct Runner<C, S, V>
where
    C: ClassesProvider,
    S: StorageProofProvider,
    V: VirtualBlockExecutor,
{
    pub(crate) classes_provider: C,
    pub(crate) storage_proofs_provider: S,
    pub(crate) virtual_block_executor: V,
    pub(crate) config: RunnerConfig,
    pub(crate) contract_class_manager: ContractClassManager,
    pub(crate) block_id: BlockId,
    pub(crate) chain_id: ChainId,
}

impl<C, S, V> Runner<C, S, V>
where
    C: ClassesProvider,
    S: StorageProofProvider,
    V: VirtualBlockExecutor,
{
    pub(crate) fn new(
        classes_provider: C,
        storage_proofs_provider: S,
        virtual_block_executor: V,
        config: RunnerConfig,
        contract_class_manager: ContractClassManager,
        block_id: BlockId,
        chain_id: ChainId,
    ) -> Self {
        Self {
            classes_provider,
            storage_proofs_provider,
            virtual_block_executor,
            config,
            contract_class_manager,
            block_id,
            chain_id,
        }
    }

    pub(crate) async fn create_virtual_os_hints(
        execution_data: VirtualBlockExecutionData,
        classes_provider: &C,
        storage_proofs_provider: &S,
        storage_proof_config: &StorageProofConfig,
        txs: Vec<(InvokeTransaction, TransactionHash)>,
    ) -> Result<OsHints, RunnerError> {
        let chain_info = execution_data.base_block_info.block_context.chain_info();
        let os_chain_info = OsChainInfo {
            chain_id: chain_info.chain_id.clone(),
            strk_fee_token_address: chain_info.fee_token_addresses.strk_fee_token_address,
        };

        let block_number = execution_data.base_block_info.block_context.block_info().block_number;

        let classes = classes_provider.get_classes(&execution_data.executed_class_hashes).await?;
        let storage_proofs = storage_proofs_provider
            .get_storage_proofs(block_number, &execution_data, storage_proof_config)
            .await?;

        let tx_execution_infos =
            execution_data.execution_outputs.into_iter().map(|output| output.0.into()).collect();

        let mut extended_initial_reads = storage_proofs.extended_initial_reads;

        extended_initial_reads
            .compiled_class_hashes
            .extend(&classes.class_hash_to_compiled_class_hash);

        extended_initial_reads.declared_contracts.clear();

        let virtual_os_block_input = VirtualOsBlockInput {
            contract_state_commitment_info: storage_proofs
                .commitment_infos
                .contracts_trie_commitment_info,
            address_to_storage_commitment_info: storage_proofs
                .commitment_infos
                .storage_tries_commitment_infos,
            contract_class_commitment_info: storage_proofs
                .commitment_infos
                .classes_trie_commitment_info,
            chain_info: os_chain_info,
            transactions: txs,
            tx_execution_infos,
            block_info: execution_data.base_block_info.block_context.block_info().clone(),
            initial_reads: extended_initial_reads,
            base_block_hash: execution_data.base_block_info.base_block_hash,
            base_block_header_commitments: execution_data
                .base_block_info
                .base_block_header_commitments,
            prev_base_block_hash: execution_data.base_block_info.prev_base_block_hash,
            compiled_classes: classes.compiled_classes,
        };

        Ok(virtual_os_block_input.into())
    }

    pub async fn run_virtual_os(
        self,
        txs: Vec<InvokeTransaction>,
    ) -> Result<RunnerOutput, RunnerError> {
        let Self {
            classes_provider,
            storage_proofs_provider,
            virtual_block_executor,
            config,
            contract_class_manager,
            block_id,
            chain_id,
        } = self;

        let txs_with_hashes: Vec<(InvokeTransaction, TransactionHash)> = txs
            .into_iter()
            .map(|tx| {
                let version = tx.version();
                let tx_hash = tx
                    .calculate_transaction_hash(&chain_id, &version)
                    .map_err(|e| RunnerError::TransactionHashError(e.to_string()))?;
                Span::current().record("tx_hash", display(&tx_hash));
                info!(transaction = ?tx, "Starting transaction proving");
                Ok((tx, tx_hash))
            })
            .collect::<Result<Vec<_>, RunnerError>>()?;

        let txs_for_hints = txs_with_hashes.clone();

        let execution_data = virtual_block_executor
            .execute(block_id, contract_class_manager, txs_with_hashes)
            .await?;

        let l2_to_l1_messages = execution_data.l2_to_l1_messages.clone();

        let os_hints = Self::create_virtual_os_hints(
            execution_data,
            &classes_provider,
            &storage_proofs_provider,
            &config.storage_proof_config,
            txs_for_hints,
        )
        .await?;

        let output = run_virtual_os(os_hints)?;

        let resources = &output.cairo_pie.execution_resources;
        info!(
            n_steps = resources.n_steps,
            n_memory_holes = resources.n_memory_holes,
            builtins = ?resources.builtin_instance_counter,
            "Virtual OS execution resources"
        );

        Ok(RunnerOutput { cairo_pie: output.cairo_pie, l2_to_l1_messages })
    }
}

/// Trait for runners that can execute the virtual Starknet OS.
#[async_trait(?Send)]
pub trait VirtualSnosRunner: Clone {
    async fn run_virtual_os(
        &self,
        block_id: BlockId,
        txs: Vec<InvokeTransaction>,
    ) -> Result<RunnerOutput, RunnerError>;
}

pub(crate) type RpcRunner =
    Runner<WasmClassesProvider, RpcStorageProofsProvider, RpcVirtualBlockExecutor>;

/// Factory for creating RPC-based runners.
#[derive(Clone)]
pub struct RpcRunnerFactory {
    node_url: Url,
    chain_info: ChainInfo,
    contract_class_manager: ContractClassManager,
    runner_config: RunnerConfig,
}

impl RpcRunnerFactory {
    pub(crate) fn new(
        node_url: Url,
        chain_info: ChainInfo,
        contract_class_manager: ContractClassManager,
        runner_config: RunnerConfig,
    ) -> Self {
        Self { node_url, chain_info, contract_class_manager, runner_config }
    }

    fn create_runner(&self, block_id: BlockId) -> RpcRunner {
        let virtual_block_executor = RpcVirtualBlockExecutor::new(
            self.node_url.to_string(),
            self.chain_info.clone(),
            block_id,
            self.runner_config.virtual_block_executor_config.clone(),
        );

        let storage_proofs_provider = RpcStorageProofsProvider::new(self.node_url.clone());

        let rpc_state_reader = RpcStateReader::new_with_config_from_url(
            self.node_url.to_string(),
            self.chain_info.clone(),
            block_id,
        );

        let classes_provider = WasmClassesProvider::new(rpc_state_reader);

        Runner::new(
            classes_provider,
            storage_proofs_provider,
            virtual_block_executor,
            self.runner_config.clone(),
            self.contract_class_manager.clone(),
            block_id,
            self.chain_info.chain_id.clone(),
        )
    }
}

#[async_trait(?Send)]
impl VirtualSnosRunner for RpcRunnerFactory {
    async fn run_virtual_os(
        &self,
        block_id: BlockId,
        txs: Vec<InvokeTransaction>,
    ) -> Result<RunnerOutput, RunnerError> {
        let runner = self.create_runner(block_id);
        runner.run_virtual_os(txs).await
    }
}
