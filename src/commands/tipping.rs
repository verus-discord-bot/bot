// group the several tipping commands here
use std::{cmp::Ordering, time::Duration};

use poise::{
    serenity_prelude::{self, Mention, Mentionable},
    ChoiceParameter,
};
use serde::Deserialize;
use tracing::*;
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::{RpcApi, SendCurrencyOutput};

use crate::{
    commands::wallet,
    util::database::{self, get_balance_for_user, store_new_address_for_user},
    Context, Error,
};

/// Tip VRSC to another user.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping")]
pub async fn tip(
    ctx: Context<'_>,
    #[description = "The user you want to tip"] user: serenity_prelude::Member,
    #[description = "The amount you want to tip"] tip_amount: f64,
) -> Result<(), Error> {
    let tip_amount = Amount::from_vrsc(tip_amount)?;

    debug!(
        "user {} ({}) wants to tip {} with {tip_amount}",
        ctx.author().name,
        ctx.author().id,
        user.user.id
    );

    // check if the tipper has enough balance
    // do a sanity check if the tippee really exists
    // update both balances in 1 go
    // send a non-ephemeral message saying "<from_user> just tipped <to_user> <amount> VRSC."

    let pool = &ctx.data().database;
    // let's first check if the tipper has enough balance:
    if let Some(balance) = database::get_balance_for_user(&pool, &ctx.author().id).await? {
        trace!("tipper has balance");

        if wallet::balance_is_enough(
            &Amount::from_sat(balance),
            &tip_amount,
            &Amount::ZERO, // no fees for tipping
        ) {
            trace!("tipper has enough balance");
            // we can tip!
            tokio::time::sleep(Duration::from_millis(500)).await;
            // what if the user we are about to tip has no balance?
            // we need to create a balance for him first. TODO: Maybe we can do that in the command itself.
            if get_balance_for_user(pool, &user.user.id).await?.is_none() {
                trace!("balance is none, so need to create new balance for user.");
                let client = &ctx.data().verus;
                let address = client.get_new_address()?;
                store_new_address_for_user(pool, &user.user.id, &address).await?;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
            trace!("now we can tip the user.");

            database::tip_user(pool, &ctx.author().id, &user.user.id, &tip_amount).await?;

            ctx.send(|reply| {
                reply.ephemeral(false).content(format!(
                    "<@{}> just tipped <@{}> {tip_amount}!",
                    &ctx.author().id,
                    user.user.id
                ))
            })
            .await?;

            return Ok(());
        }
    }

    ctx.send(|reply| {
        reply
            .ephemeral(false)
            .content(format!("Your balance is insufficient to tip"))
    })
    .await?;

    Ok(())
}

// Sends a tip to a role, dividing the amount with the people in this role.

// #[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
// #[poise::command(slash_command, category = "Tipping")]
// pub async fn tip_role(
//     ctx: Context<'_>,
//     #[description = "The user you want to tip"] role: serenity_prelude::Role,
//     #[description = "The amount you want to tip"] tip_amount: f64,
// ) -> Result<(), Error> {
//     debug!(
//         "user {} ({}) wants to tip {} with {tip_amount}",
//         ctx.author().name,
//         ctx.author().id,
//         role.name
//     );

//     // check if the tipper has enough balance
//     //

//     Ok(())
// }
