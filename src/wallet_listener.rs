use anyhow::Context;
use futures::StreamExt;
use poise::serenity_prelude::{CreateMessage, Http, UserId};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, instrument, trace, warn};
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::bitcoin::Txid;
use vrsc_rpc::json::{
    GetRawTransactionResultVerbose, GetTransactionDetailsCategory, GetTransactionResult,
};
use vrsc_rpc::{
    Auth,
    client::{Client, RpcApi},
};

use crate::config::Config;
use crate::database::*;
use crate::{Error, VRSC_CURRENCY_ID};

/// Listens for wallet transactions and processes them.
///
/// Every interaction with a wallet will trigger a notification, which gets processed here.
/// Deposits from users are thus processed here.
///
/// Works with UNIX sockets: The coin daemon sends a notification that includes the txid to `<project_root>/walletnotify.sh`
/// which subsequently sends a message over a UNIX socket. This TransactionProcessor listens on that socket for incoming messages
/// and processes each message.
///
/// The bot can be in maintenance mode, in which case processing will be postponed by putting the yet-to-be-processed
/// txids in a database table. When maintenance mode is disabled, the transactions will be processed.
#[derive(Debug)]
pub struct TransactionProcessor {
    http: Arc<Http>,
    pool: PgPool,
    config: Config,
    pub maintenance: Arc<RwLock<bool>>,
    pub deposits_enabled: Arc<RwLock<bool>>,
    queue_small_txns: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
    queue_large_txns: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
}

impl TransactionProcessor {
    pub fn new(
        http: Arc<Http>,
        pool: PgPool,
        config: Config,
        maintenance: Arc<RwLock<bool>>,
        deposits_enabled: Arc<RwLock<bool>>,
    ) -> Self {
        TransactionProcessor {
            http,
            pool,
            config,
            maintenance,
            deposits_enabled,
            queue_small_txns: Arc::new(RwLock::new(VecDeque::new())),
            queue_large_txns: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    pub async fn listen_wallet_notifications(&self, verus_client: &Client) -> Result<(), Error> {
        let mut socket = tmq::subscribe(&tmq::Context::new())
            .connect(&format!(
                "tcp://127.0.0.1:{}",
                self.config.application.zmq_tx_port
            ))?
            .subscribe(b"hash")?;

        loop {
            if let Some(Ok(msg)) = socket.next().await {
                if let Some(hash) = msg.iter().nth(1) {
                    let tx_hash_str = hash
                        .iter()
                        .map(|byte| format!("{:02x}", *byte))
                        .collect::<Vec<_>>()
                        .join("");

                    trace!("new tx: {tx_hash_str}");

                    let txid = Txid::from_str(&tx_hash_str)?;
                    let raw_tx = verus_client
                        .get_raw_transaction_verbose(&txid)
                        .with_context(|| format!("Failing tx: {txid}"))?;

                    if should_skip_wallet_tx(verus_client, &txid) {
                        continue;
                    }

                    let mut conn = self.pool.acquire().await?;

                    for vout in raw_tx.vout.iter() {
                        if let Some(addresses) = &vout.script_pubkey.addresses {
                            for address in addresses {
                                if let Some(user_id) =
                                    get_user_from_address(&mut conn, address).await?
                                {
                                    trace!(?user_id, "there is a user for this address");

                                    if vout
                                        .value
                                        .gt(&self.config.application.min_deposit_threshold)
                                    {
                                        trace!("{txid} put in long queue");
                                        let mut long_write = self.queue_large_txns.write().await;
                                        long_write.push_back((txid, vout.value))
                                    } else {
                                        trace!("{txid} put in short queue");
                                        let mut write = self.queue_small_txns.write().await;
                                        write.push_back((txid, vout.value))
                                    }
                                }
                            }
                        }
                    }
                } else {
                    error!(?msg, "not a valid message");
                }
            } else {
                warn!("message was None");
            }
        }
    }

    pub async fn listen_block_notifications(&self) -> Result<(), Error> {
        let mut socket = tmq::subscribe(&tmq::Context::new())
            .connect(&format!(
                "tcp://127.0.0.1:{}",
                self.config.application.zmq_block_port
            ))?
            .subscribe(b"hash")?;

        if let Err(e) = self.reconcile_pending_withdrawals().await {
            error!(?e, "pending withdraw reconcile failed");
        }

        loop {
            if let Some(Ok(msg)) = socket.next().await {
                if let Some(hash) = msg.into_iter().nth(1) {
                    let _block_hash = hash
                        .iter()
                        .map(|byte| format!("{:02x}", *byte))
                        .collect::<Vec<_>>()
                        .join("");

                    trace!("new block: {_block_hash}");

                    self.process_short_queue().await?;
                    self.process_long_queue().await?;
                    if let Err(e) = self.reconcile_pending_withdrawals().await {
                        error!(?e, "pending withdraw reconcile failed");
                    }
                } else {
                    error!("not a valid message!");
                }
            } else {
                error!("no correct message received");
            }
        }
    }

    pub async fn check_tx(&self, txid: Txid) -> Result<(), Error> {
        let client = Client::vrsc(
            self.config.application.testnet,
            Auth::UserPass(
                format!("127.0.0.1:{}", self.config.application.rpc_port),
                self.config.application.rpc_user.clone(),
                self.config.application.rpc_password.clone(),
            ),
        )?;

        trace!("getting raw_transaction {txid}");
        let raw_tx = client.get_raw_transaction_verbose(&txid)?;

        if should_skip_wallet_tx(&client, &txid) {
            return Ok(());
        }

        let mut conn = self.pool.acquire().await?;

        for vout in raw_tx.vout.iter() {
            if let Some(addresses) = &vout.script_pubkey.addresses {
                for address in addresses {
                    if let Some(user_id) = get_user_from_address(&mut conn, address).await? {
                        trace!("there is a user for this address: {user_id}",);
                        let mut write = self.queue_small_txns.write().await;
                        let mut long_write = self.queue_large_txns.write().await;

                        // if the value of the incoming transaction is greater than
                        if vout
                            .value
                            .gt(&self.config.application.min_deposit_threshold)
                        {
                            trace!("{txid} put in long queue");
                            long_write.push_back((txid, vout.value))
                        } else {
                            trace!("{txid} put in short queue");
                            write.push_back((txid, vout.value))
                        }
                    }
                }
            } else {
                trace!("no addresses found in scriptpubkey");
            }
        }

        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn process_short_queue(&self) -> Result<(), Error> {
        let deposits_enabled = *self.deposits_enabled.read().await;
        if !deposits_enabled {
            warn!("deposits disabled");

            return Ok(());
        }

        let mut write = self.queue_small_txns.write().await;
        let http = Arc::clone(&self.http);
        let queue_size = write.len();
        debug!("{queue_size} transactions in short queue");

        loop {
            if let Some(front) = write.front() {
                trace!("read {front:?} from front");

                let client = Client::vrsc(
                    self.config.application.testnet,
                    Auth::UserPass(
                        format!("127.0.0.1:{}", self.config.application.rpc_port),
                        self.config.application.rpc_user.clone(),
                        self.config.application.rpc_password.clone(),
                    ),
                )
                .unwrap();

                let raw_tx = client.get_raw_transaction_verbose(&front.0)?;

                if let Some(confs) = raw_tx.confirmations {
                    let min_confs = self.config.application.min_deposit_confirmations_small;

                    if confs < min_confs {
                        trace!("tx needs {}, has {confs}: {}", min_confs, front.0);
                        break;
                    } else {
                        trace!("tx has at least {} confs: {}", min_confs, front.0);
                        if let Err(e) = process_confirmed_txid(
                            Arc::clone(&http),
                            &self.pool,
                            &client,
                            &front.0,
                            &raw_tx,
                        )
                        .await
                        {
                            error!(
                                "something went wrong while handling a new wallet tx: {:?}\n{:?}",
                                e, &front
                            )
                        }

                        let _ = write.pop_front();
                        continue;
                    }
                } else {
                    trace!("{} has no confirmations yet", front.0);
                    break;
                }
            } else {
                trace!("new block but no transactions in queue");
                break;
            }
        }

        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn process_long_queue(&self) -> Result<(), Error> {
        let deposits_enabled = *self.deposits_enabled.read().await;
        if !deposits_enabled {
            warn!("deposits disabled");

            return Ok(());
        }
        let mut write = self.queue_large_txns.write().await;
        let http = Arc::clone(&self.http);
        let queue_size = write.len();
        debug!("{queue_size} transactions in long queue");

        loop {
            if let Some(front) = write.front() {
                trace!("read {front:?} from front");

                let client = Client::vrsc(
                    self.config.application.testnet,
                    Auth::UserPass(
                        format!("127.0.0.1:{}", self.config.application.rpc_port),
                        self.config.application.rpc_user.clone(),
                        self.config.application.rpc_password.clone(),
                    ),
                )
                .unwrap();

                let raw_tx = client.get_raw_transaction_verbose(&front.0)?;

                if let Some(confs) = raw_tx.confirmations {
                    let min_confs = self.config.application.min_deposit_confirmations_large;

                    if confs < min_confs {
                        trace!("tx needs {}, has {confs}: {}", min_confs, front.0);
                        break;
                    } else {
                        trace!("tx has at least {} confs: {}", min_confs, front.0);
                        if let Err(e) = process_confirmed_txid(
                            Arc::clone(&http),
                            &self.pool,
                            &client,
                            &front.0,
                            &raw_tx,
                        )
                        .await
                        {
                            error!(
                                "something went wrong while handling a new wallet tx: {:?}\n{:?}",
                                e, &front
                            )
                        }

                        let _ = write.pop_front();
                        continue;
                    }
                } else {
                    trace!("{} has no confirmations yet", front.0);
                    break;
                }
            } else {
                trace!("new block but no transactions in queue");
                break;
            }
        }

        Ok(())
    }

    pub async fn reconcile_pending_withdrawals(&self) -> Result<(), Error> {
        let client = Client::vrsc(
            self.config.application.testnet,
            Auth::UserPass(
                format!("127.0.0.1:{}", self.config.application.rpc_port),
                self.config.application.rpc_user.clone(),
                self.config.application.rpc_password.clone(),
            ),
        )?;

        let mut conn = self.pool.acquire().await?;
        let pending = get_pending_withdrawals(&mut conn).await?;
        drop(conn);

        if pending.is_empty() {
            return Ok(());
        }

        let wallet_txs = client
            .list_transactions(Some(100), None, None)
            .unwrap_or_default();

        for p in pending {
            if let Some(opid) = &p.opid {
                match client.z_get_operation_status(vec![opid.as_str()]) {
                    Ok(status) => {
                        if let Some(Some(opstatus)) = status.first() {
                            match opstatus.status.as_str() {
                                "success" => {
                                    if let Some(result) = &opstatus.result {
                                        let tx_fee = crate::commands::wallet::network_fee_for_txid(
                                            &client,
                                            &result.txid,
                                        );
                                        let mut conn = self.pool.acquire().await?;
                                        finalize_withdraw(&mut conn, &p.uuid, &result.txid, tx_fee)
                                            .await?;
                                        continue;
                                    }
                                }
                                "failed" => {
                                    let mut tx = self.pool.begin().await?;
                                    refund_failed_withdraw(&mut tx, &p).await?;
                                    tx.commit().await?;
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        warn!(?e, opid, "z_get_operation_status failed");
                    }
                }
            }

            use vrsc_rpc::json::ListSinceBlockCategory;
            let match_send = wallet_txs.iter().find(|t| {
                matches!(t.category, ListSinceBlockCategory::Send)
                    && t.address == p.address
                    && t.amount.as_sat().unsigned_abs() == p.amount.as_sat()
                    && t.time + 5 >= p.created_at.timestamp() as u64
            });

            if let Some(t) = match_send {
                let tx_fee = t
                    .fee
                    .map(|f| Amount::from_sat(f.as_sat().unsigned_abs()))
                    .unwrap_or(Amount::ZERO);
                let mut conn = self.pool.acquire().await?;
                finalize_withdraw(&mut conn, &p.uuid, &t.txid, tx_fee).await?;
                continue;
            }

            let age = sqlx::types::chrono::Utc::now() - p.created_at;
            if age.num_seconds() > 30 * 60 {
                warn!(
                    uuid = %p.uuid,
                    address = %p.address,
                    amount = %p.amount,
                    "pending withdraw older than 30m with no matching daemon tx"
                );
            }
        }

        Ok(())
    }
}

/// Skip staking/coinbase and our own sends when the wallet already knows the tx.
/// If `gettransaction` fails, the wallet may not have indexed it yet — keep it.
fn should_skip_wallet_tx(client: &Client, txid: &Txid) -> bool {
    match client.get_transaction(txid, None) {
        Ok(wallet_tx) => {
            wallet_tx.generated == Some(true)
                || !wallet_tx
                    .details
                    .iter()
                    .any(|d| d.category == GetTransactionDetailsCategory::Receive)
        }
        Err(_) => false,
    }
}

async fn process_confirmed_txid(
    http: Arc<Http>,
    pool: &PgPool,
    client: &Client,
    txid: &Txid,
    raw_tx: &GetRawTransactionResultVerbose,
) -> Result<(), Error> {
    let wallet_tx = client.get_transaction(txid, None)?;
    let mut db_tx = pool.begin().await?;
    let dms = process_txid(&mut db_tx, raw_tx, &wallet_tx).await?;
    db_tx.commit().await?;

    for (user_id, amount) in dms {
        if let Err(e) = send_deposit_dm(http.clone(), user_id, amount).await {
            error!(?e, %user_id, "failed to send deposit DM");
        }
    }

    Ok(())
}

/// Credits a wallet transaction as a deposit when it is an actual receive to a user address.
///
/// Shared-wallet movements must not be credited: staking/coinbase (`generated`), change from
/// our own withdrawals, and other `send` outputs. Those stay in the daemon without increasing
/// user balances. Only `gettransaction` details with category `receive` are deposits.
pub async fn process_txid(
    tx: &mut Transaction<'_, Postgres>,
    raw_tx: &GetRawTransactionResultVerbose,
    wallet_tx: &GetTransactionResult,
) -> Result<Vec<(UserId, Amount)>, Error> {
    let currency_id = Address::from_str(VRSC_CURRENCY_ID)?;

    if wallet_tx.generated == Some(true) {
        debug!(
            txid = %raw_tx.txid,
            "skipping generated (coinbase/stake) transaction"
        );
        return Ok(Vec::new());
    }

    if transaction_processed(&mut **tx, &raw_tx.txid, &currency_id).await? {
        debug!("transaction already processed");
        return Ok(Vec::new());
    }

    let mut dms = Vec::new();

    for detail in wallet_tx
        .details
        .iter()
        .filter(|d| d.category == GetTransactionDetailsCategory::Receive)
    {
        let Some(user_id) = get_user_from_address(&mut **tx, &detail.address).await? else {
            continue;
        };

        let Some(vout) = raw_tx.vout.iter().find(|v| v.n == u32::from(detail.vout)) else {
            warn!(
                txid = %raw_tx.txid,
                vout = detail.vout,
                "receive detail has no matching vout"
            );
            continue;
        };

        let amount = vout.value_sat;
        let uuid = Uuid::new_v4();

        increase_balance(&mut **tx, &user_id, amount, &currency_id).await?;
        store_deposit_transaction(
            &mut **tx,
            &uuid,
            &user_id,
            &raw_tx.txid,
            &currency_id,
            amount,
            &detail.address,
        )
        .await?;

        dms.push((user_id, vout.value));
    }

    Ok(dms)
}

pub(crate) async fn send_deposit_dm(
    http: Arc<Http>,
    user_id: UserId,
    amount: Amount,
) -> Result<(), Error> {
    let user = http.get_user(user_id).await?;
    user.direct_message(
        http,
        CreateMessage::new().content(format!("Your deposit of {amount} has been processed.")),
    )
    .await?;

    Ok(())
}
