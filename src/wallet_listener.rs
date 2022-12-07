use color_eyre::Report;
use poise::serenity_prelude::{Http, UserId};
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, instrument};
use vrsc::Amount;
use vrsc_rpc::bitcoin::Txid;
use vrsc_rpc::{Auth, Client, RpcApi};

use crate::util::database::*;
use crate::Error;

pub async fn listen(http: Arc<Http>, pool: PgPool) {
    let listener = UnixListener::bind("/tmp/discord_bot.sock").unwrap_or_else(|_| {
        std::fs::remove_file("/tmp/discord_bot.sock").unwrap();
        UnixListener::bind("/tmp/discord_bot.sock").unwrap()
    });

    info!("walletnotify listening");
    loop {
        let http_clone = http.clone();
        let pool_clone = pool.clone();

        match listener.accept().await {
            Ok((stream, _address)) => {
                tokio::spawn(async move {
                    if let Err(e) = handle(http_clone, pool_clone, stream).await {
                        error!(
                            "something went wrong while handling a new wallet tx: {:?}",
                            e
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
async fn handle(http: Arc<Http>, pool: PgPool, stream: UnixStream) -> Result<(), Error> {
    //
    stream.readable().await?;
    let tx_hash = parse_bytes(&stream).await?;

    // todo: need to get client from main
    let client = Client::vrsc(true, Auth::ConfigFile)?;

    let raw_tx = client.get_raw_transaction_verbose(&Txid::from_str(&tx_hash)?)?;

    if let Some(confs) = raw_tx.confirmations {
        if confs >= 1 {
            info!("new confirmed tx: {}", &tx_hash);

            // todo need to get notified if anything below goes wrong.
            // skip if tx was already processed
            if !transaction_processed(&pool, &raw_tx.txid).await? {
                for vout in raw_tx.vout {
                    if let Some(addresses) = &vout.script_pubkey.addresses {
                        for address in addresses {
                            if let Some(user_id) = fetch_user(address, &pool).await? {
                                info!("user found: {user_id}");
                                let result = increase_balance(&pool, user_id, vout.value_sat).await;
                                match result {
                                    Ok(_) => {
                                        if let Err(e) =
                                            store_deposit_transaction(&pool, user_id, raw_tx.txid).await
                                        {
                                            error!("something went wrong while storing a transaction to the database: {:?}", e)
                                        } else {
                                            send_dm(http.clone(), user_id, vout.value).await?;
                                        }

                                    }
                                    Err(e) => error!("something went wrong while increasing a user's balance\nuser: {user_id} txid: {tx_hash} vout: {} \nerror: {:?}", vout.n, e),
                                }
                            }
                        }
                    }
                }
            } else {
                debug!("transaction already processed")
            }
        }
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

async fn send_dm(http: Arc<Http>, user_id: UserId, amount: Amount) -> Result<(), Error> {
    let user = http.get_user(user_id.0).await?;
    user.direct_message(http, |message| {
        message.content(format!("Your deposit of {} has been processed.", amount))
    })
    .await?;

    Ok(())
}
