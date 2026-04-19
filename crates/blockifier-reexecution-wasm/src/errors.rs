use thiserror::Error;

pub type ReexecutionResult<T> = Result<T, ReexecutionError>;

#[derive(Debug, Error)]
pub enum ReexecutionError {
    #[error("rpc request failed: {0}")]
    RpcRequest(String),
    #[error("rpc status error: {0}")]
    RpcStatus(String),
    #[error("rpc response parse error: {0}")]
    RpcParse(String),
    #[error("block not found")]
    BlockNotFound,
    #[error("contract address not found")]
    ContractAddressNotFound,
    #[error("class hash not found")]
    ClassHashNotFound,
    #[error("invalid params: {0}")]
    InvalidParams(String),
    #[error("transaction execution error: {0}")]
    TransactionExecution(String),
    #[error("unexpected rpc error {code}: {message}")]
    UnexpectedRpc { code: i32, message: String },
    #[error("serde error: {0}")]
    Serde(String),
    #[error("state error: {0}")]
    State(String),
    #[error("compiled class error: {0}")]
    CompiledClass(String),
}

impl From<reqwest::Error> for ReexecutionError {
    fn from(value: reqwest::Error) -> Self {
        Self::RpcRequest(value.to_string())
    }
}

impl From<serde_json::Error> for ReexecutionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value.to_string())
    }
}
