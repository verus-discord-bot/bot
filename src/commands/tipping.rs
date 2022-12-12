// group the several tipping commands here
use std::cmp::Ordering;

use poise::serenity_prelude;
use tracing::*;
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::{RpcApi, SendCurrencyOutput};

use crate::{util::database, Context, Error};
/// Withdraw funds from the tipbot wallet. You can use R*, i* or an existing identity (ends with @).
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping")]
pub async fn tip(
    ctx: Context<'_>,
    #[description = "The user you want to tip"] user: serenity_prelude::User,
    #[description = "The amount you want to tip"] tip_amount: f64,
) -> Result<(), Error> {
    debug!(
        "user {} ({}) wants to tip {} with {tip_amount}",
        ctx.author().name,
        ctx.author().id,
        user.name
    );

    let pool = &ctx.data().database;
    let balance = database::get_balance_for_user(&pool, ctx.author().id).await?;

    // check if the tipper has enough balance
    // do a sanity check if the tippee really exists
    // update both balances in 1 go
    // send a non-ephemeral message saying "<from_user> just tipped <to_user> <amount> VRSC."

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
