use std::time::Duration;

use poise::serenity_prelude::{self, CacheHttp, Emoji, RoleId, UserId};
use tracing::*;
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::RpcApi;

use crate::{
    commands::{misc::Notification, wallet},
    util::database::{self, get_balance_for_user, store_new_address_for_user},
    Context, Error,
};

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
            trace!("there is enough balance");

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
                    debug!("members: {:#?}", &role_members);

                    let tip_event_id = Uuid::new_v4();

                    database::tip_multiple_users(
                        pool,
                        &ctx.author().id,
                        &role_members,
                        &div_tip_amount,
                    )
                    .await?;

                    database::store_tip_transaction(
                        pool,
                        &tip_event_id,
                        &ctx.author().id,
                        "send",
                        &tip_amount,
                        role.id.0,
                    )
                    .await?;

                    database::store_multiple_tip_transactions(
                        pool,
                        &tip_event_id,
                        &role_members,
                        "recv",
                        &div_tip_amount,
                        &ctx.author().id,
                    )
                    .await?;

                    let notification_settings =
                        database::get_notification_setting_batch(pool, &role_members).await?;

                    for (user_id, notification) in notification_settings {
                        match (user_id, notification) {
                            (_, Notification::All) | (_, Notification::DMOnly) => {
                                let user = UserId(user_id as u64).to_user(ctx.http()).await?;
                                user.dm(ctx.http(), |message| {
                                    message.content(format!(
                                        "You just got tipped {div_tip_amount} from <@{}>!",
                                        &ctx.author().id,
                                    ))
                                })
                                .await?;
                            }
                            _ => {
                                // don't ping when ChannelOnly or Off
                            }
                        }
                    }

                    ctx.send(|reply| {
                        reply.ephemeral(false).content(format!(
                            "<@{}> just tipped {} to {} users!",
                            &ctx.author().id,
                            tip_amount,
                            &role_members.len()
                        ))
                    })
                    .await?;

                    return Ok(());
                } else {
                    ctx.send(|reply| {
                        reply.ephemeral(false).content(format!(
                            "Could not send tip to role, maybe the amount is too low?"
                        ))
                    })
                    .await?;

                    return Ok(());
                }
            }
        }
    }

    trace!("tipper has no balance or has not enough balance");

    ctx.send(|reply| {
        reply
            .ephemeral(false)
            .content(format!("Your balance is insufficient to tip that amount!"))
    })
    .await?;

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
    // update both balances in 1 go

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

            trace!("the tippee has a balance, we can tip now.");

            database::tip_user(pool, &ctx.author().id, &user.id, &tip_amount).await?;

            let tip_event_id = Uuid::new_v4();

            database::store_tip_transaction(
                pool,
                &tip_event_id,
                &ctx.author().id,
                "send",
                &tip_amount,
                user.id.0,
            )
            .await?;

            database::store_tip_transaction(
                pool,
                &tip_event_id,
                &user.id,
                "recv",
                &tip_amount,
                ctx.author().id.0,
            )
            .await?;

            // TODO: get notification settings
            let notification: Notification =
                database::get_notification_setting(&pool, &user.id).await?;

            match notification {
                Notification::All | Notification::ChannelOnly => {
                    // send a message in the same channel:
                    ctx.send(|reply| {
                        reply.ephemeral(false).content(format!(
                            "<@{}> just tipped <@{}> {tip_amount}!",
                            &ctx.author().id,
                            user.id
                        ))
                    })
                    .await?;
                }
                Notification::DMOnly => {
                    // send a non-pinging message in the channel:
                    ctx.send(|reply| {
                        reply.ephemeral(false).content(format!(
                            "<@{}> just tipped `{}` {tip_amount}!",
                            &ctx.author().id,
                            user.tag()
                        ))
                    })
                    .await?;
                    // send a notification in dm:
                    user.dm(&ctx.http(), |message| {
                        message.content(format!(
                            "You just got tipped {tip_amount} from <@{}> :party:",
                            &ctx.author().id,
                        ))
                    })
                    .await?;
                }
                Notification::Off => {
                    // send a non-pinging message in the channel:
                    ctx.send(|reply| {
                        reply.ephemeral(false).content(format!(
                            "<@{}> just tipped `{}` {tip_amount}!",
                            &ctx.author().id,
                            user.tag()
                        ))
                    })
                    .await?;
                }
            }

            return Ok(());
        }
    }

    trace!("tipper has no balance or has not enough balance");

    ctx.send(|reply| {
        reply
            .ephemeral(false)
            .content(format!("Your balance is insufficient to tip that amount!"))
    })
    .await?;

    Ok(())
}

#[instrument(skip(_ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping", subcommands("role", "user"))]
pub async fn tip(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping")]
pub async fn reactdrop(
    ctx: Context<'_>,
    emoji: Emoji,
    #[min = 0.5] amount: f64,
    #[max = 600]
    #[min = 10]
    time_in_secs: u32,
) -> Result<(), Error> {
    debug!("{:#?}", emoji);

    debug!("regex found");

    let reply_handle = ctx.say("hello").await?;
    let mut msg = reply_handle.into_message().await?;

    let context = ctx.serenity_context().to_owned();

    let http = context.http.clone();
    let http_2 = context.http.clone();

    let mut i: i32 = time_in_secs as i32;

    while i >= 0 {
        // this is a rough countdown as the time is not precisely 1 second every sleep event. This is what the Tokio docs say:
        // "`Sleep` operates at millisecond granularity and should not be used for tasks that require high-resolution timers."
        // But it's fine for our usecase :)
        tokio::time::sleep(Duration::from_secs(1)).await;
        msg.edit(http.clone(), |f| {
            f.content(format!("time left: {i} seconds"))
        })
        .await
        .unwrap();
        i -= 1;
    }

    let mut last_user = None;

    loop {
        if let Ok(users) = msg
            .reaction_users(http_2.clone(), emoji.clone(), Some(1), last_user)
            .await
        {
            last_user = users.last().map(|user| user.id);
            if last_user.is_none() {
                break;
            }
            debug!("user: {:#?}", &last_user);

            continue;
        } else {
            break;
        }
    }

    Ok(())
}
