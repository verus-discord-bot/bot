pub mod configuration;

use std::sync::Arc;

use crate::configuration::get_configuration;

use color_eyre::Report;
use secrecy::ExposeSecret;
use serenity::{
    async_trait,
    client::{Client, Context},
    framework::standard::{
        macros::{group, hook},
        DispatchError, StandardFramework,
    },
    model::{channel::Message, gateway::GatewayIntents, prelude::Ready},
    prelude::EventHandler,
};
use sqlx::types::Uuid;
use tracing::{debug, error, info, instrument};
use tracing_subscriber::EnvFilter;
use vrsc_rpc::RpcApi;

// this allows for prefix commands to be grouped together
#[group]
pub struct General;

#[derive(Debug)]
pub struct Handler {}

#[async_trait]
impl EventHandler for Handler {
    #[instrument(skip(_ctx), fields(
        request_id = %Uuid::new_v4()
    ))]
    async fn ready(&self, _ctx: Context, _ready: Ready) {
        info!("Bot is ready!");
    }
}

#[tokio::main(worker_threads = 8)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Hello, world!");

    let config = get_configuration().expect("failed to read config file");

    setup_logging().await?;

    let client = match config.application.testnet {
        true => vrsc_rpc::Client::vrsc(true, vrsc_rpc::Auth::ConfigFile),
        false => vrsc_rpc::Client::vrsc(false, vrsc_rpc::Auth::ConfigFile),
    }?;

    if let Err(e) = client.ping() {
        error!("Verus daemon not ready: {:?}", e);
        return Ok(());
    }

    debug!("{}", config.database.connection_string());

    let framework = StandardFramework::new()
        .configure(|c| c.prefix("!")) // set the bot's prefix to "!"
        .on_dispatch_error(on_dispatch_error)
        .group(&GENERAL_GROUP);

    let handler = Arc::new(Handler {});

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
        std::env::set_var("RUST_LOG", "serenity=info,verusnft=debug")
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
