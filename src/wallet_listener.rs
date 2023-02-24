use color_eyre::Report;
use poise::serenity_prelude::{Http, UserId};
use sqlx::PgPool;
use std::collections::VecDeque;
use std::fs::Permissions;
use std::os::unix::prelude::PermissionsExt;
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument, trace};
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::bitcoin::Txid;
use vrsc_rpc::json::GetRawTransactionResultVerbose;
use vrsc_rpc::{Auth, Client, RpcApi};

use crate::configuration::Settings;
use crate::util::database::{self, *};
use crate::Error;

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
///
///

#[derive(Debug)]
pub struct TransactionProcessor {
    http: Arc<Http>,
    pool: PgPool,
    config: Settings,
    pub maintenance: Arc<RwLock<bool>>,
    pub deposits_enabled: Arc<RwLock<bool>>,
    queue_small_txns: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
    queue_large_txns: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
}

impl TransactionProcessor {
    pub fn new(
        http: Arc<Http>,
        pool: PgPool,
        config: Settings,
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

    pub async fn listen_wallet_notifications(&self) {
        let wallet_notify_socket_path = &self.config.application.vrsc_wallet_notify_socket_path;
        let wallet_listener = UnixListener::bind(&wallet_notify_socket_path).unwrap_or_else(|_| {
            std::fs::remove_file(&wallet_notify_socket_path).unwrap();
            let bind = UnixListener::bind(&wallet_notify_socket_path).unwrap();

            std::fs::set_permissions(&wallet_notify_socket_path, Permissions::from_mode(0o777))
                .unwrap();

            bind
        });

        // tokio::spawn(async move {
        loop {
            match wallet_listener.accept().await {
                Ok((stream, _address)) => {
                    let parsed_str = parse_bytes(&stream).await.expect("valid string");
                    let txid = Txid::from_str(&parsed_str).expect("valid txid");

                    // at this point, the bot could be in maintenance mode, so we should check for that.
                    // if it is in maintenance mode, we should store all the transactions in a database for later check
                    if *self.maintenance.read().await || !*self.deposits_enabled.read().await {
                        trace!("store {txid} in unprocessed_transactions");
                        if let Err(e) =
                            database::store_unprocessed_transaction(&self.pool, &txid).await
                        {
                            error!("Something went wrong while storing an unprocessed transaction: {:?}", e);
                        }

                        return;
                    }

                    if let Err(e) = self.check_tx(txid).await {
                        error!("something went wrong while checking a transaction: {:?}", e);
                    }
                }
                Err(e) => {
                    error!("connection to socket listener failed: {}", e);

                    break;
                }
            }
        }
        // });
        info!("walletnotify listening");
    }

    pub async fn listen_block_notifications(&self) {
        let block_notify_socket_path = &self.config.application.vrsc_block_notify_socket_path;
        let block_listener = UnixListener::bind(&block_notify_socket_path).unwrap_or_else(|_| {
            std::fs::remove_file(&block_notify_socket_path).unwrap();
            let bind = UnixListener::bind(&block_notify_socket_path).unwrap();

            std::fs::set_permissions(&block_notify_socket_path, Permissions::from_mode(0o777))
                .unwrap();

            bind
        });

        let deposits_enabled = self.deposits_enabled.read().await.clone();

        loop {
            match block_listener.accept().await {
                Ok((_stream, _address)) => loop {
                    if deposits_enabled == false {
                        // deposits are disabled, let's return
                        info!("deposits are disabled");
                        break;
                    }

                    self.process_short_queue().await.unwrap();
                    self.process_long_queue().await.unwrap();

                    break;
                },
                Err(e) => {
                    error!("connection to socket listener failed: {}", e);

                    break;
                }
            }
        }

        info!("blocknotify listening");
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

        for vout in raw_tx.vout.iter() {
            if let Some(addresses) = &vout.script_pubkey.addresses {
                for address in addresses {
                    if let Some(user_id) = get_user_from_address(&self.pool, address).await? {
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
    pub async fn process_short_queue(
        // queue: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
        // config: &Settings,
        // pool: &PgPool,
        // http: Arc<Http>,
        &self,
    ) -> Result<(), Error> {
        let mut write = self.queue_small_txns.write().await;
        let http = Arc::clone(&self.http);
        let pool = self.pool.clone();
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
                        if let Err(e) = process_txid(Arc::clone(&http), &pool, &raw_tx).await {
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
        let mut write = self.queue_large_txns.write().await;
        let http = Arc::clone(&self.http);
        let pool = self.pool.clone();
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
                        if let Err(e) = process_txid(Arc::clone(&http), &pool, &raw_tx).await {
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
    pool: &PgPool,
    raw_tx: &GetRawTransactionResultVerbose,
    // _config: ?Settings,
) -> Result<(), Error> {
    if !transaction_processed(&pool, &raw_tx.txid).await? {
        for vout in raw_tx.vout.iter() {
            if let Some(addresses) = &vout.script_pubkey.addresses {
                for address in addresses {
                    if let Some(user_id) = get_user_from_address(&pool, address).await? {
                        let uuid = Uuid::new_v4();
                        if let Err(e) = increase_balance(&pool, &user_id, vout.value_sat).await {
                            error!("something went wrong while increasing a user's balance\nuser: {user_id} txid: {} vout: {} \nerror: {:?}", &raw_tx.txid, vout.n, e)
                        } else {
                            if let Err(e) =
                                store_deposit_transaction(&pool, &uuid, &user_id, &raw_tx.txid)
                                    .await
                            {
                                error!("something went wrong while storing a transaction to the database: {:?}", e)
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

async fn parse_bytes(stream: &UnixStream) -> Result<String, Report> {
    stream.readable().await?;

    let mut data = vec![0; 64];
    match stream.try_read(&mut data) {
        Ok(_) => {
            let tx_hash = String::from_utf8(data)?;
            return Ok(tx_hash);
        }
        Err(e) => {
            return Err(e.into());
        }
    }
}

async fn send_deposit_dm(http: Arc<Http>, user_id: UserId, amount: Amount) -> Result<(), Error> {
    let user = http.get_user(user_id.0).await?;
    user.direct_message(http, |message| {
        message.content(format!("Your deposit of {} has been processed.", amount))
    })
    .await?;

    Ok(())
}
