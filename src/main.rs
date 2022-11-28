pub mod configuration;
pub mod events;
pub mod framework;

use std::sync::Arc;

use crate::{configuration::get_configuration, framework::*};

use color_eyre::Report;
use secrecy::ExposeSecret;
use serenity::{
    client::{Client, Context},
    framework::standard::{macros::hook, DispatchError, StandardFramework},
    model::{channel::Message, gateway::GatewayIntents},
};
use tracing::{debug, error};
use tracing_subscriber::EnvFilter;
use vrsc_rpc::RpcApi;

#[tokio::main(worker_threads = 8)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Hello, world!");

    let config = get_configuration().expect("failed to read config file");

    setup_logging().await?;

    let client = match config.application.testnet {
        true => vrsc_rpc::Client::vrsc(true, vrsc_rpc::Auth::ConfigFile),
        false => vrsc_rpc::Client::vrsc(false, vrsc_rpc::Auth::ConfigFile),
    }?;

    // do not start bot if Verus daemon isn't ready
    if let Err(e) = client.ping() {
        error!("Verus daemon not ready: {:?}", e);
        return Ok(());
    }

    debug!("connection string: {}", config.database.connection_string());

    let framework = StandardFramework::new()
        .configure(|c| c.prefix("!")) // set the bot's prefix to "!" if a prefix is used.
        .on_dispatch_error(on_dispatch_error)
        .group(&GENERAL_GROUP);

    let handler = Arc::new(events::Handler {});

    let mut intents = GatewayIntents::all();
    intents.remove(GatewayIntents::DIRECT_MESSAGE_TYPING);
    intents.remove(GatewayIntents::GUILD_MESSAGE_TYPING);

    let mut client = Client::builder(config.application.discord.expose_secret(), intents)
        .event_handler_arc(handler.clone())
        .framework(framework)
        .await
        .expect("Error creating serenity client");

    debug!("starting client");

    if let Err(why) = client.start().await {
        error!(
            "An error occurred while running the discord bot client: {:?}",
            why
        );
    }

    Ok(())
}

async fn setup_logging() -> Result<(), Report> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1")
    }
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "bot=debug,serenity=info")
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    Ok(())
}

#[hook]
pub async fn on_dispatch_error(
    ctx: &Context,
    msg: &Message,
    error: DispatchError,
    _command_name: &str,
) {
    match error {
        DispatchError::OnlyForDM => {
            if let Err(e) = msg
                .reply(ctx, "This can only be done in DM with this bot")
                .await
            {
                error!("something went wrong while sending a reply in DM: {:?}", e);
            }
        }
        _ => {
            error!("Unhandled dispatch error: {:?}", error);
        }
    }
}
