use std::{fmt::Display, time::Instant};

use poise::{
    ChoiceParameter, CreateReply,
    serenity_prelude::{CreateEmbed, CreateEmbedFooter},
};
use tracing::instrument;
use uuid::Uuid;

use crate::{Context, Error, database};

/// Show information about this bot.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(track_edits, slash_command, category = "Miscellaneous")]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let elapsed = Instant::now().duration_since(ctx.data()._bot_start_time);
    ctx.send(
        CreateReply::default()
            .embed(
                CreateEmbed::new()
                    .title("Verus bot info")
                    .field("Version", "1.0a", false)
                    .field(
                        "Time since last start (h\\:m\\:s)",
                        format!(
                            "{h:0>2}:{m:0>2}:{s:0>2}",
                            h = (elapsed.as_secs() / 60) / 60,
                            m = (elapsed.as_secs() / 60) % 60,
                            s = elapsed.as_secs() % 60
                        ),
                        false,
                    )
                    .field(
                        "Source code",
                        "https://github.com/verus-discord-bot/bot",
                        false,
                    )
                    .footer(CreateEmbedFooter::new("Made for Verus by jorian@")),
            )
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Show help
#[poise::command(track_edits, slash_command, category = "Miscellaneous")]
#[instrument]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> Result<(), Error> {
    let extra_text_at_bottom = "\
Type `/help <command>` for more info on a command.";

    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration {
            extra_text_at_bottom,
            ephemeral: true,
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

/// Links to the bot GitHub repo
#[poise::command(discard_spare_arguments, slash_command, category = "Miscellaneous")]
pub async fn source(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        CreateReply::default()
            .content("https://github.com/verus-discord-bot/bot")
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Register slash commands in this guild or globally
///
/// Run with no arguments to register in guild, run with argument "global" to register globally.
#[poise::command(owners_only, prefix_command, hide_in_help, category = "Miscellaneous")]
pub async fn register(ctx: Context<'_>, #[flag] global: bool) -> Result<(), Error> {
    poise::builtins::register_application_commands(ctx, global).await?;

    Ok(())
}

/// Change notification settings
///
/// -------- :robot: **Notification settings** --------
///
/// - **All**: Get both notifications in DM when you get tipped as a role, \
/// and get tagged in channels where you get tipped directly.
/// - **DM Only**: Get a DM of every tip, even direct tips.
/// - **Channel only**: Do not get DM's about tips, only get notifications of \
/// direct tips in channels where you get tipped directly.
/// - **Off**: Do not get notifications of any kind.
#[instrument(skip(ctx, notifications), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(track_edits, slash_command, category = "Miscellaneous")]
pub async fn notifications(ctx: Context<'_>, notifications: Notification) -> Result<(), Error> {
    let mut conn = ctx.data().database.acquire().await?;
    database::update_notifications(&mut conn, &ctx.author().id, &notifications.to_string()).await?;

    ctx.send(CreateReply::default().ephemeral(true).content(format!(
        "You successfully set notifications to: {}",
        &notifications.to_string()
    )))
    .await?;

    Ok(())
}

#[derive(Debug, ChoiceParameter)]
pub enum Notification {
    #[name = "All"]
    All,
    #[name = "DM only"]
    DMOnly,
    #[name = "Channel only"]
    ChannelOnly,
    #[name = "Off"]
    Off,
}

impl From<String> for Notification {
    fn from(s: String) -> Self {
        match s.as_str() {
            "All" => Self::All,
            "DM only" => Self::DMOnly,
            "Channel only" => Self::ChannelOnly,
            "Off" => Self::Off,
            _ => Self::ChannelOnly, // This is the default setting.
        }
    }
}

impl Display for Notification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Notification::All => write!(f, "All"),
            Notification::DMOnly => write!(f, "DM only"),
            Notification::ChannelOnly => write!(f, "Channel only"),
            Notification::Off => write!(f, "Off"),
        }
    }
}
