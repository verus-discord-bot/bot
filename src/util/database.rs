use std::str::FromStr;

use crate::{
    commands::misc::Notification,
    reactdrop::{Reactdrop, ReactdropState},
    Error,
};
// use num_traits::cast::ToPrimitive;
use num_traits::cast::ToPrimitive;
use poise::serenity_prelude::UserId;
use sqlx::{
    types::chrono::{DateTime, Utc},
    PgPool, Postgres, QueryBuilder,
};
use tracing::*;
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::bitcoin::Txid;

pub async fn insert_discord_user(pool: &PgPool, user_id: &UserId) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO discord_users(discord_id) 
        VALUES ($1) 
        ON CONFLICT (discord_id) 
        DO NOTHING",
        user_id.0 as i64
    )
    .execute(pool)
    .await?;

    Ok(())
}

// to store multiple tip transactions at once. Usually when a group tip needs to be processed.
pub async fn store_tip_transactions(
    pool: &PgPool,
    uuid: &Uuid,
    user_ids: &Vec<UserId>,
    kind: &str,
    amount: &Amount,
    counterparty: UserId, // this is always a user
) -> Result<(), Error> {
    let mut query_builder: QueryBuilder<Postgres> =
        QueryBuilder::new("INSERT INTO tips_vrsc(uuid, discord_id, kind, amount, counterparty) ");

    let tuples = user_ids.iter().map(|user| {
        (
            uuid.to_string(),
            user.0 as i64,
            kind,
            amount.as_sat() as i64,
            counterparty.0 as i64,
        )
    });

    query_builder.push_values(tuples, |mut b, tuple| {
        b.push_bind(tuple.0)
            .push_bind(tuple.1)
            .push_bind(tuple.2)
            .push_bind(tuple.3)
            .push_bind(tuple.4);
    });

    query_builder.build().execute(pool).await?;

    Ok(())
}

/// Queries the database and retrieves the balance for the user, if it exists.
/// If there is no row for this user, None will be returned.
///
/// The database has a constraint that balances can not go below 0.
pub async fn get_balance_for_user(pool: &PgPool, user_id: &UserId) -> Result<Option<u64>, Error> {
    if let Some(row) = sqlx::query!(
        "SELECT balance FROM balance_vrsc WHERE discord_id = $1",
        user_id.0 as i64
    )
    .fetch_optional(pool)
    .await?
    {
        let balance = row.balance;
        debug!("i64 balance: {balance}");

        Ok(Some(balance as u64))
    } else {
        Ok(None)
    }
}

// process a tip from 1 user to 1 or more users.
// The tipper can tip himself.
// This function both increases the balances for the tip receivers and decreases the balance of the tipper.
// If one of these 2 actions fail, the database is not updated.
pub async fn process_a_tip(
    pool: &PgPool,
    from_user: &UserId,
    to_users: &Vec<UserId>,
    tip_amount: &Amount,
) -> Result<(), Error> {
    let mut tx = pool.begin().await?;

    let mut query_builder: QueryBuilder<Postgres> =
        QueryBuilder::new("INSERT INTO balance_vrsc (discord_id, balance) ");

    let tuples = to_users
        .iter()
        .map(|user| (user.0 as i64, tip_amount.as_sat() as i64));

    query_builder.push_values(tuples, |mut b, tuple| {
        b.push_bind(tuple.0).push_bind(tuple.1);
    });

    query_builder
        .push(" ON CONFLICT (discord_id) DO UPDATE SET balance = balance_vrsc.balance + $2");

    query_builder.build().execute(&mut tx).await?;

    debug!("updated balances");

    if let Some(mul) = tip_amount.checked_mul(to_users.len() as u64) {
        sqlx::query!(
            "UPDATE balance_vrsc SET balance = balance - $1 WHERE discord_id = $2",
            mul.as_sat() as i64,
            from_user.0 as i64
        )
        .execute(&mut tx)
        .await?;

        tx.commit().await?;

        debug!("decreased balances");
        return Ok(());
    }

    error!("something went wrong while processing a tip to multiple users");
    tx.rollback().await?;

    Ok(())
}

pub async fn store_new_address_for_user(
    pool: &PgPool,
    user_id: &UserId,
    address: &Address,
) -> Result<(), Error> {
    sqlx::query!(
        "WITH inserted_row AS (
            INSERT INTO discord_users (discord_id) 
            VALUES ($1)
            ON CONFLICT (discord_id) DO NOTHING
        )
        INSERT INTO addresses (discord_id, address)
        VALUES ($1, $2)
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
    user_id: &UserId,
) -> Result<Option<Address>, Error> {
    if let Some(row) = sqlx::query!(
        "SELECT discord_id, address FROM addresses WHERE discord_id = $1",
        user_id.0 as i64
    )
    .fetch_optional(pool)
    .await?
    {
        Ok(Some(Address::from_str(&row.address)?))
    } else {
        Ok(None)
    }
}

pub async fn get_user_from_address(
    pool: &PgPool,
    address: &Address,
) -> Result<Option<UserId>, Error> {
    if let Some(row) = sqlx::query!(
        "SELECT discord_id FROM addresses WHERE address = $1",
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
        "SELECT * FROM transactions_vrsc WHERE transaction_id = $1 AND transaction_action = 'deposit'",
        &txid.to_string()
    )
    .fetch_optional(pool)
    .await?;

    match transaction_query {
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

pub async fn increase_balance(
    pool: &PgPool,
    user_id: &UserId,
    amount: Amount,
) -> Result<(), Error> {
    debug!(
        "going to increase balance for {user_id} with {} VRSC",
        amount.as_vrsc()
    );
    let result = sqlx::query!(
        "INSERT INTO balance_vrsc (discord_id, balance)
        VALUES ($1, $2)
        ON CONFLICT (discord_id)
        DO UPDATE SET balance = balance_vrsc.balance + $2",
        user_id.0 as i64,
        amount.as_sat() as i64
    )
    .execute(pool)
    .await;

    match result {
        Ok(result) => info!("increasing the balance went ok! {:?}", result),
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

pub async fn decrease_balance(
    pool: &PgPool,
    user_id: &UserId,
    amount: &Amount,
    tx_fee: &Amount,
) -> Result<(), Error> {
    if let Some(to_decrease) = amount.checked_add(*tx_fee) {
        debug!(
            "going to decrease balance for {user_id} with {} VRSC",
            to_decrease.as_vrsc()
        );
        let result = sqlx::query!(
            "UPDATE balance_vrsc SET balance = balance - $1 WHERE discord_id = $2",
            to_decrease.as_sat() as i64,
            user_id.0 as i64
        )
        .execute(pool)
        .await;

        match result {
            Ok(result) => info!("decreasing the balance went ok! {:?}", result),
            Err(e) => {
                error!("something went wrong while decreasing the balance: {:?}", e);
                return Err(e.into());
            }
        }
    } else {
        // summing the 2 balances went wrong. This is an edge case that only happens when someone is withdrawing more than 184,467,440,737.09551615 VRSC,
        // which is more than the supply of VRSC will ever be.
        unreachable!()
        // TODO: It could be that a PBaaS chain will have such a supply, in which case we need to catch the error and inform the user. But not needed right now.
    }
    Ok(())
}

pub async fn store_deposit_transaction(
    pool: &PgPool,
    uuid: &Uuid,
    user_id: &UserId,
    tx_hash: &Txid,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO transactions_vrsc (uuid, discord_id, transaction_id, transaction_action) VALUES ($1, $2, $3, $4)",
        uuid.to_string(),
        user_id.0 as i64,
        tx_hash.to_string(),
        "deposit"
        )
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn store_withdraw_transaction(
    pool: &PgPool,
    uuid: &Uuid,
    user_id: &UserId,
    tx_hash: Option<&Txid>,
    opid: &str,
    tx_fee: &Amount,
) -> Result<(), Error> {
    let tx_hash = if let Some(tx) = tx_hash {
        tx.to_string()
    } else {
        String::from("")
    };
    sqlx::query!(
        "INSERT INTO transactions_vrsc (uuid, discord_id, transaction_id, opid, transaction_action, fee) VALUES ($1, $2, $3, $4, $5, $6)",
        uuid.to_string(),
        user_id.0 as i64,
        tx_hash,
        opid,
        "withdraw",
        tx_fee.as_sat() as i64
        )
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn store_opid(
    pool: &PgPool,
    opid: &str,
    status: &str,
    creation_time: i64,
    result: Option<Txid>,
    address: &str,
    amount: f64,
    currency: &str,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO opids (opid, status, creation_time, result, address, amount, currency) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        opid.to_string(),
        status.to_string(),
        creation_time,
        result.map_or(String::new(), |r| r.to_string()),
        address.to_string(),
        (amount * 100_000_000.0) as i64,
        currency.to_string()
        )
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn update_notifications(
    pool: &PgPool,
    user_id: &UserId,
    notification: &str,
) -> Result<(), Error> {
    // pre_command takes care of having a db row at this point for this user.
    sqlx::query!(
        "UPDATE discord_users SET notifications = ($1) WHERE discord_id = ($2)",
        notification,
        user_id.0 as i64
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_notification_settings(
    pool: &PgPool,
    user_ids: &Vec<UserId>,
) -> Result<Vec<(i64, Notification)>, Error> {
    let users = user_ids
        .iter()
        .map(|user| user.0 as i64)
        .collect::<Vec<_>>();
    let rows = sqlx::query!(
        "SELECT discord_id, notifications FROM discord_users WHERE discord_id IN (SELECT * FROM UNNEST($1::bigint[]))",
        &users
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter(|row| row.notifications.is_some())
        .map(|row| (row.discord_id, row.notifications.unwrap().into()))
        .collect())
}

pub async fn get_blacklist_status(pool: &PgPool, user_id: UserId) -> Result<Option<bool>, Error> {
    if let Some(row) = sqlx::query!(
        "SELECT blacklisted FROM discord_users WHERE discord_id = $1",
        user_id.0 as i64
    )
    .fetch_optional(pool)
    .await?
    {
        return Ok(row.blacklisted);
    } else {
        Ok(None)
    }
}

// TODO user might not exist?
pub async fn set_blacklist_status(
    pool: &PgPool,
    user_id: UserId,
    blacklist: bool,
) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE discord_users SET blacklisted = $1 WHERE discord_id = $2",
        blacklist,
        user_id.0 as i64
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn store_unprocessed_transaction(pool: &PgPool, txid: &Txid) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO unprocessed_transactions (txid, status) VALUES ($1, $2)",
        &txid.to_string(),
        "unprocessed"
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_stored_txids(pool: &PgPool) -> Result<Vec<Txid>, Error> {
    let rows =
        sqlx::query!("SELECT txid FROM unprocessed_transactions WHERE status = 'unprocessed'")
            .fetch_all(pool)
            .await?;

    return Ok(rows
        .into_iter()
        .map(|row| Txid::from_str(&row.txid).unwrap())
        .collect::<Vec<_>>());
}

pub async fn set_stored_txid_to_processed(pool: &PgPool, txid: &Txid) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE unprocessed_transactions SET status = 'processed' WHERE txid = $1",
        &txid.to_string(),
    )
    .execute(pool)
    .await?;

    Ok(())
}

// sums all the balances currently in the database and returns them
pub async fn get_total_balance(pool: &PgPool) -> Result<u64, Error> {
    let record = sqlx::query!("SELECT SUM(CAST(balance AS BIGINT)) FROM balance_vrsc")
        .fetch_one(pool)
        .await?;

    if let Some(balance) = record.sum {
        return Ok(balance.to_u64().unwrap());
    }

    Ok(0)
}

pub async fn get_total_tipped(pool: &PgPool) -> Result<u64, Error> {
    let record = sqlx::query!("SELECT SUM(CAST(amount AS BIGINT)) FROM tips_vrsc")
        .fetch_one(pool)
        .await?;

    if let Some(total) = record.sum {
        return Ok(total.to_u64().unwrap());
    }

    Ok(0)
}

pub async fn get_largest_tip(pool: &PgPool) -> Result<u64, Error> {
    let record = sqlx::query!("SELECT MAX(amount) FROM tips_vrsc")
        .fetch_one(pool)
        .await?;

    if let Some(max) = record.max {
        return Ok(max.to_u64().unwrap());
    }

    Ok(0)
}

pub async fn get_all_txids(pool: &PgPool, transaction_action: &str) -> Result<Vec<Txid>, Error> {
    let rows = sqlx::query!(
        "SELECT transaction_id FROM transactions_vrsc WHERE transaction_action = $1",
        transaction_action
    )
    .fetch_all(pool)
    .await?;

    return Ok(rows
        .into_iter()
        .map(|row| Txid::from_str(&row.transaction_id).unwrap())
        .collect::<Vec<_>>());
}

pub async fn insert_reactdrop(
    pool: &PgPool,
    emoji: String,
    amount: i64,
    channel_id: i64,
    message_id: i64,
    finish_time: DateTime<Utc>,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO reactdrops(channel_id, message_id, finish_time, emojistr, amount, status) \
    VALUES ($1, $2, $3, $4, $5, 'pending') \
    ON CONFLICT (channel_id, message_id) \
    DO NOTHING",
        channel_id,
        message_id,
        finish_time,
        emoji,
        amount,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Returns pending reactdrops, or an emtpy Vec if no pending reactdrops present
pub async fn get_pending_reactdrops(pool: &PgPool) -> Result<Vec<Reactdrop>, Error> {
    let rows = sqlx::query!(
        "SELECT * \
FROM reactdrops \
WHERE status = 'pending'"
    )
    .fetch_all(pool)
    .await?;

    let vec: Vec<Reactdrop> = rows
        .into_iter()
        .map(|row| Reactdrop {
            status: crate::reactdrop::ReactdropState::Pending,
            emoji: row.emojistr,
            tip_amount: Amount::from_sat(row.amount as u64),
            channel_id: (row.channel_id as u64).into(),
            message_id: (row.message_id as u64).into(),
            finish_time: row.finish_time,
        })
        .collect();

    Ok(vec)
}

pub async fn update_reactdrop(
    pool: &PgPool,
    channel_id: i64,
    message_id: i64,
    status: ReactdropState,
) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE reactdrops SET status = $3 WHERE channel_id = $1 AND message_id = $2",
        channel_id,
        message_id,
        status.to_string()
    )
    .execute(pool)
    .await?;

    Ok(())
}
