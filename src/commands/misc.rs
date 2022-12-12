use tracing::{instrument, trace};

use crate::{Context, Error};

/// Show this menu
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
            ephemeral: false,
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

/// Links to the bot GitHub repo
#[poise::command(discard_spare_arguments, slash_command, category = "Miscellaneous")]
pub async fn source(ctx: Context<'_>) -> Result<(), Error> {
    trace!("source called");
    ctx.say("https://github.com/verus-discord-bot/bot").await?;
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
