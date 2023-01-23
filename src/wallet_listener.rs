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
use crate::util::database::*;
use crate::Error;

pub struct TransactionProcessor {
    queue: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
    queueue: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
}

impl TransactionProcessor {
    pub fn new() -> Self {
        TransactionProcessor {
            queue: Arc::new(RwLock::new(VecDeque::new())),
            queueue: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    pub async fn listen(
        &mut self,
        http: Arc<Http>,
        pool: PgPool,
        config: Settings,
        deposits_enabled: Arc<RwLock<bool>>,
    ) {
        // walletnotify
        let wallet_notify_socket_path = &config.application.vrsc_wallet_notify_socket_path;
        let wallet_listener = UnixListener::bind(&wallet_notify_socket_path).unwrap_or_else(|_| {
            std::fs::remove_file(&wallet_notify_socket_path).unwrap();
            let bind = UnixListener::bind(&wallet_notify_socket_path).unwrap();

            std::fs::set_permissions(&wallet_notify_socket_path, Permissions::from_mode(0o777))
                .unwrap();

            bind
        });

        let block_notify_socket_path = &config.application.vrsc_block_notify_socket_path;
        let block_listener = UnixListener::bind(&block_notify_socket_path).unwrap_or_else(|_| {
            std::fs::remove_file(&block_notify_socket_path).unwrap();
            let bind = UnixListener::bind(&block_notify_socket_path).unwrap();

            std::fs::set_permissions(&block_notify_socket_path, Permissions::from_mode(0o777))
                .unwrap();

            bind
        });

        let queue_clone = self.queue.clone();
        let queueue_clone = self.queueue.clone();
        let pool_clone = pool.clone();
        let config_clone = config.clone();

        tokio::spawn(async move {
            loop {
                match wallet_listener.accept().await {
                    Ok((stream, _address)) => {
                        let parsed_str = parse_bytes(&stream).await.expect("valid string");
                        let txid = Txid::from_str(&parsed_str).expect("valid txid");

                        let client = Client::vrsc(
                            config_clone.application.testnet,
                            Auth::UserPass(
                                format!("127.0.0.1:{}", config_clone.application.rpc_port),
                                config_clone.application.rpc_user.clone(),
                                config_clone.application.rpc_password.clone(),
                            ),
                        )
                        .unwrap();

                        let raw_tx = client.get_raw_transaction_verbose(&txid).unwrap();

                        for vout in raw_tx.vout.iter() {
                            if let Some(addresses) = &vout.script_pubkey.addresses {
                                for address in addresses {
                                    if let Some(user_id) =
                                        get_user_from_address(&pool_clone, address).await.unwrap()
                                    {
                                        trace!("there is a user for this address: {user_id}",);
                                        let mut write = queue_clone.write().await;
                                        let mut long_write = queueue_clone.write().await;

                                        // if the value of the incoming transaction is greater than
                                        if vout.value.gt(&config.application.min_deposit_threshold)
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
                                debug!("no addresses found in scriptpubkey");
                            }
                        }
                    }
                    Err(e) => {
                        error!("connection to socket listener failed: {}", e)
                    }
                }
            }
        });
        info!("walletnotify listening");

        let config_clone = config.clone();
        let queue_clone = self.queue.clone();
        let queueue_clone = self.queueue.clone();

        tokio::spawn(async move {
            loop {
                match block_listener.accept().await {
                    Ok((_stream, _address)) => loop {
                        if *deposits_enabled.read().await == false {
                            // deposits are disabled, let's return
                            debug!("deposits are disabled, break");
                            break;
                        }

                        process_short_queue(
                            queue_clone.clone(),
                            &config_clone,
                            &pool,
                            http.clone(),
                        )
                        .await;
                        process_long_queue(
                            queueue_clone.clone(),
                            &config_clone,
                            &pool,
                            http.clone(),
                        )
                        .await;

                        break;
                    },
                    Err(e) => {
                        error!("connection to socket listener failed: {}", e)
                    }
                }
            }
        });
        info!("blocknotify listening");
    }
}

#[instrument]
async fn process_short_queue(
    queue: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
    config: &Settings,
    pool: &PgPool,
    http: Arc<Http>,
) {
    let mut write = queue.write().await;
    let queue_size = write.len();
    debug!("{queue_size} transactions in short queue");

    loop {
        if let Some(front) = write.front() {
            trace!("read {front:?} from front");

            let client = Client::vrsc(
                config.application.testnet,
                Auth::UserPass(
                    format!("127.0.0.1:{}", config.application.rpc_port),
                    config.application.rpc_user.clone(),
                    config.application.rpc_password.clone(),
                ),
            )
            .unwrap();

            let raw_tx = client.get_raw_transaction_verbose(&front.0).unwrap();

            if let Some(confs) = raw_tx.confirmations {
                let min_confs = config.application.min_deposit_confirmations_small;

                if confs < min_confs {
                    trace!("tx needs {}, has {confs}: {}", min_confs, front.0);
                    break;
                } else {
                    trace!("tx has at least {} confs: {}", min_confs, front.0);
                    if let Err(e) =
                        handle(Arc::clone(&http), pool.clone(), &raw_tx, config.clone()).await
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
}

#[instrument]
async fn process_long_queue(
    queue: Arc<RwLock<VecDeque<(Txid, Amount)>>>,
    config: &Settings,
    pool: &PgPool,
    http: Arc<Http>,
) {
    let mut write = queue.write().await;
    let queue_size = write.len();
    debug!("{queue_size} transactions in long queue");

    loop {
        if let Some(front) = write.front() {
            trace!("read {front:?} from front");

            let client = Client::vrsc(
                config.application.testnet,
                Auth::UserPass(
                    format!("127.0.0.1:{}", config.application.rpc_port),
                    config.application.rpc_user.clone(),
                    config.application.rpc_password.clone(),
                ),
            )
            .unwrap();

            let raw_tx = client.get_raw_transaction_verbose(&front.0).unwrap();

            if let Some(confs) = raw_tx.confirmations {
                let min_confs = config.application.min_deposit_confirmations_large;

                if confs < min_confs {
                    trace!("tx needs {}, has {confs}: {}", min_confs, front.0);
                    break;
                } else {
                    trace!("tx has at least {} confs: {}", min_confs, front.0);
                    if let Err(e) =
                        handle(Arc::clone(&http), pool.clone(), &raw_tx, config.clone()).await
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
}

// #[instrument(skip(http, pool, raw_tx, _config))]
async fn handle(
    http: Arc<Http>,
    pool: PgPool,
    raw_tx: &GetRawTransactionResultVerbose,
    _config: Settings,
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
