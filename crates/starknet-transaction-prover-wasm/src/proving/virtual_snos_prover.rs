#[cfg(feature = "stwo_proving")]
use std::sync::Arc;

use blockifier::state::contract_class_manager::ContractClassManager;
#[cfg(feature = "stwo_proving")]
use privacy_prove::{prepare_recursive_prover_precomputes, RecursiveProverPrecomputes};
use serde::{Deserialize, Serialize};
use starknet_api::rpc_transaction::{RpcInvokeTransaction, RpcInvokeTransactionV3, RpcTransaction};
use starknet_api::transaction::fields::{Fee, Proof, ProofFacts, ValidResourceBounds};
use starknet_api::transaction::{InvokeTransaction, MessageToL1};
use tracing::{info, instrument};
use url::Url;

use crate::config::ProverConfig;
use crate::errors::VirtualSnosProverError;
use crate::rpc_compat::{get_chain_info, BlockId};
use crate::running::runner::{RpcRunnerFactory, RunnerOutput, VirtualSnosRunner};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProveTransactionResult {
    pub proof: Proof,
    pub proof_facts: ProofFacts,
    pub l2_to_l1_messages: Vec<MessageToL1>,
}

#[derive(Clone)]
pub struct VirtualSnosProver<R: VirtualSnosRunner> {
    runner: R,
    validate_zero_fee_fields: bool,
    #[cfg(feature = "stwo_proving")]
    precomputes: Arc<RecursiveProverPrecomputes>,
}

pub type RpcVirtualSnosProver = VirtualSnosProver<RpcRunnerFactory>;

impl VirtualSnosProver<RpcRunnerFactory> {
    pub fn new(prover_config: &ProverConfig) -> Self {
        let contract_class_manager =
            ContractClassManager::start(prover_config.contract_class_manager_config.clone());
        let node_url =
            Url::parse(&prover_config.rpc_node_url).expect("Invalid RPC node URL in config");
        let chain_info =
            get_chain_info(&prover_config.chain_id, prover_config.strk_fee_token_address);
        let runner = RpcRunnerFactory::new(
            node_url,
            chain_info,
            contract_class_manager,
            prover_config.runner_config.clone(),
        );
        #[cfg(feature = "stwo_proving")]
        let precomputes = prepare_recursive_prover_precomputes()
            .expect("Failed to prepare recursive prover precomputes");
        Self {
            runner,
            validate_zero_fee_fields: prover_config.validate_zero_fee_fields,
            #[cfg(feature = "stwo_proving")]
            precomputes,
        }
    }
}

impl<R: VirtualSnosRunner> VirtualSnosProver<R> {
    #[allow(dead_code)]
    pub(crate) fn from_runner(runner: R) -> Self {
        #[cfg(feature = "stwo_proving")]
        let precomputes = prepare_recursive_prover_precomputes()
            .expect("Failed to prepare recursive prover precomputes");
        Self {
            runner,
            validate_zero_fee_fields: true,
            #[cfg(feature = "stwo_proving")]
            precomputes,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn disable_fee_validation(mut self) -> Self {
        self.validate_zero_fee_fields = false;
        self
    }

    #[instrument(skip(self, transaction), fields(block_id = ?block_id, tx_hash))]
    pub async fn prove_transaction(
        &self,
        block_id: BlockId,
        transaction: RpcTransaction,
    ) -> Result<ProveTransactionResult, VirtualSnosProverError> {
        if matches!(block_id, BlockId::Pending) {
            return Err(VirtualSnosProverError::ValidationError(
                "Pending blocks are not supported; only finalized blocks can be proven."
                    .to_string(),
            ));
        }

        let invoke_v3 = extract_rpc_invoke_tx(transaction)?;
        validate_transaction_input(&invoke_v3, self.validate_zero_fee_fields)?;
        let invoke_tx = InvokeTransaction::V3(invoke_v3.into());

        let txs = vec![invoke_tx];
        let runner_output = self
            .runner
            .run_virtual_os(block_id, txs)
            .await
            .map_err(|err| VirtualSnosProverError::RunnerError(Box::new(err)))?;

        info!("OS execution completed");

        let result = self.prove_virtual_snos_run(runner_output).await?;

        info!("prove_transaction completed");
        Ok(result)
    }

    #[cfg(not(feature = "stwo_proving"))]
    async fn prove_virtual_snos_run(
        &self,
        _runner_output: RunnerOutput,
    ) -> Result<ProveTransactionResult, VirtualSnosProverError> {
        unimplemented!(
            "In-memory proving requires the `stwo_proving` feature flag and a nightly Rust \
             toolchain."
        );
    }

    #[cfg(feature = "stwo_proving")]
    async fn prove_virtual_snos_run(
        &self,
        runner_output: RunnerOutput,
    ) -> Result<ProveTransactionResult, VirtualSnosProverError> {
        use starknet_api::transaction::fields::VIRTUAL_SNOS;

        use crate::proving::prover::prove;

        let prover_output = prove(runner_output.cairo_pie, self.precomputes.clone()).await?;
        let proof_facts = prover_output.program_output.try_into_proof_facts(VIRTUAL_SNOS)?;

        Ok(ProveTransactionResult {
            proof: prover_output.proof,
            proof_facts,
            l2_to_l1_messages: runner_output.l2_to_l1_messages,
        })
    }
}

fn extract_rpc_invoke_tx(
    tx: RpcTransaction,
) -> Result<RpcInvokeTransactionV3, VirtualSnosProverError> {
    match tx {
        RpcTransaction::Invoke(RpcInvokeTransaction::V3(invoke_v3)) => Ok(invoke_v3),
        RpcTransaction::Declare(_) => Err(VirtualSnosProverError::InvalidTransactionType(
            "Declare transactions are not supported; only Invoke transactions are allowed"
                .to_string(),
        )),
        RpcTransaction::DeployAccount(_) => Err(VirtualSnosProverError::InvalidTransactionType(
            "DeployAccount transactions are not supported; only Invoke transactions are allowed"
                .to_string(),
        )),
    }
}

fn validate_transaction_input(
    tx: &RpcInvokeTransactionV3,
    validate_zero_fee_fields: bool,
) -> Result<(), VirtualSnosProverError> {
    if !tx.proof.is_empty() {
        return Err(VirtualSnosProverError::InvalidTransactionInput(
            "The proof field must be empty on input.".to_string(),
        ));
    }
    if !tx.proof_facts.is_empty() {
        return Err(VirtualSnosProverError::InvalidTransactionInput(
            "The proof_facts field must be empty on input.".to_string(),
        ));
    }
    if validate_zero_fee_fields {
        let resource_bounds = ValidResourceBounds::AllResources(tx.resource_bounds);
        let max_fee = resource_bounds.max_possible_fee(tx.tip);
        if max_fee != Fee(0) {
            return Err(VirtualSnosProverError::InvalidTransactionInput(format!(
                "Max possible fee must be zero, got: {max_fee}."
            )));
        }
    }
    Ok(())
}
