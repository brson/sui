// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// TODO remove when integrated
#![allow(unused)]

use std::sync::Arc;

use crate::abi::EthBridgeEvent;
use crate::error::{BridgeError, BridgeResult};
use crate::types::{BridgeAction, EthLog};
use ethers::providers::{Http, JsonRpcClient, Middleware, Provider, ProviderError};
use ethers::types::TxHash;
use ethers::types::{Block, BlockId, Filter, H256};
use std::str::FromStr;
use tap::{Tap, TapFallible};

#[cfg(test)]
use crate::eth_mock_provider::EthMockProvider;
use ethers::{
    providers::MockProvider,
    types::{U256, U64},
};

pub struct EthClient<P> {
    provider: Provider<P>,
}

impl EthClient<Http> {
    pub async fn new(provider_url: &str) -> anyhow::Result<Self> {
        let provider = Provider::try_from(provider_url)?;
        let self_ = Self { provider };
        self_.describe().await?;
        Ok(self_)
    }
}

#[cfg(test)]
impl EthClient<EthMockProvider> {
    pub fn new_mocked(provider: EthMockProvider) -> Self {
        let provider = Provider::new(provider);
        Self { provider }
    }
}

impl<P> EthClient<P>
where
    P: JsonRpcClient,
{
    // TODO assert chain identifier
    async fn describe(&self) -> anyhow::Result<()> {
        let chain_id = self.provider.get_chainid().await?;
        let block_number = self.provider.get_block_number().await?;
        tracing::info!(
            "EthClient is connected to chain {chain_id}, current block number: {block_number}"
        );
        Ok(())
    }

    pub async fn get_finalized_bridge_action_maybe(
        &self,
        tx_hash: TxHash,
        event_idx: u16,
    ) -> BridgeResult<BridgeAction> {
        let receipt = self
            .provider
            .get_transaction_receipt(tx_hash)
            .await
            .map_err(BridgeError::from)?
            .ok_or(BridgeError::TxNotFound)?;
        let receipt_block_num = receipt.block_number.ok_or(BridgeError::ProviderError(
            "Provider returns log without block_number".into(),
        ))?;
        let last_finalized_block_id = self.get_last_finalized_block_id().await?;
        if receipt_block_num.as_u64() > last_finalized_block_id {
            return Err(BridgeError::TxNotFinalized);
        }
        let log = receipt
            .logs
            .get(event_idx as usize)
            .ok_or(BridgeError::NoBridgeEventsInTxPosition)?;
        let eth_log = EthLog {
            block_number: receipt_block_num.as_u64(),
            tx_hash,
            log_index_in_tx: event_idx,
            log: log.clone(),
        };
        let bridge_event = EthBridgeEvent::try_from_eth_log(&eth_log)
            .ok_or(BridgeError::NoBridgeEventsInTxPosition)?;
        bridge_event
            .try_into_bridge_action(tx_hash, event_idx)
            .ok_or(BridgeError::BridgeEventNotActionable)
    }

    pub async fn get_last_finalized_block_id(&self) -> BridgeResult<u64> {
        let block: Result<Option<Block<ethers::types::TxHash>>, ethers::prelude::ProviderError> =
            self.provider
                .request("eth_getBlockByNumber", ("finalized", false))
                .await;
        let block = block?.ok_or(BridgeError::TransientProviderError(
            "Provider fails to return last finalized block".into(),
        ))?;
        let number = block.number.ok_or(BridgeError::TransientProviderError(
            "Provider returns block without number".into(),
        ))?;
        Ok(number.as_u64())
    }

    // TODO: this needs some pagination if the range is too big
    pub async fn get_events_in_range(
        &self,
        address: ethers::types::Address,
        start_block: u64,
        end_block: u64,
    ) -> BridgeResult<Vec<EthLog>> {
        let filter = Filter::new()
            .from_block(start_block)
            .to_block(end_block)
            .address(address);
        let logs = self
            .provider
            .get_logs(&filter)
            .await
            .map_err(BridgeError::from)
            .tap_err(|e| {
                tracing::error!(
                    "get_events_in_range failed. Filter: {:?}. Error {:?}",
                    filter,
                    e
                )
            })?;
        if logs.is_empty() {
            return Ok(vec![]);
        }
        let tasks = logs.into_iter().map(|log| self.get_log_tx_details(log));
        let results = futures::future::join_all(tasks)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .tap_err(|e| {
                tracing::error!(
                    "get_log_tx_details failed. Filter: {:?}. Error {:?}",
                    filter,
                    e
                )
            })?;
        Ok(results)
    }

    /// This function converts a `Log` to `EthLog`, to make sure the `block_num`, `tx_hash` and `log_index_in_tx`
    /// are available for downstream.
    // It's frustratingly ugly because of the nulliability of many fields in `Log`.
    async fn get_log_tx_details(&self, log: ethers::types::Log) -> BridgeResult<EthLog> {
        let block_number = log
            .block_number
            .ok_or(BridgeError::ProviderError(
                "Provider returns log without block_number".into(),
            ))?
            .as_u64();
        let tx_hash = log.transaction_hash.ok_or(BridgeError::ProviderError(
            "Provider returns log without transaction_hash".into(),
        ))?;
        // This is the log index in the block, rather than transaction.
        let log_index = log.log_index.ok_or(BridgeError::ProviderError(
            "Provider returns log without log_index".into(),
        ))?;

        // Now get the log's index in the transaction. There is `transaction_log_index` field in
        // `Log`, but I never saw it populated.

        let receipt = self
            .provider
            .get_transaction_receipt(tx_hash)
            .await
            .map_err(BridgeError::from)?
            .ok_or(BridgeError::ProviderError(format!(
                "Provide cannot find eth transaction for log: {:?})",
                log
            )))?;

        let receipt_block_num = receipt.block_number.ok_or(BridgeError::ProviderError(
            "Provider returns log without block_number".into(),
        ))?;
        if receipt_block_num.as_u64() != block_number {
            return Err(BridgeError::ProviderError(format!("Provider returns receipt with different block number from log. Receipt: {:?}, Log: {:?}", receipt, log)));
        }

        // Find the log index in the transaction
        let mut log_index_in_tx = None;
        for (idx, receipt_log) in receipt.logs.iter().enumerate() {
            // match log index (in the block)
            if receipt_log.log_index == Some(log_index) {
                // make sure the topics and data match
                if receipt_log.topics != log.topics || receipt_log.data != log.data {
                    return Err(BridgeError::ProviderError(format!("Provider returns receipt with different log from log. Receipt: {:?}, Log: {:?}", receipt, log)));
                }
                log_index_in_tx = Some(idx);
            }
        }
        let log_index_in_tx = log_index_in_tx.ok_or(BridgeError::ProviderError(format!(
            "Couldn't find matching log {:?} in transaction {}",
            log, tx_hash
        )))?;

        Ok(EthLog {
            block_number,
            tx_hash,
            log_index_in_tx: log_index_in_tx as u16,
            log,
        })
    }
}

#[cfg(test)]
mod tests {
    use ethers::types::{Address as EthAddress, Log, TransactionReceipt};
    use prometheus::Registry;

    use super::*;
    use crate::test_utils::mock_get_logs;
    use crate::test_utils::{
        get_test_authority_and_key, get_test_log_and_action, get_test_sui_to_eth_bridge_action,
        mock_last_finalized_block,
    };
    use crate::types::BridgeAction;
    use crate::types::SignedBridgeAction;

    #[tokio::test]
    async fn test_get_finalized_bridge_action_maybe() {
        telemetry_subscribers::init_for_testing();
        let registry = Registry::new();
        mysten_metrics::init_metrics(&registry);
        let mock_provider = EthMockProvider::new();
        mock_last_finalized_block(&mock_provider, 777);
        let client = EthClient::new_mocked(mock_provider.clone());
        let result = client.get_last_finalized_block_id().await.unwrap();
        assert_eq!(result, 777);

        let eth_tx_hash = TxHash::random();
        let log = Log {
            transaction_hash: Some(eth_tx_hash),
            block_number: Some(U64::from(778)),
            ..Default::default()
        };
        let (good_log, bridge_action) = get_test_log_and_action(EthAddress::zero(), eth_tx_hash, 1);
        // Mocks `eth_getTransactionReceipt` to return `log` and `good_log` in order
        mock_provider
            .add_response::<[TxHash; 1], TransactionReceipt, TransactionReceipt>(
                "eth_getTransactionReceipt",
                [log.transaction_hash.unwrap()],
                TransactionReceipt {
                    block_number: log.block_number,
                    logs: vec![log, good_log],
                    ..Default::default()
                },
            )
            .unwrap();

        let error = client
            .get_finalized_bridge_action_maybe(eth_tx_hash, 0)
            .await
            .unwrap_err();
        match error {
            BridgeError::TxNotFinalized => {}
            _ => panic!("expected TxNotFinalized"),
        };

        // 778 is now finalized
        mock_last_finalized_block(&mock_provider, 778);

        let error = client
            .get_finalized_bridge_action_maybe(eth_tx_hash, 2)
            .await
            .unwrap_err();
        // Receipt only has 2 logs
        match error {
            BridgeError::NoBridgeEventsInTxPosition => {}
            _ => panic!("expected NoBridgeEventsInTxPosition"),
        };

        let error = client
            .get_finalized_bridge_action_maybe(eth_tx_hash, 0)
            .await
            .unwrap_err();
        // Same, `log` is not a BridgeEvent
        match error {
            BridgeError::NoBridgeEventsInTxPosition => {}
            _ => panic!("expected NoBridgeEventsInTxPosition"),
        };

        let action = client
            .get_finalized_bridge_action_maybe(eth_tx_hash, 1)
            .await
            .unwrap();
        assert_eq!(action, bridge_action);
    }
}
