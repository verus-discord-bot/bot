// use std::io::{BufRead, BufReader};
// use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
// use std::thread;

use color_eyre::Report;

use poise::serenity_prelude::{Http, UserId};
use sqlx::PgPool;
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info};
use vrsc::{Address, Amount};
use vrsc_rpc::bitcoin::{TxIn, Txid};
use vrsc_rpc::{Auth, Client, RpcApi};

use crate::Error;

use std::str::FromStr;

pub async fn listen(http: Arc<Http>, pool: PgPool) {
    let listener = UnixListener::bind("/tmp/discord_bot.sock").unwrap_or_else(|_| {
        std::fs::remove_file("/tmp/discord_bot.sock").unwrap();
        UnixListener::bind("/tmp/discord_bot.sock").unwrap()
    });

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

async fn fetch_user(address: &Address, pool: &PgPool) -> Result<Option<UserId>, Error> {
    if let Some(row) = sqlx::query!(
        "SELECT discord_id FROM discord_users WHERE vrsc_address = $1",
        &address.to_string()
    )
    .fetch_optional(pool)
    .await?
    {
        Ok(Some(UserId(row.discord_id as u64)))
    } else {
        Ok(None)
    }
}

async fn transaction_processed(pool: &PgPool, txid: &Txid) -> Result<bool, Error> {
    let transaction_query = sqlx::query!(
        "SELECT * FROM transactions_vrsc WHERE (transaction_id) = ($1)",
        &txid.to_string()
    )
    .fetch_optional(pool)
    .await?;

    match transaction_query {
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

async fn increase_balance(pool: &PgPool, user_id: UserId, amount: Amount) -> Result<(), Error> {
    debug!("this user_id was found for the incoming tx: {:?}", user_id);

    // now we check if the transaction was already processed

    //         let query_result = sqlx::query!(
    //             "UPDATE discord_users SET balance = balance + $1 WHERE discord_id = $2",
    //             vout.value.as_sat() as i64,
    //             discord_id.discord_id
    //         )
    //         .execute(&pool)
    //         .await;

    //         if let Ok(_) = query_result {
    //             info!("the query worked");

    //             sqlx::query!(
    //     "INSERT INTO transactions (discord_id, transaction_id, transaction_action) VALUES ($1, $2, $3)",
    //     discord_id.discord_id as i64,
    //     &tx_hash,
    //     "deposit"
    // )
    // .execute(&pool)
    // .await?;

    Ok(())
}

async fn store_transaction(pool: &PgPool, user_id: UserId, amount: Amount) -> Result<(), Error> {
    todo!()
}

async fn handle(_http: Arc<Http>, pool: PgPool, stream: UnixStream) -> Result<(), Error> {
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
                                let result = increase_balance(&pool, user_id, vout.value_sat).await;
                                if result.is_ok() {
                                    store_transaction(&pool, user_id, vout.value_sat).await?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // http.
    // let pool = &ctx.data()._database;
    // let client = &ctx.data().verus;

    // let raw_tx = client.get_raw_transaction_verbose(&Txid::from_str(&tx_hash)?)?;

    // // debug!("{:?}", &raw_tx);

    // if let Some(confirmations) = raw_tx.confirmations {
    //     if confirmations >= 1 {
    //         for vout in raw_tx.vout {
    //             let addresses = &vout.script_pubkey.addresses.unwrap();
    //             let address = addresses.first().unwrap();
    //             info!("{:?}", &address);

    //             // let discord_id = sqlx::query!(
    //             //     "SELECT discord_id FROM discord_users WHERE tokel_address = $1",
    //             //     &address.to_string()
    //             // )
    //             // .fetch_optional(&pool)
    //             // .await?;

    //             // debug!("{:?}", &discord_id);

    //             //     if let Some(discord_id) = discord_id {
    //             //         info!(
    //             //             "this discord_id was found for the incoming tx: {:?}",
    //             //             discord_id.discord_id
    //             //         );

    //             //         // now we check if the transaction was already processed
    //             //         let transaction_query = sqlx::query!(
    //             //             "SELECT * FROM transactions WHERE (discord_id, transaction_id) = ($1, $2)",
    //             //             &discord_id.discord_id,
    //             //             &tx_hash
    //             //         )
    //             //         .fetch_optional(&pool)
    //             //         .await?;
    //             //         if let Some(row) = transaction_query {
    //             //             info!("this transaction was already processed, ignore");
    //             //             continue;
    //             //         }

    //             //         let query_result = sqlx::query!(
    //             //             "UPDATE discord_users SET balance = balance + $1 WHERE discord_id = $2",
    //             //             vout.value.as_sat() as i64,
    //             //             discord_id.discord_id
    //             //         )
    //             //         .execute(&pool)
    //             //         .await;

    //             //         if let Ok(_) = query_result {
    //             //             info!("the query worked");

    //             //             sqlx::query!(
    //             //     "INSERT INTO transactions (discord_id, transaction_id, transaction_action) VALUES ($1, $2, $3)",
    //             //     discord_id.discord_id as i64,
    //             //     &tx_hash,
    //             //     "deposit"
    //             // )
    //             // .execute(&pool)
    //             // .await?;
    //             //         }
    //             //     }
    //         }
    //     }
    // }
    Ok(())
}

// async fn listen_on_socket(ctx: Context<'_>) -> Result<(), Report> {
//     let listener = UnixListener::bind("/tmp/discord_bot.sock").unwrap_or_else(|_| {
//         std::fs::remove_file("/tmp/discord_bot.sock").unwrap();
//         UnixListener::bind("/tmp/discord_bot.sock").unwrap()
//     });

//     loop {
//         // let ctx_clone = ctx.clone();
//         let clone = ctx.clone();
//         match listener.accept().await {
//             Ok((stream, _)) => {
//                 tokio::spawn(async {
//                     handle(stream).await;
//                 });
//             }
//             Err(e) => {
//                 error!("connection failed: {}", e)
//             }
//         }
//     }
// }

// async fn handle(stream: UnixStream) -> Result<(), Error> {
// stream.readable().await?;
// let tx_hash = parse_bytes(&stream).await?;
// info!("new tx: {}", &tx_hash);

// let pool = &ctx.data()._database;
// let client = &ctx.data().verus;

// let raw_tx = client.get_raw_transaction_verbose(&Txid::from_str(&tx_hash)?)?;

// // debug!("{:?}", &raw_tx);

// if let Some(confirmations) = raw_tx.confirmations {
//     if confirmations >= 1 {
//         for vout in raw_tx.vout {
//             let addresses = &vout.script_pubkey.addresses.unwrap();
//             let address = addresses.first().unwrap();
//             info!("{:?}", &address);

// let discord_id = sqlx::query!(
//     "SELECT discord_id FROM discord_users WHERE tokel_address = $1",
//     &address.to_string()
// )
// .fetch_optional(&pool)
// .await?;

// debug!("{:?}", &discord_id);

// if let Some(discord_id) = discord_id {
//     info!(
//         "this discord_id was found for the incoming tx: {:?}",
//         discord_id.discord_id
//     );

//     // now we check if the transaction was already processed
//     let transaction_query = sqlx::query!(
//         "SELECT * FROM transactions WHERE (discord_id, transaction_id) = ($1, $2)",
//         &discord_id.discord_id,
//         &tx_hash
//     )
//     .fetch_optional(&pool)
//     .await?;
//     if let Some(row) = transaction_query {
//         info!("this transaction was already processed, ignore");
//         continue;
//     }

//     let query_result = sqlx::query!(
//         "UPDATE discord_users SET balance = balance + $1 WHERE discord_id = $2",
//         vout.value.as_sat() as i64,
//         discord_id.discord_id
//     )
//     .execute(&pool)
//     .await;

//     if let Ok(_) = query_result {
//         info!("the query worked");

//         sqlx::query!(
//                 "INSERT INTO transactions (discord_id, transaction_id, transaction_action) VALUES ($1, $2, $3)",
//                 discord_id.discord_id as i64,
//                 &tx_hash,
//                 "deposit"
//             )
//             .execute(&pool)
//             .await?;
//     }
// }
//         }

//         Ok(())
//     } else {
//         info!("transaction did not have enough confirmations");
//         Ok(())
//     }
// } else {
//     info!("transaction is not mined yet");
//     Ok(())
// }
// }

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
