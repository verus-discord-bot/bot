use poise::serenity_prelude::UserId;
use tracing::{debug, error, instrument, trace};
use vrsc::Amount;
use vrsc_rpc::RpcApi;

use crate::{util::database, Context, Error};

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

    let client = &ctx.data().verus;
    if let Ok(()) = client.rescan_from_height(height) {
        trace!("rescan done");
        ctx.send(|reply| reply.content("Rescan done")).await?;
    } else {
        trace!("rescan did not succeed")
    }

    Ok(())
}

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

#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn depositenabled(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("set deposits enabled to {value}");

    {
        let deposits_enabled = &ctx.data().deposits_enabled;
        let mut write = deposits_enabled.write().await;
        *write = value;
    }

    ctx.send(|reply| reply.content(format!("Deposits enabled: {value}")))
        .await?;

    Ok(())
}
