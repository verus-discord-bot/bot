use color_eyre::Report;
use poise::serenity_prelude::{Http, UserId};
use sqlx::PgPool;
use std::fs::{File, Permissions};
use std::os::unix::prelude::PermissionsExt;
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, instrument, trace};
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::bitcoin::Txid;
use vrsc_rpc::{Auth, Client, RpcApi};

use crate::configuration::Settings;
use crate::util::database::*;
use crate::Error;

pub async fn listen(http: Arc<Http>, pool: PgPool, config: Settings) {
    let listener = UnixListener::bind(&config.application.vrsc_socket_path).unwrap_or_else(|_| {
        std::fs::remove_file(&config.application.vrsc_socket_path).unwrap();
        let bind = UnixListener::bind(&config.application.vrsc_socket_path).unwrap();

        std::fs::set_permissions(
            &config.application.vrsc_socket_path,
            Permissions::from_mode(0o777),
        )
        .unwrap();

        bind
    });

    info!("walletnotify listening");
    loop {
        let http_clone = http.clone();
        let pool_clone = pool.clone();
        let config_clone = config.clone();

        match listener.accept().await {
            Ok((stream, _address)) => {
                tokio::spawn(async move {
                    if let Err(e) = handle(http_clone, pool_clone, &stream, config_clone).await {
                        error!(
                            "something went wrong while handling a new wallet tx: {:?}\n{:?}",
                            e, &stream
                        )
                    }
                })
                .await
                .unwrap();
            }
            Err(e) => {
                error!("connection to socket listener failed: {}", e)
            }
        }
    }
}

#[instrument(skip(http, pool))]
async fn handle(
    http: Arc<Http>,
    pool: PgPool,
    stream: &UnixStream,
    config: Settings,
) -> Result<(), Error> {
    stream.readable().await?;
    let tx_hash = parse_bytes(&stream).await?;
    debug!("parsed tx_hash: {}", &tx_hash);

    // todo: need to get client from main
    let client = Client::vrsc(
        config.application.testnet,
        Auth::UserPass(
            format!("127.0.0.1:{}", config.application.rpc_port),
            config.application.rpc_user,
            config.application.rpc_password,
        ),
    )?;

    let raw_tx = client.get_raw_transaction_verbose(&Txid::from_str(&tx_hash)?)?;

    if let Some(confs) = raw_tx.confirmations {
        if confs >= 1 {
            debug!("new confirmed tx: {}", &tx_hash);

            // todo need to get notified if anything below goes wrong.
            // skip if tx was already processed
            if !transaction_processed(&pool, &raw_tx.txid).await? {
                for vout in raw_tx.vout {
                    if let Some(addresses) = &vout.script_pubkey.addresses {
                        for address in addresses {
                            if let Some(user_id) = get_user_from_address(&pool, address).await? {
                                let uuid = Uuid::new_v4();
                                if let Err(e) =
                                    increase_balance(&pool, &user_id, vout.value_sat).await
                                {
                                    error!("something went wrong while increasing a user's balance\nuser: {user_id} txid: {tx_hash} vout: {} \nerror: {:?}", vout.n, e)
                                } else {
                                    if let Err(e) = store_deposit_transaction(
                                        &pool,
                                        &uuid,
                                        &user_id,
                                        &raw_tx.txid,
                                    )
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
        } else {
            // todo: confs can be negative to indicate a fork
        }
    } else {
        trace!("tx still confirming...")
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
