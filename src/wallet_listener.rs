use futures::StreamExt;
use poise::serenity_prelude::{CreateMessage, Http, UserId};
use sqlx::{PgConnection, PgPool};
use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, instrument, trace, warn};
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::bitcoin::Txid;
use vrsc_rpc::json::GetRawTransactionResultVerbose;
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
                    let raw_tx = verus_client.get_raw_transaction_verbose(&txid)?;

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
                                        long_write.push_back((txid.clone(), vout.value))
                                    } else {
                                        trace!("{txid} put in short queue");
                                        let mut write = self.queue_small_txns.write().await;
                                        write.push_back((txid.clone(), vout.value))
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
                            long_write.push_back((txid.clone(), vout.value))
                        } else {
                            trace!("{txid} put in short queue");
                            write.push_back((txid.clone(), vout.value))
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
        let deposits_enabled = self.deposits_enabled.read().await.clone();
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

                let mut conn = self.pool.acquire().await?;

                if let Some(confs) = raw_tx.confirmations {
                    let min_confs = self.config.application.min_deposit_confirmations_small;

                    if confs < min_confs {
                        trace!("tx needs {}, has {confs}: {}", min_confs, front.0);
                        break;
                    } else {
                        trace!("tx has at least {} confs: {}", min_confs, front.0);
                        if let Err(e) = process_txid(Arc::clone(&http), &mut conn, &raw_tx).await {
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
        let deposits_enabled = self.deposits_enabled.read().await.clone();
        if !deposits_enabled {
            warn!("deposits disabled");

            return Ok(());
        }
        let mut write = self.queue_large_txns.write().await;
        let http = Arc::clone(&self.http);
        let queue_size = write.len();
        debug!("{queue_size} transactions in long queue");

        loop {
            let mut conn = self.pool.acquire().await?;
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
                        if let Err(e) = process_txid(Arc::clone(&http), &mut conn, &raw_tx).await {
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
}

// checks if a transaction id contains an output address that belongs to a discord user
// if it exists, the balance of that user is increased
// the transactions is stored in the database such that it doesn't get processed again
// a dm is sent to the user afterwards
pub async fn process_txid(
    http: Arc<Http>,
    mut conn: &mut PgConnection,
    raw_tx: &GetRawTransactionResultVerbose,
) -> Result<(), Error> {
    if !transaction_processed(
        &mut conn,
        &raw_tx.txid,
        &Address::from_str(VRSC_CURRENCY_ID)?,
    )
    .await?
    {
        for vout in raw_tx.vout.iter() {
            if let Some(addresses) = &vout.script_pubkey.addresses {
                for address in addresses {
                    if let Some(user_id) = get_user_from_address(&mut conn, address).await? {
                        let uuid = Uuid::new_v4();
                        if let Err(e) = increase_balance(
                            &mut conn,
                            &user_id,
                            vout.value_sat,
                            &Address::from_str(VRSC_CURRENCY_ID)?,
                        )
                        .await
                        {
                            error!(
                                "something went wrong while increasing a user's balance\nuser: {user_id} txid: {} vout: {} \nerror: {:?}",
                                &raw_tx.txid, vout.n, e
                            )
                        } else {
                            if let Err(e) = store_deposit_transaction(
                                &mut conn,
                                &uuid,
                                &user_id,
                                &raw_tx.txid,
                                &Address::from_str(VRSC_CURRENCY_ID)?,
                                vout.value_sat,
                                &address,
                            )
                            .await
                            {
                                error!(
                                    "something went wrong while storing a transaction to the database: {:?}",
                                    e
                                )
                            } else {
                                send_deposit_dm(http.clone(), user_id, vout.value).await?;
                            }
                        }
                    }
                }
            } else {
                debug!("no addresses found in scriptpubkey");
            }
        }
    } else {
        debug!("transaction already processed")
    }

    Ok(())
}

async fn send_deposit_dm(http: Arc<Http>, user_id: UserId, amount: Amount) -> Result<(), Error> {
    let user = http.get_user(user_id).await?;
    user.direct_message(
        http,
        CreateMessage::new().content(format!("Your deposit of {} has been processed.", amount)),
    )
    .await?;

    Ok(())
}
