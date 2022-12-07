use crate::Error;
use poise::serenity_prelude::UserId;
use sqlx::PgPool;
use tracing::*;
use vrsc::{Address, Amount};
use vrsc_rpc::bitcoin::Txid;

pub async fn fetch_user(address: &Address, pool: &PgPool) -> Result<Option<UserId>, Error> {
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

pub async fn transaction_processed(pool: &PgPool, txid: &Txid) -> Result<bool, Error> {
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

pub async fn increase_balance(pool: &PgPool, user_id: UserId, amount: Amount) -> Result<(), Error> {
    info!(
        "going to increase balance for {user_id} with {} VRSC",
        amount.as_vrsc()
    );
    let result = sqlx::query!(
        "UPDATE balance_vrsc SET balance = balance + $1 WHERE discord_id = $2",
        amount.as_sat() as i64,
        user_id.0 as i64
    )
    .execute(pool)
    .await;

    match result {
        Ok(result) => info!("increasing the balance went ok! {:?}", result),
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

pub async fn store_deposit_transaction(
    pool: &PgPool,
    user_id: UserId,
    tx_hash: Txid,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO transactions_vrsc (discord_id, transaction_id, transaction_action) VALUES ($1, $2, $3)",
        user_id.0 as i64,
        &tx_hash.to_string(),
        "deposit"
        )
        .execute(pool)
        .await?;

    Ok(())
}
