use serde::{Deserialize, Serialize};

use crate::running::runner::RunnerConfig;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ProverConfig {
    pub rpc_node_url: String,
    pub chain_id: starknet_api::core::ChainId,
    pub strk_fee_token_address: Option<starknet_api::core::ContractAddress>,
    #[serde(default)]
    pub validate_zero_fee_fields: bool,
    #[serde(default)]
    pub runner_config: RunnerConfig,
    #[serde(default)]
    pub contract_class_manager_config:
        blockifier::blockifier::config::ContractClassManagerConfig,
}

impl Default for ProverConfig {
    fn default() -> Self {
        Self {
            rpc_node_url: String::new(),
            chain_id: starknet_api::core::ChainId::Sepolia,
            strk_fee_token_address: None,
            validate_zero_fee_fields: true,
            runner_config: RunnerConfig::default(),
            contract_class_manager_config: Default::default(),
        }
    }
}
