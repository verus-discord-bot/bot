// group the several tipping commands here

use poise::serenity_prelude::{self, RoleId};
use tracing::*;
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::RpcApi;

use crate::{
    commands::wallet,
    util::database::{self, get_balance_for_user, store_new_address_for_user},
    Context, Error,
};

// check if the sending user has (enough) balance
// exclude tipper from role (if he exists)
// exclude role id from getting tipped
// exclude tipbot from getting tipped 1046736508297687040
// for every receiving user in the role:
// -v if the user does not have a db entry, create in both discord_users and balance_vrsc
// -v increase the balance
// - get notification settings
// - notify people in DM that want to be notified
#[poise::command(slash_command, category = "Tipping")]
async fn role(
    ctx: Context<'_>,
    role: serenity_prelude::Role,
    #[description = "The amount you want to tip"] tip_amount: f64,
) -> Result<(), Error> {
    let pool = &ctx.data().database;
    let tip_amount = Amount::from_vrsc(tip_amount)?;

    debug!("role: {:?}", role.id);
    if let Some(balance) = database::get_balance_for_user(&pool, &ctx.author().id).await? {
        trace!("tipper has balance");

        if wallet::balance_is_enough(
            &Amount::from_sat(balance),
            &tip_amount,
            &Amount::ZERO, // no fees for tipping
        ) {
            if let Some(guild) = ctx.guild() {
                debug!("guildid: {:?}", guild.id);
                let guild_members = guild.members.values();
                let role_members = guild_members
                    .filter(
                        |m| m.roles.contains(&role.id) || &role.id == &RoleId(guild.id.0), // @everyone role_id (same as guild_id) does never get tips
                    )
                    .map(|m| m.user.id.as_ref())
                    .collect::<Vec<_>>();

                debug!(
                    "tipping {} members of role {}",
                    role_members.len(),
                    role.name
                );

                // TODO optimize this query (select all that don't exist, insert them in 1 go)
                // check if all the tippees have an entry in the db
                for user_id in role_members.iter() {
                    if database::get_address_from_user(pool, user_id)
                        .await?
                        .is_none()
                    {
                        trace!("need to get new address");
                        let client = &ctx.data().verus;
                        let address = client.get_new_address()?;
                        store_new_address_for_user(pool, user_id, &address).await?;
                    }
                }

                // need to divide tipping amount over number of people in a role
                if let Some(div_tip_amount) = tip_amount.checked_div(role_members.len() as u64) {
                    debug!("after division every member gets {div_tip_amount}");
                    debug!("members: {:#?}", role_members);

                    database::tip_multiple_users(
                        pool,
                        &ctx.author().id,
                        role_members,
                        &div_tip_amount,
                    )
                    .await?;

                    // TODO: need to do notifications for users that have notification settings to ALL or DM-only.
                } else {
                    ctx.send(|reply| {
                        reply.ephemeral(false).content(format!(
                            "Could not send tip to role, maybe the amount is too low?"
                        ))
                    })
                    .await?;
                }
            }
        }
    }

    Ok(())
}

#[poise::command(slash_command, category = "Tipping")]
async fn user(
    ctx: Context<'_>,
    user: serenity_prelude::User,
    #[description = "The amount you want to tip"] tip_amount: f64,
) -> Result<(), Error> {
    let tip_amount = Amount::from_vrsc(tip_amount)?;

    debug!(
        "user {} ({}) wants to tip {} with {tip_amount}",
        ctx.author().name,
        ctx.author().id,
        user.id
    );

    // check if the tipper has enough balance
    // do a sanity check if the tippee really exists
    // update both balances in 1 go
    // send a non-ephemeral message saying "<from_user> just tipped <to_user> <amount> VRSC."

    let pool = &ctx.data().database;
    // let's first check if the tipper has enough balance:
    if let Some(balance) = database::get_balance_for_user(&pool, &ctx.author().id).await? {
        trace!("tipper has balance: {balance}");

        if wallet::balance_is_enough(
            &Amount::from_sat(balance),
            &tip_amount,
            &Amount::ZERO, // no fees for tipping
        ) {
            trace!("tipper has enough balance");
            // we can tip!
            // what if the user we are about to tip has no balance?
            // we need to create a balance for him first. TODO: Maybe we can do that in the command itself.
            if get_balance_for_user(pool, &user.id).await?.is_none() {
                trace!("balance is none, so need to create new balance for user.");
                let client = &ctx.data().verus;
                let address = client.get_new_address()?;
                store_new_address_for_user(pool, &user.id, &address).await?;
            }

            trace!("now we can tip the user.");

            database::tip_user(pool, &ctx.author().id, &user.id, &tip_amount).await?;

            // TODO: get notification settings
            ctx.send(|reply| {
                reply.ephemeral(false).content(format!(
                    "<@{}> just tipped <@{}> {tip_amount}!",
                    &ctx.author().id,
                    user.id
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

#[instrument(skip(_ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping", subcommands("role", "user"))]
pub async fn tip(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}
