use blockifier::state::cached_state::StateMaps;
use serde::Deserialize;
use starknet_api::core::{ClassHash, ContractAddress, Nonce};
use starknet_api::state::StorageKey;
use starknet_types_core::felt::Felt;

#[derive(Deserialize)]
struct StorageEntry {
    contract_address: ContractAddress,
    key: StorageKey,
    value: Felt,
}

#[derive(Deserialize)]
struct NonceEntry {
    contract_address: ContractAddress,
    nonce: Nonce,
}

#[derive(Deserialize)]
struct ClassHashEntry {
    contract_address: ContractAddress,
    class_hash: ClassHash,
}

#[derive(Deserialize)]
struct DeclaredEntry {
    class_hash: ClassHash,
    is_declared: bool,
}

#[derive(Deserialize, Default)]
struct RpcInitialReads {
    #[serde(default)]
    storage: Vec<StorageEntry>,
    #[serde(default)]
    nonces: Vec<NonceEntry>,
    #[serde(default)]
    class_hashes: Vec<ClassHashEntry>,
    #[serde(default)]
    declared_contracts: Vec<DeclaredEntry>,
}

impl From<RpcInitialReads> for StateMaps {
    fn from(reads: RpcInitialReads) -> Self {
        let mut maps = StateMaps::default();
        maps.storage
            .extend(reads.storage.into_iter().map(|e| ((e.contract_address, e.key), e.value)));
        maps.nonces.extend(reads.nonces.into_iter().map(|e| (e.contract_address, e.nonce)));
        maps.class_hashes
            .extend(reads.class_hashes.into_iter().map(|e| (e.contract_address, e.class_hash)));
        maps.declared_contracts
            .extend(reads.declared_contracts.into_iter().map(|e| (e.class_hash, e.is_declared)));
        maps
    }
}

pub(crate) fn deserialize_rpc_initial_reads(value: serde_json::Value) -> Result<StateMaps, String> {
    serde_json::from_value::<RpcInitialReads>(value).map(Into::into).map_err(|e| e.to_string())
}
