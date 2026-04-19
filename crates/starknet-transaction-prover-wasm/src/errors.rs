use blockifier::state::errors::StateError;
use cairo_vm::types::errors::program_errors::ProgramError;
use starknet_api::core::ClassHash;
use starknet_api::transaction::TransactionHash;
use starknet_os::errors::StarknetOsError;
use starknet_os::io::os_output::OsOutputError;
use starknet_patricia_storage::errors::SerializationError;
use starknet_proof_verifier::ProgramOutputError;
use starknet_rust::providers::ProviderError;
use thiserror::Error;

use crate::rpc_compat::ReexecutionError;

#[derive(Debug, Error)]
pub enum VirtualBlockExecutorError {
    #[error(transparent)]
    ReexecutionError(#[from] ReexecutionError),
    #[error("Transaction execution failed: {0}")]
    TransactionExecutionError(String),
    #[error("Reverted transactions are not supported; hash: {0:?}, revert reason: {1}")]
    TransactionReverted(TransactionHash, String),
    #[error("Block state unavailable after execution")]
    StateUnavailable,
    #[error("Failed to acquire bouncer lock: {0}")]
    BouncerLockError(String),
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error(transparent)]
    ClassesProvider(#[from] ClassesProviderError),
    #[error(transparent)]
    ProofProvider(#[from] ProofProviderError),
    #[error(transparent)]
    VirtualBlockExecutor(#[from] VirtualBlockExecutorError),
    #[error(transparent)]
    OsExecution(#[from] StarknetOsError),
    #[error("OS Input generation failed: {0}")]
    InputGenerationError(String),
    #[error("Failed to calculate transaction hash: {0}")]
    TransactionHashError(String),
}

#[derive(Debug, Error)]
pub enum ProofProviderError {
    #[error("Invalid state diff: {0}")]
    InvalidStateDiff(String),
    #[error("RPC provider error: {0}")]
    Rpc(#[from] ProviderError),
    #[error(transparent)]
    SerializationError(#[from] SerializationError),
    #[error("Invalid RPC proof response: {0}")]
    InvalidProofResponse(String),
    #[error("Block commitment error: {0}")]
    BlockCommitmentError(String),
}

#[derive(Debug, Error)]
pub enum ClassesProviderError {
    #[error("Failed to get classes: {0}")]
    GetClassesError(String),
    #[error("Starknet os does not support deprecated contract classes, class hash: {0} is deprecated")]
    DeprecatedContractError(ClassHash),
    #[error("Unexpected error: bytecode of a class contained a non-integer value")]
    InvalidBytecodeElement,
    #[error(transparent)]
    StateError(#[from] StateError),
    #[error(transparent)]
    HintsConversionError(#[from] ProgramError),
}

#[derive(Debug, Error)]
pub enum ProvingError {
    #[cfg(feature = "stwo_proving")]
    #[error("Prover execution failed: {0}")]
    ProverExecution(String),
}

#[derive(Debug, Error)]
pub enum VirtualSnosProverError {
    #[error("Invalid transaction type: {0}")]
    InvalidTransactionType(String),
    #[error("Invalid transaction input: {0}")]
    InvalidTransactionInput(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error(transparent)]
    ProgramOutputError(#[from] ProgramOutputError),
    #[error(transparent)]
    RunnerError(#[from] Box<RunnerError>),
    #[cfg(feature = "stwo_proving")]
    #[error(transparent)]
    ProvingError(#[from] ProvingError),
    #[error(transparent)]
    OutputParseError(#[from] OsOutputError),
}
