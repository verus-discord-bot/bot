pub mod commands;
pub mod configuration;
pub mod util;
pub mod wallet_listener;

use commands::*;

use crate::{
    configuration::{get_configuration, Settings},
    wallet_listener::listen,
};

use color_eyre::Report;
use poise::serenity_prelude as serenity;
use secrecy::ExposeSecret;
use sqlx::PgPool;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;
use vrsc_rpc::{Client as VerusClient, RpcApi};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug)]
pub struct Data {
    verus: VerusClient,
    settings: Settings,
    _bot_user_id: serenity::UserId,
    //    mod_role_id: serenity::RoleId,
    _bot_start_time: std::time::Instant,
    database: sqlx::PgPool,
}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    warn!("Encountered error: {:?}", error);
    if let poise::FrameworkError::Command { ctx, error } = error {
        if let Err(e) = ctx.say(error.to_string()).await {
            warn!("{}", e)
        }
    }
}

async fn app() -> Result<(), Error> {
    let options = poise::FrameworkOptions {
        commands: vec![
            misc::help(),
            misc::source(),
            misc::register(),
            chain::info(),
            wallet::deposit(),
            wallet::balance(),
            wallet::withdraw(),
        ],
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some("?".into()),
            edit_tracker: Some(poise::EditTracker::for_timespan(
                std::time::Duration::from_secs(60 * 60 * 24 * 2), // 2 days
            )),
            ..Default::default()
        },
        pre_command: |ctx| {
            Box::pin(async move {
                let channel_name = ctx
                    .channel_id()
                    .name(&ctx.serenity_context())
                    .await
                    .unwrap_or_else(|| "<unknown>".to_owned());
                let author = ctx.author().tag();

                match ctx {
                    poise::Context::Prefix(ctx) => {
                        info!("{} in {}: `{}`", author, channel_name, &ctx.msg.content);
                    }
                    poise::Context::Application(ctx) => {
                        let command_name = &ctx.interaction.data().name;

                        info!("{} in {}: `/{}`", author, channel_name, command_name);
                    }
                }
            })
        },
        on_error: |error| Box::pin(on_error(error)),
        // event_handler: |ctx, event, _framework, data| Box::pin(listener(ctx, event, data)),
        ..Default::default()
    };

    let config = get_configuration()?;

    let client = vrsc_rpc::Client::vrsc(
        config.application.testnet,
        vrsc_rpc::Auth::UserPass(
            format!("http://127.0.0.1:{}", config.application.rpc_port),
            config.application.rpc_user.clone(),
            config.application.rpc_password.clone(),
        ),
    )?;

    // do not start bot if Verus daemon isn't ready
    if let Err(e) = client.ping() {
        error!("Verus daemon not ready: {:?}", e);
        return Ok(());
    }

    debug!("connection string: {}", config.database.connection_string());

    let pg_url = &config.database.connection_string();
    let database = PgPool::connect_lazy(pg_url)?;

    debug!("starting client");

    poise::Framework::builder()
        .token(config.application.discord.expose_secret())
        .setup(move |ctx, bot, _framework| {
            let http = ctx.http.clone();
            let db = database.clone();

            debug!("really starting");

            Box::pin(async move {
                tokio::spawn(async { listen(http, db).await });

                Ok(Data {
                    verus: client,
                    settings: config,
                    _bot_user_id: bot.user.id,
                    _bot_start_time: std::time::Instant::now(),
                    database,
                })
            })
        })
        .options(options)
        .intents(
            serenity::GatewayIntents::non_privileged()
                | serenity::GatewayIntents::GUILD_MEMBERS
                | serenity::GatewayIntents::MESSAGE_CONTENT,
        )
        .run()
        .await?;

    Ok(())
}

#[tokio::main(worker_threads = 8)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    setup_logging().await?;

    if let Err(e) = app().await {
        error!("{}", e);
        std::process::exit(1);
    }

    Ok(())
}

async fn setup_logging() -> Result<(), Report> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1")
    }
    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "bot=trace")
    }

    // let home_dir = std::env::var("HOME").unwrap();

    // let file_appender =
    //     tracing_appender::rolling::hourly(format!("{home_dir}/log/bot"), "tracing.log");

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        // .with_writer(file_appender)
        // .with_ansi(false) // uncomment to disable color
        .init();

    Ok(())
}
