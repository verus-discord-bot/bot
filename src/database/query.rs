use std::str::FromStr;

use crate::{
    Error,
    commands::misc::Notification,
    reactdrop::{Reactdrop, ReactdropState},
};
use num_traits::cast::ToPrimitive;
use poise::serenity_prelude::UserId;
use sqlx::{
    PgConnection, Postgres, Transaction,
    types::chrono::{DateTime, Utc},
};
use tracing::*;
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::bitcoin::Txid;

// pub async fn insert_discord_user(conn: &mut PgConnection, user_id: &UserId) -> Result<(), Error> {
//     sqlx::query!(
//         "INSERT INTO discord_users(discord_id)
//         VALUES ($1)
//         ON CONFLICT (discord_id)
//         DO NOTHING",
//         user_id.get() as i64
//     )
//     .execute(conn)
//     .await?;

//     Ok(())
// }

// to store multiple tip transactions at once. Usually when a group tip needs to be processed.
// Because it all ends up as a transaction, it's fine to have multiple INSERT statements.
pub async fn store_tip_transactions(
    conn: &mut PgConnection,
    uuid: &Uuid,
    tippees: &Vec<UserId>,
    kind: &str,
    amount: Amount,
    tipper: UserId, // this is always a user
    currency_id: &Address,
) -> Result<(), Error> {
    for tippee in tippees {
        sqlx::query!(
            "INSERT INTO tips (uuid, currency_id, discord_id, kind, amount, counterparty)
            VALUES ($1, $2, $3, $4, $5, $6)",
            uuid.to_string(),
            currency_id.to_string(),
            tippee.get() as i64,
            kind,
            amount.as_sat() as i64,
            tipper.get() as i64,
        )
        .execute(&mut *conn)
        .await?;
    }

    Ok(())
}

/// Queries the database and retrieves the balance for the user, if it exists.
/// If there is no row for this user, None will be returned.
///
/// The database has a constraint that balances can not go below 0.
pub async fn get_balance_for_user(
    conn: &mut PgConnection,
    user_id: UserId,
    currency_id: &Address,
) -> Result<Option<u64>, Error> {
    let amount = sqlx::query!(
        "SELECT balance 
        FROM balances 
        WHERE discord_id = $1 AND
            currency_id = $2",
        user_id.get() as i64,
        currency_id.to_string()
    )
    .fetch_optional(conn)
    .await?
    .map(|row| row.balance as u64);

    Ok(amount)
}

// process a tip from 1 user to 1 or more users.
// The tipper can tip himself.
// This function both increases the balances for the tip receivers and decreases the balance of the tipper.
// If one of these 2 actions fail, the database is not updated.
pub async fn process_a_tip(
    tx: &mut Transaction<'_, Postgres>,
    tipper: UserId,
    tippees: &[UserId],
    amount: Amount,
    currency_id: &Address,
) -> Result<(), Error> {
    for tippee in tippees {
        sqlx::query!(
            "INSERT INTO balances (currency_id, discord_id, balance)
            VALUES ($1, $2, $3)
            ON CONFLICT (currency_id, discord_id)
            DO UPDATE
            SET balance = balances.balance + excluded.balance",
            currency_id.to_string(),
            tippee.get() as i64,
            amount.as_sat() as i64
        )
        .execute(&mut **tx)
        .await?;
    }

    if let Some(mul) = amount.checked_mul(tippees.len() as u64) {
        sqlx::query!(
            "UPDATE balances 
            SET balance = balance - $1
            WHERE discord_id = $2 AND 
            currency_id = $3",
            mul.as_sat() as i64,
            tipper.get() as i64,
            currency_id.to_string()
        )
        .execute(&mut **tx)
        .await?;

        trace!("decreased balances");
        return Ok(());
    }

    Ok(())
}

pub async fn store_new_address_for_user(
    conn: &mut PgConnection,
    user_id: &UserId,
    address: &Address,
    currency_id: &Address,
) -> Result<(), Error> {
    sqlx::query!(
        // "WITH inserted_row AS (
        //     INSERT INTO discord_users (discord_id)
        //     VALUES ($1)
        //     ON CONFLICT (discord_id) DO NOTHING
        // )
        "
        INSERT INTO addresses (discord_id, address, currency_id)
        VALUES ($1, $2, $3)
        ",
        user_id.get() as i64,
        &address.to_string(),
        currency_id.to_string()
    )
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn get_address_from_user(
    conn: &mut PgConnection,
    user_id: &UserId,
    currency_id: &Address,
) -> Result<Option<Address>, Error> {
    let address = sqlx::query!(
        "SELECT discord_id, address 
        FROM addresses
        WHERE discord_id = $1 AND
          currency_id = $2",
        user_id.get() as i64,
        currency_id.to_string()
    )
    .fetch_optional(conn)
    .await?
    .map(|row| Address::from_str(&row.address))
    .transpose()?;

    Ok(address)
}

pub async fn get_user_from_address(
    conn: &mut PgConnection,
    address: &Address,
) -> Result<Option<UserId>, Error> {
    // the chance of collision is 2^256, so we don't need currency_id
    let user = sqlx::query!(
        "SELECT discord_id FROM addresses WHERE address = $1",
        &address.to_string()
    )
    .fetch_optional(conn)
    .await?
    .map(|row| UserId::new(row.discord_id as u64));

    Ok(user)
}

pub async fn transaction_processed(
    conn: &mut PgConnection,
    txid: &Txid,
    currency_id: &Address,
) -> Result<bool, Error> {
    let is_processed = sqlx::query!(
        "SELECT * 
        FROM transactions 
        WHERE transaction_id = $1 AND 
        transaction_action = 'deposit' AND
        currency_id = $2",
        &txid.to_string(),
        currency_id.to_string()
    )
    .fetch_optional(conn)
    .await
    .map(|r| r.is_some())?;

    Ok(is_processed)
}

pub async fn increase_balance(
    conn: &mut PgConnection,
    user_id: &UserId,
    amount: Amount,
    currency_id: &Address,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO balances (discord_id, balance, currency_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (discord_id, currency_id)
        DO UPDATE 
        SET balance = balances.balance + excluded.balance",
        user_id.get() as i64,
        amount.as_sat() as i64,
        currency_id.to_string()
    )
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn decrease_balance(
    conn: &mut PgConnection,
    user_id: &UserId,
    amount: &Amount,
    tx_fee: &Amount,
    currency_id: &Address,
) -> Result<(), Error> {
    if let Some(to_decrease) = amount.checked_add(*tx_fee) {
        sqlx::query!(
            "UPDATE balances 
            SET balance = balance - $1 
            WHERE discord_id = $2 AND
            currency_id = $3",
            to_decrease.as_sat() as i64,
            user_id.get() as i64,
            currency_id.to_string()
        )
        .execute(conn)
        .await?;
    } else {
        // summing the 2 balances went wrong. This is an edge case that only happens when someone
        // is withdrawing more than 184,467,440,737.09551615 VRSC,
        // which is more than the supply of VRSC will ever be.
        unreachable!()
        // TODO: It could be that a PBaaS chain will have such a supply, in which case we need to
        // catch the error and inform the user. But not needed right now.
    }

    Ok(())
}

pub async fn store_deposit_transaction(
    conn: &mut PgConnection,
    uuid: &Uuid,
    user_id: &UserId,
    tx_hash: &Txid,
    currency_id: &Address,
    amount: Amount,
    address: &Address,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO transactions (
        uuid, 
        discord_id, 
        transaction_id, 
        transaction_action, 
        currency_id,
        amount,
        address
    ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        uuid.to_string(),
        user_id.get() as i64,
        tx_hash.to_string(),
        "deposit",
        currency_id.to_string(),
        amount.as_sat() as i64,
        &address.to_string()
    )
    .execute(conn)
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn store_withdraw_transaction(
    conn: &mut PgConnection,
    uuid: &Uuid,
    user_id: &UserId,
    tx_hash: Option<&Txid>,
    opid: &str,
    fee: &Amount,
    currency_id: &Address,
    amount: Amount,
    address: &Address,
    tx_fee: Amount,
) -> Result<(), Error> {
    let tx_hash = tx_hash.map(|tx| tx.to_string()).unwrap_or_default();

    sqlx::query!(
        "INSERT INTO transactions (
            uuid, 
            discord_id, 
            transaction_id, 
            opid,
            transaction_action, 
            fee,
            currency_id,
            amount,
            address,
            tx_fee
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        uuid.to_string(),
        user_id.get() as i64,
        tx_hash,
        opid,
        "withdraw",
        fee.as_sat() as i64,
        currency_id.to_string(),
        amount.as_sat() as i64,
        &address.to_string(),
        tx_fee.as_sat() as i64
    )
    .execute(conn)
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn store_opid(
    conn: &mut PgConnection,
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
        .execute(conn)
        .await?;

    Ok(())
}

pub async fn update_notifications(
    conn: &mut PgConnection,
    user_id: &UserId,
    notification: &str,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO notifications (discord_id, loudness)
        VALUES ($1, $2)
        ON CONFLICT (discord_id) 
        DO UPDATE SET loudness = $2",
        user_id.get() as i64,
        notification,
    )
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn get_loudness_setting(
    conn: &mut PgConnection,
    user_id: UserId,
) -> Result<Option<Notification>, Error> {
    let row = sqlx::query!(
        "SELECT loudness FROM notifications WHERE discord_id = $1",
        user_id.get() as i64
    )
    .fetch_optional(conn)
    .await?;

    Ok(row.and_then(|r| r.loudness.map(Notification::from)))
}

pub async fn get_blacklist_status(
    conn: &mut PgConnection,
    user_id: UserId,
) -> Result<Option<bool>, Error> {
    let is_blacklisted = sqlx::query!(
        "SELECT blacklisted FROM blacklist WHERE discord_id = $1",
        user_id.get() as i64
    )
    .fetch_optional(conn)
    .await?
    .map(|r| r.blacklisted);

    Ok(is_blacklisted)
}

pub async fn set_blacklist_status(
    conn: &mut PgConnection,
    user_id: UserId,
    blacklist: bool,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO blacklist (discord_id, blacklisted)
        VALUES ($1, $2) 
        ON CONFLICT (discord_id)
        DO UPDATE SET 
        blacklisted = excluded.blacklisted",
        user_id.get() as i64,
        blacklist,
    )
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn get_stored_txids(conn: &mut PgConnection) -> Result<Vec<Txid>, Error> {
    let rows =
        sqlx::query!("SELECT txid FROM unprocessed_transactions WHERE status = 'unprocessed'")
            .fetch_all(conn)
            .await?;

    Ok(rows
        .into_iter()
        .map(|row| Txid::from_str(&row.txid).unwrap())
        .collect::<Vec<_>>())
}

pub async fn set_stored_txid_to_processed(
    conn: &mut PgConnection,
    txid: &Txid,
) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE unprocessed_transactions SET status = 'processed' WHERE txid = $1",
        &txid.to_string(),
    )
    .execute(conn)
    .await?;

    Ok(())
}

// sums all the balances currently in the database and returns them
pub async fn get_total_balance(
    conn: &mut PgConnection,
    currency_id: &Address,
) -> Result<u64, Error> {
    let record = sqlx::query!(
        r#"SELECT COALESCE(SUM(CAST(balance AS BIGINT)), 0) AS "sum!"
        FROM balances
        WHERE currency_id = $1"#,
        currency_id.to_string()
    )
    .fetch_one(&mut *conn)
    .await?;

    let res = record.sum.to_u64().unwrap_or_default();

    Ok(res)
}

pub async fn get_total_tipped(
    conn: &mut PgConnection,
    currency_id: &Address,
) -> Result<u64, Error> {
    let record = sqlx::query!(
        r#"SELECT COALESCE(SUM(CAST(amount AS BIGINT)), 0) AS "sum!" 
        FROM tips 
        WHERE currency_id = $1"#,
        currency_id.to_string()
    )
    .fetch_one(&mut *conn)
    .await?;

    let res = record.sum.to_u64().unwrap_or_default();

    Ok(res)
}

pub async fn get_largest_tip(conn: &mut PgConnection, currency_id: &Address) -> Result<u64, Error> {
    let record = sqlx::query!(
        r#"SELECT COALESCE(MAX(amount), 0) AS "max!"
        FROM tips 
        WHERE currency_id = $1"#,
        currency_id.to_string()
    )
    .fetch_one(&mut *conn)
    .await?;

    Ok(record.max as u64)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_reactdrop(
    conn: &mut PgConnection,
    author: i64,
    emoji: String,
    amount: i64,
    channel_id: i64,
    message_id: i64,
    finish_time: DateTime<Utc>,
    currency_id: &Address,
) -> Result<(), Error> {
    sqlx::query!(
        "INSERT INTO reactdrops
        (author, channel_id, message_id, finish_time, emojistr, amount, status, currency_id)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7)
        ON CONFLICT (channel_id, message_id)
        DO NOTHING",
        author,
        channel_id,
        message_id,
        finish_time,
        emoji,
        amount,
        currency_id.to_string()
    )
    .execute(conn)
    .await?;

    Ok(())
}

/// Returns pending reactdrops, or an emtpy Vec if no pending reactdrops present
pub async fn get_pending_reactdrops(conn: &mut PgConnection) -> Result<Vec<Reactdrop>, Error> {
    let rows = sqlx::query!(
        "SELECT *
        FROM reactdrops
        WHERE status = 'pending'"
    )
    .fetch_all(conn)
    .await?;

    let vec: Vec<Reactdrop> = rows
        .into_iter()
        .map(|row| Reactdrop {
            status: crate::reactdrop::ReactdropState::Pending,
            author: (row.author as u64).into(),
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
    conn: &mut PgConnection,
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
    .execute(conn)
    .await?;

    Ok(())
}

pub async fn get_summed_deposits(conn: &mut PgConnection) -> Result<Amount, Error> {
    let row = sqlx::query!(
        r#"
        SELECT SUM(amount) as "amount!" 
        FROM transactions 
        WHERE transaction_action = 'deposit'
        "#
    )
    .fetch_one(conn)
    .await?;

    let amount = row.amount.to_u64().unwrap_or_default();

    Ok(Amount::from_sat(amount))
}

pub async fn get_summed_withdrawals(conn: &mut PgConnection) -> Result<Amount, Error> {
    let row = sqlx::query!(
        r#"
        SELECT SUM(amount) as "amount!" 
        FROM transactions 
        WHERE transaction_action = 'withdraw'
        "#
    )
    .fetch_one(conn)
    .await?;

    let amount = row.amount.to_u64().unwrap_or_default();

    Ok(Amount::from_sat(amount))
}
