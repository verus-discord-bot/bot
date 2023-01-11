use tracing::{debug, trace};
use vrsc::Amount;
use vrsc_rpc::RpcApi;

use crate::{util::database, Context, Error};

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
    let b = if let Some(balance) =
        database::get_balance_for_user(&pool, &ctx.data().bot_user_id).await?
    {
        debug!("got bot balance: {balance}");
        balance
    } else {
        debug!("no balance found for bot");
        0
    };

    ctx.send(|reply| reply.content(format!("Fees collected by bot: {}", Amount::from_sat(b))))
        .await?;

    Ok(())
}
