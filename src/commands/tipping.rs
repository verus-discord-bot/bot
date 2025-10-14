use std::str::FromStr;

use ::chrono::Duration;
use poise::{
    CreateReply,
    serenity_prelude::{self, CacheHttp, ChannelId, CreateMessage, ReactionType, RoleId, UserId},
};

use sqlx::{Postgres, Transaction, types::chrono};
use tracing::*;
use uuid::Uuid;
use vrsc::{Address, Amount};

use crate::{
    Context, Error, VRSC_CURRENCY_ID,
    commands::{misc::Notification, user_blacklisted},
    database,
    wallet::get_and_check_balance,
};

/// Tip a user or a role
///
/// -------- :robot: **Tipping a user** --------
/// Tip a role by entering and selecting the user name. The selection menu will update as you type.
///
/// -------- :robot: **Tipping a role** --------
/// Tip a role by entering and selecting the role name. The role name can be any role, \
/// even the @everyone role.
/// The amount entered in the second parameter will be split evenly among the members of the role.
#[instrument(skip(_ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping", subcommands("role", "user"))]
pub async fn tip(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Tip a role by entering and selecting the role name.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping")]
async fn role(
    ctx: Context<'_>,
    #[description = "Enter and select the role you want to tip"] role: serenity_prelude::Role,
    #[description = "The amount you want to tip"]
    #[min = 0.5]
    tip_amount: f64,
) -> Result<(), Error> {
    let mut tx = ctx.data().database.begin().await?;

    if user_blacklisted(ctx, ctx.author().id).await? {
        return Ok(());
    }

    debug!("role: {:?}", role.id);
    let tip_amount = Amount::from_vrsc(tip_amount)?;

    if get_and_check_balance(&ctx, tip_amount, Amount::ZERO)
        .await?
        .is_some()
    {
        trace!("tipper has enough balance");
        let guild_id = ctx.guild_id();

        if let Some(members) = ctx.guild().map(|guild| guild.members.clone()) {
            let guild_members = members.values();
            let role_members = guild_members
                .filter(
                    // @everyone role_id (same as guild_id) does never get tips
                    |m| {
                        m.roles.contains(&role.id)
                            || role.id == RoleId::new(guild_id.unwrap().get())
                    },
                )
                .map(|m| m.user.id)
                .collect::<Vec<_>>();

            tip_multiple_users(
                &mut tx,
                ctx.author().id,
                ctx.http(),
                &ctx.channel_id(),
                role_members,
                &tip_amount,
                "role",
            )
            .await?;

            tx.commit().await?;

            return Ok(());
        } else {
            trace!("not in a guild, send error");

            return Ok(());
        }
    }

    Ok(())
}

/// Tip a user by entering and selecting the user's name.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping")]
async fn user(
    ctx: Context<'_>,
    #[description = "Enter and select the user you want to tip"] user: serenity_prelude::User,
    #[description = "The amount you want to tip"] tip_amount: f64,
) -> Result<(), Error> {
    if user_blacklisted(ctx, ctx.author().id).await? {
        return Ok(());
    }

    let tip_amount = Amount::from_vrsc(tip_amount)?;

    debug!(
        "user {} ({}) wants to tip {} with {tip_amount}",
        ctx.author().name,
        ctx.author().id,
        user.id
    );

    // check if the tipper has enough balance
    // update both balances in 1 go

    if get_and_check_balance(&ctx, tip_amount, Amount::ZERO)
        .await?
        .is_some()
    {
        trace!("tipper has enough balance");

        let mut tx = ctx.data().database.begin().await?;
        database::process_a_tip(
            &mut tx,
            ctx.author().id,
            &[user.id],
            tip_amount,
            &Address::from_str(VRSC_CURRENCY_ID)?,
        )
        .await?;

        // tips are only stored one way: counterparty is the sender of the tip.
        let tip_event_id = Uuid::new_v4();
        database::store_tip_transactions(
            &mut tx,
            &tip_event_id,
            &vec![user.id],
            "direct",
            tip_amount,
            ctx.author().id,
            &Address::from_str(VRSC_CURRENCY_ID)?,
        )
        .await?;
        tx.commit().await?;

        let mut conn = ctx.data().database.acquire().await?;
        match database::get_loudness_setting(&mut conn, user.id).await? {
            Some(notification) => {
                match notification {
                    Notification::All | Notification::ChannelOnly => {
                        // send a message in the same channel:
                        ctx.send(CreateReply::default().ephemeral(false).content(format!(
                            "<@{}> just tipped <@{}> {tip_amount}!",
                            &ctx.author().id,
                            user.id
                        )))
                        .await?;
                    }
                    Notification::DMOnly => {
                        // send a non-pinging message in the channel:
                        ctx.send(CreateReply::default().ephemeral(false).content(format!(
                            "<@{}> just tipped `{}` {tip_amount}!",
                            &ctx.author().id,
                            user.tag()
                        )))
                        .await?;
                        // send a notification in dm:
                        user.dm(
                            &ctx.http(),
                            CreateMessage::new().content(format!(
                                "You just got tipped {tip_amount} from <@{}>!",
                                &ctx.author().id,
                            )),
                        )
                        .await?;
                    }
                    Notification::Off => {
                        // send a non-pinging message in the channel:
                        ctx.send(CreateReply::default().ephemeral(false).content(format!(
                            "<@{}> just tipped `{}` {tip_amount}!",
                            &ctx.author().id,
                            user.tag()
                        )))
                        .await?;
                    }
                }
            }
            None => {
                trace!("User has not set notification settings, defaulting to Channel");

                ctx.send(CreateReply::default().ephemeral(false).content(format!(
                    "<@{}> just tipped <@{}> {tip_amount}!",
                    &ctx.author().id,
                    user.id
                )))
                .await?;
            }
        }

        return Ok(());
    }

    Ok(())
}

#[derive(Debug, poise::ChoiceParameter)]
pub enum Hms {
    Hours,
    Minutes,
}

/// Start a giveaway where users need to react to a message to participate
///
/// -------- :robot: **Reactdrop** --------
/// When initiating a reactdrop, find a suitable emoji in the first parameter. \
/// It can be any Emoji, as long as the emoji is in the current server.
///
/// The amount is entered in the second parameter. This amount will be split \
/// among the participants of the reactdrop when it ends.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Tipping")]
pub async fn reactdrop(
    ctx: Context<'_>,
    #[description = "The emoji users need to react with"] emoji: String,
    #[min = 0.1]
    #[description = "The amount you want to give away"]
    amount: f64,
    #[min = 1] time: i64,
    #[description = "The time in hours, minutes or seconds"] hms: Hms,
) -> Result<(), Error> {
    if user_blacklisted(ctx, ctx.author().id).await? {
        return Ok(());
    }

    let tip_amount = Amount::from_vrsc(amount)?;

    if get_and_check_balance(&ctx, tip_amount, Amount::ZERO)
        .await?
        .is_some()
    {
        debug!("emoji picked for reactdrop: {}", emoji);

        if let Ok(reaction_type) = ReactionType::try_from(emoji) {
            match &reaction_type {
                ReactionType::Custom { id, .. } => {
                    let emojis = ctx.guild_id().unwrap().emojis(&ctx.http()).await?;
                    if !emojis.iter().any(|e| e.id == id.get()) {
                        trace!("emoji not in guild");
                        ctx.send(CreateReply::default().ephemeral(true).content(
                            "This emoji is not found in this Discord server, so it can't be \
                                    used. Please pick another one",
                        ))
                        .await?;

                        return Ok(());
                    } else {
                        debug!("emoji in guild");
                    }
                }
                ReactionType::Unicode(unicode) => {
                    let emoji = emojis::get(unicode);

                    if emoji.is_none() {
                        ctx.send(CreateReply::default().ephemeral(true).content(
                            "This is not a valid emoji. \
                                    Please pick an emoji to start a Reactdrop",
                        ))
                        .await?;

                        return Ok(());
                    } else {
                        trace!("valid unicode");
                    }
                }
                ref s => {
                    unreachable!("we find ourselves in a weird state: {:?}", s);
                }
            }

            trace!("valid emoji");

            let time_in_seconds: Duration = match hms {
                Hms::Hours => Duration::seconds(time * 60 * 60),
                Hms::Minutes => Duration::seconds(time * 60),
            };

            let now = chrono::Utc::now();
            // sane values are guaranteed by command argument limits
            let finish_time = now.checked_add_signed(time_in_seconds).unwrap();
            debug!("finish_time: {finish_time:?}");

            let reply_handle = ctx
                .say(format!(
                    ">>> **A reactdrop of {tip_amount} was started!**\n\n\
    React with the {} emoji to participate\n\n
    Time remaining: {} hour(s) and {} minute(s)",
                    reaction_type.clone(),
                    time_in_seconds.num_seconds() / (60 * 60),
                    (time_in_seconds.num_seconds() / 60) % 60
                ))
                .await?;
            let msg = reply_handle.into_message().await?;
            msg.react(ctx.http(), reaction_type.clone()).await?;

            // a reactdrop can be started for as long as a user wants it to last.
            // Discord however limits the lifetime of a context to 15 minutes.
            // We must account for this by extracting the necessary data from `Context`
            // and store it for later use.
            let channel_id = ctx.channel_id();
            let message_id = msg.id;

            let mut conn = ctx.data().database.acquire().await?;

            database::insert_reactdrop(
                &mut conn,
                ctx.author().id.into(),
                reaction_type.to_string(),
                Amount::from_vrsc(amount).unwrap().as_sat() as i64,
                channel_id.into(),
                message_id.into(),
                finish_time,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;
        }
    }

    Ok(())
}

// Divides the amount over the `users` vec, increases the balance for all `users`
// and stores the tip transaction
// This function gets called in `tip role` and `reactdrop`
// We need the ChannelId here because ReactDrops tend to last longer than 15 minutes,
// which is the time Discord drops the context, giving
// us an invalid webhook token when trying to send a message using that context.
pub async fn tip_multiple_users(
    tx: &mut Transaction<'_, Postgres>,
    author: UserId,
    http: impl CacheHttp,
    channel_id: &ChannelId,
    users: Vec<UserId>,
    amount: &Amount,
    kind: &str,
) -> Result<(), Error> {
    // TODO optimize this query (select all that don't exist, insert them in 1 go)
    // check if all the tippees have an entry in the db
    // let pool = &ctx.data().database;
    // let author = ctx.author().id;
    // let http = ctx.http();

    debug!("users in tip_users: {:?}", users);

    // need to divide tipping amount over number of users
    // calculation is done using integer division, any remainder is lost,
    // so we effectively round down the tip amounts.
    if let Some(div_tip_amount) = amount.checked_div(users.len() as u64) {
        // we sum it all together again to get a (potentially lower) total amount
        // to tip
        let amount = div_tip_amount
            .checked_mul(users.len() as u64)
            .unwrap_or(*amount);
        debug!("after division every member gets {div_tip_amount}");
        debug!("members: {:#?}", &users);

        let tip_event_id = Uuid::new_v4();

        database::process_a_tip(
            &mut *tx,
            author,
            &users,
            div_tip_amount,
            &Address::from_str(VRSC_CURRENCY_ID)?,
        )
        .await?;

        database::store_tip_transactions(
            tx,
            &tip_event_id,
            &users,
            kind,
            div_tip_amount,
            author,
            &Address::from_str(VRSC_CURRENCY_ID)?,
        )
        .await?;

        for user_id in &users {
            if let Some(Notification::All | Notification::DMOnly) =
                database::get_loudness_setting(tx, *user_id).await?
            {
                user_id
                    .to_user(&http)
                    .await?
                    .dm(
                        &http,
                        CreateMessage::new().content(format!(
                            "You just got tipped {div_tip_amount} from <@{}>!",
                            &author,
                        )),
                    )
                    .await?;
            }
        }

        channel_id
            .send_message(
                http,
                CreateMessage::new().content(format!(
                    "<@{}> just tipped {} to {} users!",
                    &author,
                    amount,
                    &users.len()
                )),
            )
            .await?;
    } else {
        error!("could not send tip to role");
    }

    Ok(())
}
