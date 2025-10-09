use poise::{
    CreateReply,
    serenity_prelude::{CreateEmbed, UserId},
};
use sqlx::{Postgres, Transaction};
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, instrument, trace};
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::{bitcoin::Txid, client::RpcApi};

use crate::{
    Context, Error, database,
    wallet_listener::{TransactionProcessor, process_txid},
};

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn adminhelp(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(CreateReply::default().ephemeral(true).content(
        r#"```
!status                         - (financial) status of the bot
!blacklist <user_id>            - blacklists a user (no more tipping, deposits & withdraws)
!rescanfromheight <blockheight> - rescan blockchain from given height
!checktxid <txid>               - manually check txid (in case user balance was not updated)
!withdrawenabled <true/false>   - enable / disable withdraws
!depositenabled <true/false>    - enable / disable deposits
!setwithdrawfee <sats>          - sets the fee a user is charged when withdrawing funds
!maintenance <true/false>       - set maintenance mode (commands are not executed) 
```"#,
    ))
    .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let mut conn = ctx.data().database.acquire().await?;

    let maintenance = *ctx.data().tx_processor.maintenance.read().await;
    let deposits_enabled = *ctx.data().tx_processor.deposits_enabled.read().await;
    let withdrawals_enabled = *ctx.data().withdrawals_enabled.read().await;
    let total_balance = Amount::from_sat(database::get_total_balance(&mut conn).await?);
    let total_tipped = Amount::from_sat(database::get_total_tipped(&mut conn).await?);
    let largest_tip = Amount::from_sat(database::get_largest_tip(&mut conn).await?);
    let total_deposited = totaldeposited(ctx).await?;
    let total_withdrawn = totalwithdrawn(ctx).await?;

    let client = ctx.data().verus()?;

    let daemon_balance = client.get_balance(None, None)?;

    debug!("total balance: {total_balance}");
    debug!("total_tipped: {total_tipped}");
    debug!("largest_tip: {largest_tip}");
    debug!("total_deposited: {total_deposited}");
    debug!("total_withdrawn: {total_withdrawn}");
    debug!("daemon_balance: {daemon_balance}");

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .title("Status report")
                .field("bot in maintenance", maintenance.to_string(), false)
                .field("deposits enabled", deposits_enabled.to_string(), false)
                .field(
                    "withdrawals enabled",
                    withdrawals_enabled.to_string(),
                    false,
                )
                .field("VRSC daemon balance", daemon_balance.to_string(), false)
                .field("Tipbot balance", total_balance.to_string(), false)
                .field("Total deposited", total_deposited.to_string(), false)
                .field("Total withdrawn", total_withdrawn.to_string(), false)
                .field(
                    "Database deposits - withdraws",
                    total_deposited
                        .checked_sub(total_withdrawn)
                        .unwrap_or(Amount::ZERO)
                        .to_string_in(vrsc::Denomination::Verus),
                    false,
                )
                .field("Total tipped", total_tipped.to_string(), false)
                .field("Largest tip", largest_tip.to_string(), false)
                .field(
                    "Bot fees _(minus network fees)_",
                    {
                        if let Some(pos_amount) = daemon_balance.checked_sub(total_balance) {
                            pos_amount
                        } else {
                            Amount::ZERO
                        }
                    }
                    .to_string_in(vrsc::Denomination::Verus),
                    false,
                ),
        ),
    )
    .await?;

    Ok(())
}

async fn totaldeposited(ctx: Context<'_>) -> Result<Amount, Error> {
    ctx.defer().await?;

    let mut conn = ctx.data().database.acquire().await?;
    let client = ctx.data().verus()?;

    let deposit_transactions = database::get_all_txids(&mut conn, "deposit").await?;

    let mut sum = Amount::ZERO;

    for txid in deposit_transactions {
        let raw_tx = client.get_raw_transaction_verbose(&txid)?;

        for vout in raw_tx.vout.iter() {
            if let Some(addresses) = &vout.script_pubkey.addresses {
                for address in addresses {
                    if let Some(user_id) =
                        database::get_user_from_address(&mut conn, address).await?
                    {
                        trace!("there is a user for this address: {user_id}",);
                        sum = sum.checked_add(vout.value_sat).unwrap();
                    }
                }
            } else {
                trace!("no addresses found in scriptpubkey");
            }
        }
    }

    Ok(sum)
}

pub async fn totalwithdrawn(ctx: Context<'_>) -> Result<Amount, Error> {
    ctx.defer().await?;

    let mut conn = ctx.data().database.acquire().await?;
    let client = ctx.data().verus()?;

    let withdraw_transactions = database::get_all_txids(&mut *conn, "withdraw").await?;

    let mut sum = Amount::ZERO;

    for txid in withdraw_transactions {
        let raw_tx = client.get_raw_transaction_verbose(&txid)?;
        let vout = raw_tx.vout.first().unwrap();

        sum = sum.checked_add(vout.value_sat).unwrap();
    }

    Ok(sum)
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn blacklist(ctx: Context<'_>, user_id: UserId) -> Result<(), Error> {
    debug!("no more fun for {user_id}");
    let mut tx = ctx.data().database.begin().await?;

    if let Some(status) = database::get_blacklist_status(&mut tx, user_id).await? {
        if status == true {
            database::set_blacklist_status(&mut tx, user_id, false).await?;
            if let Ok(mut blacklist) = ctx.data().blacklist.lock() {
                blacklist.remove(&user_id);
            }
            ctx.send(
                CreateReply::default().content(format!("user {user_id} removed from blacklist")),
            )
            .await?;
            trace!("{user_id} has been removed from blacklist");
        } else {
            database::set_blacklist_status(&mut tx, user_id, true).await?;
            if let Ok(mut blacklist) = ctx.data().blacklist.lock() {
                blacklist.insert(user_id);
            }

            ctx.send(CreateReply::default().content(format!("user {user_id} blacklisted")))
                .await?;

            trace!("{user_id} has been added to blacklist");
        }

        tx.commit().await?;
    } else {
        error!("user not in database");
    }

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn setwithdrawfee(ctx: Context<'_>, amount: u64) -> Result<(), Error> {
    let withdrawal_fee = &ctx.data().withdrawal_fee;

    debug!("fee before changing: {:?}", withdrawal_fee);

    let mut write = withdrawal_fee.write().await;
    *write = Amount::from_sat(amount);

    debug!("fee after changing: {:?}", withdrawal_fee);
    ctx.send(CreateReply::default().content(format!("Withdraw fee set to {} sats", amount)))
        .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn rescanfromheight(ctx: Context<'_>, height: u64) -> Result<(), Error> {
    trace!("Initiating a rescan from height {height}");

    let client = &ctx.data().verus()?;
    if let Ok(()) = client.rescan_from_height(height) {
        trace!("rescan done");

        tokio::time::sleep(Duration::from_secs(1)).await;
        ctx.data().tx_processor.process_long_queue().await?;
        ctx.data().tx_processor.process_short_queue().await?;
        ctx.send(CreateReply::default().content("Rescan done"))
            .await?;
    } else {
        trace!("rescan did not succeed")
    }

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn withdrawenabled(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("set withdraws enabled to {value}");

    {
        let withdrawals_enabled = &ctx.data().withdrawals_enabled;
        let mut write = withdrawals_enabled.write().await;
        *write = value;
    }

    ctx.send(CreateReply::default().content(format!("Withdraws enabled: {value}")))
        .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn depositenabled(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("set deposits enabled to {value}");

    {
        let mut tx = ctx.data().database.begin().await?;
        let deposits_enabled = &ctx.data().deposits_enabled;
        let mut write = deposits_enabled.write().await;
        if *write == true && value == false {
            trace!("need to process possible unprocessed transactions");

            let tx_proc = Arc::clone(&ctx.data().tx_processor);

            process_stored_txids(&mut tx, tx_proc).await?
        }
        tx.commit().await?;
        *write = value;
    }

    ctx.send(CreateReply::default().content(format!("Deposits enabled: {value}")))
        .await?;

    Ok(())
}

/// Manually checks a tx if it was not caught with rescan
#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn checktxid(ctx: Context<'_>, txid: Txid) -> Result<(), Error> {
    trace!("manually check {txid}");
    let http = ctx.serenity_context().http.clone();
    let mut tx = ctx.data().database.begin().await?;

    let client = &ctx.data().verus()?;

    if let Ok(raw_tx) = client.get_raw_transaction_verbose(&txid) {
        process_txid(http, &mut *tx, &raw_tx).await?;
        tx.commit().await?;
    }

    Ok(())
}

/// Manually add withdraw tx when one didn't register
///
/// Needs discord_user_id, txid, tx_fee (in sats)
#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn manuallyaddwithdraw(
    ctx: Context<'_>,
    user_id: UserId,
    txid: Txid,
    tx_fee: u64,
) -> Result<(), Error> {
    trace!("manually add withdraw: {txid}");
    let mut tx = ctx.data().database.begin().await?;
    let uuid = Uuid::new_v4();

    debug!("manually storing withdraw transaction: {uuid}: {user_id} - {txid} ({tx_fee})");

    database::store_withdraw_transaction(
        &mut *tx,
        &uuid,
        &user_id,
        Some(&txid),
        &format!("opid-{uuid}"),
        &Amount::from_sat(tx_fee),
    )
    .await?;

    tx.commit().await?;

    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .content(format!("{txid} stored.")),
    )
    .await?;

    Ok(())
}

/// Set maintenance mode on or off
#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn maintenance(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("setting maintenance mode to {value}");

    {
        let mut tx = ctx.data().database.begin().await?;
        let mut write = ctx.data().tx_processor.maintenance.write().await;
        if *write == true && value == false {
            trace!("need to process possible unprocessed transactions");

            let tx_proc = Arc::clone(&ctx.data().tx_processor);

            process_stored_txids(&mut tx, tx_proc).await?
        }
        tx.commit().await?;
        *write = value;
    }

    ctx.send(CreateReply::default().content(format!("Maintenance mode set to {value}")))
        .await?;

    Ok(())
}

async fn process_stored_txids(
    tx: &mut Transaction<'_, Postgres>,
    tx_proc: Arc<TransactionProcessor>,
) -> Result<(), Error> {
    let stored_txids = database::get_stored_txids(&mut *tx).await?;

    for txid in stored_txids {
        trace!("processing {txid}");
        // checks tx and puts them in a queue
        tx_proc.check_tx(txid).await?;

        // process the queue immediately
        tx_proc.process_long_queue().await?;
        tx_proc.process_short_queue().await?;

        database::set_stored_txid_to_processed(&mut *tx, &txid).await?;
    }

    Ok(())
}
