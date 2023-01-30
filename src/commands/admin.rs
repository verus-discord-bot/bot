use std::sync::Arc;

use poise::serenity_prelude::UserId;
use sqlx::PgPool;
use tracing::{debug, error, instrument, trace};
use vrsc::Amount;
use vrsc_rpc::{bitcoin::Txid, RpcApi};

use crate::{
    util::database,
    wallet_listener::{process_txid, TransactionProcessor},
    Context, Error,
};

#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let pool = &ctx.data().database;

    let total_balance = Amount::from_sat(database::get_total_balance(pool).await?);
    let total_tipped = Amount::from_sat(database::get_total_tipped(pool).await?);
    let largest_tip = Amount::from_sat(database::get_largest_tip(pool).await?);

    let client = ctx.data().verus()?;

    let daemon_balance = client.get_balance(None, None)?;

    ctx.send(|reply| {
        reply.embed(|embed| {
            embed
                .title(":verus: Status report :verus:")
                .field("VRSC daemon balance", daemon_balance, false)
                .field("Tipbot balance", total_balance, false)
                .field("Total tipped", total_tipped, false)
                .field("Largest tip", largest_tip, false)
        })
    })
    .await?;
    // total balance held by all users
    // total amount tipped
    // total amount deposited
    // total amount withdrawn
    // largest tip
    // deposits: bool
    // withdrawals: bool
    // maintenance: bool

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn blacklist(ctx: Context<'_>, user_id: UserId) -> Result<(), Error> {
    debug!("no more fun for {user_id}");
    let pool = &ctx.data().database;

    if let Some(status) = database::get_blacklist_status(&pool, user_id).await? {
        if status == true {
            database::set_blacklist_status(&pool, user_id, false).await?;
            if let Ok(mut blacklist) = ctx.data().blacklist.lock() {
                blacklist.remove(&user_id);
            }
            ctx.send(|reply| reply.content(format!("user {user_id} removed from blacklist")))
                .await?;
            trace!("{user_id} has been removed from blacklist");
        } else {
            database::set_blacklist_status(&pool, user_id, true).await?;
            if let Ok(mut blacklist) = ctx.data().blacklist.lock() {
                blacklist.insert(user_id);
            }
            ctx.send(|reply| reply.content(format!("user {user_id} blacklisted")))
                .await?;

            trace!("{user_id} has been added to blacklist");
        }
    } else {
        error!("user not in database");
    }

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn setwithdrawfee(ctx: Context<'_>, amount: u64) -> Result<(), Error> {
    let withdrawal_fee = &ctx.data().withdrawal_fee;

    debug!("fee before changing: {:?}", withdrawal_fee);

    let mut write = withdrawal_fee.write().await;
    *write = Amount::from_sat(amount);

    debug!("fee after changing: {:?}", withdrawal_fee);
    ctx.send(|reply| reply.content(format!("Withdraw fee set to {} sats", amount)))
        .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn rescanfromheight(ctx: Context<'_>, height: u64) -> Result<(), Error> {
    trace!("Initiating a rescan from height {height}");

    let client = &ctx.data().verus()?;
    if let Ok(()) = client.rescan_from_height(height) {
        trace!("rescan done");
        ctx.send(|reply| reply.content("Rescan done")).await?;
    } else {
        trace!("rescan did not succeed")
    }

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn feescollected(ctx: Context<'_>) -> Result<(), Error> {
    trace!("fetching bot fees for {}", &ctx.data().bot_user_id);

    let pool = &ctx.data().database;
    let balance = database::get_bot_fees(pool).await?;

    ctx.send(|reply| {
        reply.content(format!(
            "Fees collected by bot: {}",
            Amount::from_sat(balance)
        ))
    })
    .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn withdrawenabled(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("set withdraws enabled to {value}");

    {
        let withdrawals_enabled = &ctx.data().withdrawals_enabled;
        let mut write = withdrawals_enabled.write().await;
        *write = value;
    }

    ctx.send(|reply| reply.content(format!("Withdraws enabled: {value}")))
        .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn depositenabled(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("set deposits enabled to {value}");

    {
        let deposits_enabled = &ctx.data().deposits_enabled;
        let mut write = deposits_enabled.write().await;
        if *write == true && value == false {
            trace!("need to process possible unprocessed transactions");

            let pool = &ctx.data().database;
            let tx_proc = Arc::clone(&ctx.data().tx_processor);

            process_stored_txids(pool, tx_proc).await?
        }
        *write = value;
    }

    ctx.send(|reply| reply.content(format!("Deposits enabled: {value}")))
        .await?;

    Ok(())
}

/// Manually checks a tx if it was not caught with rescan
#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn checktxid(ctx: Context<'_>, txid: Txid) -> Result<(), Error> {
    trace!("manually check {txid}");
    let http = ctx.serenity_context().http.clone();
    let pool = ctx.data().database.clone();

    let client = &ctx.data().verus()?;

    if let Ok(raw_tx) = client.get_raw_transaction_verbose(&txid) {
        process_txid(http, &pool, &raw_tx).await?;
    }

    Ok(())
}

/// Set maintenance mode on or off
#[instrument(skip(ctx))]
#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn maintenance(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("setting maintenance mode to {value}");

    {
        let mut write = ctx.data().tx_processor.maintenance.write().await;
        if *write == true && value == false {
            trace!("need to process possible unprocessed transactions");

            let pool = &ctx.data().database;
            let tx_proc = Arc::clone(&ctx.data().tx_processor);

            process_stored_txids(pool, tx_proc).await?
        }
        *write = value;
    }

    ctx.send(|reply| reply.content(format!("Maintenance mode set to {value}")))
        .await?;

    Ok(())
}

async fn process_stored_txids(
    pool: &PgPool,
    tx_proc: Arc<TransactionProcessor>,
) -> Result<(), Error> {
    let stored_txids = database::get_stored_txids(&pool).await?;

    for txid in stored_txids {
        trace!("processing {txid}");
        tx_proc.check_tx(txid).await?;

        database::set_stored_txid_to_processed(&pool, &txid).await?;
    }

    Ok(())
}
