use std::sync::LazyLock;

use blockifier::context::{ChainInfo, FeeTokenAddresses};
use starknet_api::core::{ChainId, ContractAddress};
use starknet_types_core::felt::Felt;

pub static STRK_FEE_CONTRACT_ADDRESS: LazyLock<ContractAddress> = LazyLock::new(|| {
    ContractAddress::try_from(
        Felt::from_hex(
            "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d",
        )
        .expect("invalid STRK fee contract address"),
    )
    .expect("failed to convert STRK fee contract address")
});

pub static ETH_FEE_CONTRACT_ADDRESS: LazyLock<ContractAddress> = LazyLock::new(|| {
    ContractAddress::try_from(
        Felt::from_hex(
            "0x49d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7",
        )
        .expect("invalid ETH fee contract address"),
    )
    .expect("failed to convert ETH fee contract address")
});

pub fn get_chain_info(
    chain_id: &ChainId,
    strk_fee_token_address_override: Option<ContractAddress>,
) -> ChainInfo {
    let fee_token_addresses = match chain_id {
        ChainId::Mainnet | ChainId::Sepolia | ChainId::IntegrationSepolia => FeeTokenAddresses {
            strk_fee_token_address: strk_fee_token_address_override
                .unwrap_or(*STRK_FEE_CONTRACT_ADDRESS),
            eth_fee_token_address: *ETH_FEE_CONTRACT_ADDRESS,
        },
        unknown_chain => unimplemented!("Unknown chain ID {unknown_chain}."),
    };

    ChainInfo { chain_id: chain_id.clone(), fee_token_addresses, is_l3: false }
}
