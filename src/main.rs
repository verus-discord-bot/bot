pub mod commands;
pub mod configuration;
pub mod util;
pub mod wallet_listener;

use std::{borrow::Borrow, collections::HashSet, sync::Arc};

use commands::*;
use vrsc::Amount;

use crate::{
    configuration::{get_configuration, Settings},
    wallet_listener::TransactionProcessor,
};

// use std::collections::hash_set::Iter<'_, UserId>
use opentelemetry::global;
use poise::serenity_prelude::{self as serenity, UserId};
use secrecy::ExposeSecret;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use vrsc_rpc::{Client as VerusClient, RpcApi};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug)]
pub struct Data {
    // maintenance: Arc<RwLock<bool>>,
    _verus: VerusClient,
    _bot_start_time: std::time::Instant,
    settings: Settings,
    bot_user_id: serenity::UserId,
    database: sqlx::PgPool,
    withdrawal_fee: Arc<RwLock<Amount>>,
    withdrawals_enabled: Arc<RwLock<bool>>,
    deposits_enabled: Arc<RwLock<bool>>,
    blacklist: std::sync::Mutex<HashSet<UserId>>,
    tx_processor: Arc<TransactionProcessor>,
    owners: HashSet<UserId>,
}

impl Data {
    pub fn verus(&self) -> Result<VerusClient, Error> {
        vrsc_rpc::Client::vrsc(
            self.settings.application.testnet,
            vrsc_rpc::Auth::UserPass(
                format!("http://127.0.0.1:{}", self.settings.application.rpc_port),
                self.settings.application.rpc_user.clone(),
                self.settings.application.rpc_password.clone(),
            ),
        )
        .map_err(|e| e.into())
    }
}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    warn!("Encountered error: {:?}", error);

    match error {
        poise::FrameworkError::Command { ctx, error } => {
            let owners = &ctx.data().owners;

            let s = owners.into_iter().map(|id| format!("<@{}>", id.0.to_string())).collect::<Vec<_>>().join(", ");

            if let Err(e) = ctx.send(|reply| reply.content(format!("{s}, {error}"))).await {
                warn!("{}", e)
            }
        }
        poise::FrameworkError::ArgumentParse { error: _, input, ctx } => {
            if let Err(e) = ctx
                .say(format!(
                    "The argument you provided ({}) was incorrect. Press arrow up \u{2191} to change the arguments and press Enter when you're done.",
                    input.unwrap()
                ))
                .await
            {
                warn!("{}", e)
            }
        }
        _ => {
            warn!("an unrecoverable error occured")
        }
    }
}

async fn app() -> Result<(), Error> {
    let config = get_configuration()?;
    let pg_url = &config.database.connection_string();
    let database = PgPool::connect_lazy(pg_url)?;
    sqlx::migrate!("./migrations").run(&database).await?;

    let owners = config
        .application
        .owners
        .iter()
        .map(|x| UserId(x.parse::<u64>().unwrap()))
        .collect::<HashSet<UserId>>()
        .clone();
    debug!("{owners:?}");
    let owners_clone = owners.clone();

    let options = poise::FrameworkOptions {
        commands: vec![
            admin::setwithdrawfee(),
            admin::rescanfromheight(),
            admin::feescollected(),
            admin::depositenabled(),
            admin::withdrawenabled(),
            admin::blacklist(),
            admin::checktxid(),
            admin::maintenance(),
            admin::status(),
            misc::help(),
            misc::source(),
            misc::register(),
            misc::notifications(),
            chain::info(),
            chain::peerinfo(),
            chain::price(),
            wallet::deposit(),
            wallet::balance(),
            wallet::withdraw(),
            tipping::tip(),
            tipping::reactdrop(),
        ],
        command_check: Some(|ctx| {
            let author = &ctx.author().id;
            let owners = &ctx.data().owners;
            // let owner = owners_clone;
            Box::pin(async move {
                let maintenance_mode = { *ctx.data().tx_processor.maintenance.read().await };

                if maintenance_mode && !owners.contains(&author) {
                    ctx.send(|reply| {
                        reply.content(
                            ":tools: The bot is in maintenance mode, we'll be right back :tools:",
                        ).ephemeral(true)
                    })
                    .await?;

                    return Ok(false);
                }

                Ok(true)
            })
        }),
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

                let pool = &ctx.data().database;
                if let Ok(response) =
                    crate::util::database::get_balance_for_user(pool.borrow(), &ctx.author().id)
                        .await
                {
                    if response.is_none() {
                        let client = &ctx.data().verus().unwrap();
                        let address = client.get_new_address().unwrap();
                        crate::util::database::store_new_address_for_user(
                            &pool,
                            &ctx.author().id,
                            &address,
                        )
                        .await
                        .expect("an address from the verus daemon");
                    }
                }

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
        owners,

        ..Default::default()
    };

    let client = vrsc_rpc::Client::vrsc(
        config.application.testnet,
        vrsc_rpc::Auth::UserPass(
            format!("http://127.0.0.1:{}", config.application.rpc_port),
            config.application.rpc_user.clone(),
            config.application.rpc_password.clone(),
        ),
    );

    if client.as_ref().is_err() || client.as_ref().unwrap().ping().is_err() {
        error!("Verus daemon not ready, abort");

        return Ok(());
    }

    // let client = client.unwrap();
    debug!("connection string: {}", config.database.connection_string());

    debug!("starting client");

    poise::Framework::builder()
        .token(config.application.discord.expose_secret())
        .setup(move |ctx, bot, _framework| {
            let http = ctx.http.clone();
            let pool = database.clone();
            let config_clone = config.clone();
            let deposits_enabled = Arc::new(RwLock::new(true));
            let deposits_enabled_clone = deposits_enabled.clone();

            Box::pin(async move {
                let tx_proc = Arc::new(TransactionProcessor::new(
                    http,
                    pool,
                    config_clone,
                    Arc::new(RwLock::new(false)),
                    deposits_enabled_clone,
                ));

                let tx_proc_clone = tx_proc.clone();
                tokio::spawn(async move {
                    tx_proc_clone.clone().listen_wallet_notifications().await;
                });

                let tx_proc_clone = tx_proc.clone();
                tokio::spawn(async move {
                    tx_proc_clone.clone().listen_block_notifications().await;
                });

                let withdrawal_fee =
                    Arc::new(RwLock::new(config.application.global_withdrawal_fee));

                Ok(Data {
                    // maintenance: Arc::new(RwLock::new(false)),
                    _verus: client.unwrap(),
                    _bot_start_time: std::time::Instant::now(),
                    settings: config,
                    bot_user_id: bot.user.id,
                    database,
                    withdrawal_fee,
                    withdrawals_enabled: Arc::new(RwLock::new(true)),
                    deposits_enabled,
                    blacklist: std::sync::Mutex::new(HashSet::new()),
                    tx_processor: tx_proc,
                    owners: owners_clone,
                })
            })
        })
        .options(options)
        .intents(
            serenity::GatewayIntents::non_privileged()
                | serenity::GatewayIntents::GUILD_MEMBERS
                | serenity::GatewayIntents::MESSAGE_CONTENT
                | serenity::GatewayIntents::GUILD_PRESENCES,
        )
        .run()
        .await?;

    Ok(())
}

#[tokio::main(worker_threads = 8)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // setup_logging().await?;
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var(
            "RUST_LOG",
            "verusbot=trace,vrsc-rpc=info,poise=info,serenity=info",
        )
    }

    global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

    // let tracer = opentelemetry_jaeger::new_agent_pipeline()
    //     .with_service_name("verusbot")
    //     .install_simple()?;

    // let opentelemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        // .with(opentelemetry)
        // Continue logging to stdout
        .with(fmt::Layer::default())
        .with(filter_layer)
        .try_init()?;

    // Start actual app:
    if let Err(e) = app().await {
        error!("{}", e);
        std::process::exit(1);
    }

    global::shutdown_tracer_provider(); // sending remaining spans

    Ok(())
}

// async fn setup_logging() -> Result<(), Report> {
//     if std::env::var("RUST_LIB_BACKTRACE").is_err() {
//         std::env::set_var("RUST_LIB_BACKTRACE", "1")
//     }
//     color_eyre::install()?;

// if std::env::var("RUST_LOG").is_err() {
//     std::env::set_var("RUST_LOG", "bot=trace")
// }

//     // let tracer = stdout::new_pipeline().install_simple();
//     // let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
//     // let subscriber = Registry::default().with(telemetry);
//     // let home_dir = std::env::var("HOME").unwrap();

//     // let file_appender =
//     //     tracing_appender::rolling::hourly(format!("{home_dir}/log/bot"), "tracing.log");
//     // tracing::subscriber::with_default(subscriber, || {
//     //     // Spans will be sent to the configured OpenTelemetry exporter
//     //     let root = span!(tracing::Level::TRACE, "app_start", work_units = 2);
//     //     let _enter = root.enter();

//     //     error!("This event will be logged in the root span.");
//     // });

//     tracing_subscriber::fmt()
//         .with_env_filter(EnvFilter::from_default_env())
//         //     // .with_writer(file_appender)
//         //     // .with_ansi(false) // uncomment to disable color
//         .init();

//     Ok(())
// }
