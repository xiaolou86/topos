use crate::{Error, SubnetEvent};
use ethers::abi::ethabi::ethereum_types::{H160, U64};
use ethers::prelude::LocalWallet;
use ethers::{
    prelude::abigen,
    providers::{Middleware, Provider, Ws},
    signers::Signer,
};
use std::sync::Arc;
use tracing::info;

abigen!(IToposCore, "npm:@topos-network/topos-smart-contracts@latest/artifacts/contracts/interfaces/IToposCore.sol/IToposCore.json");

pub(crate) fn create_topos_core_contract_from_json<T: Middleware>(
    contract_address: &str,
    client: Arc<T>,
) -> Result<IToposCore<T>, Error> {
    let address: ethers::types::Address =
        contract_address.parse().map_err(Error::HexDecodingError)?;
    let contract = IToposCore::new(address, client);
    Ok(contract)
}

pub(crate) async fn get_block_events(
    contract: &IToposCore<Provider<Ws>>,
    block_number: U64,
) -> Result<Vec<crate::SubnetEvent>, Error> {
    let events = contract.events().from_block(block_number);
    let topos_core_events = events
        .query()
        .await
        .map_err(|e| Error::ContractError(e.to_string()))?;
    let mut result = Vec::new();

    for event in topos_core_events {
        if let IToposCoreEvents::CrossSubnetMessageSentFilter(f) = event {
            info!("Received CrossSubnetMessageSentFilter event: {f:?}");
            result.push(SubnetEvent::CrossSubnetMessageSent {
                target_subnet_id: f.target_subnet_id.into(),
            })
        } else {
            // Ignored for now other events UpgradedFilter, CertStoredFilter
        }
    }

    Ok(result)
}

pub fn derive_eth_address(secret_key: &[u8]) -> Result<H160, crate::Error> {
    let signer = hex::encode(secret_key)
        .parse::<LocalWallet>()
        .map_err(|e| Error::InvalidKey(e.to_string()))?;
    Ok(signer.address())
}
