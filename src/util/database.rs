use std::str::FromStr;

use crate::Error;
use poise::serenity_prelude::UserId;
use sqlx::PgPool;
use tracing::*;
use vrsc::{Address, Amount};
use vrsc_rpc::bitcoin::Txid;

pub async fn store_new_address_for_user(
    pool: &PgPool,
    user_id: UserId,
    address: &Address,
) -> Result<(), Error> {
    sqlx::query!(
        "WITH inserted_row AS (
            INSERT INTO discord_users (discord_id, vrsc_address) 
            VALUES ($1, $2)
        )
        INSERT INTO balance_vrsc (discord_id)
        VALUES ($1)
        ",
        user_id.0 as i64,
        &address.to_string()
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_address_from_user(
    pool: &PgPool,
    user_id: UserId,
) -> Result<Option<Address>, Error> {
    if let Some(row) = sqlx::query!(
        "SELECT discord_id, vrsc_address FROM discord_users WHERE discord_id = $1",
        user_id.0 as i64
    )
    .fetch_optional(pool)
    .await?
    {
        Ok(Some(Address::from_str(&row.vrsc_address)?))
    } else {
        Ok(None)
    }
}

pub async fn get_user_from_address(
    pool: &PgPool,
    address: &Address,
) -> Result<Option<UserId>, Error> {
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
    debug!(
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
