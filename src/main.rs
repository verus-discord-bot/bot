pub mod commands;
pub mod configuration;
pub mod reactdrop;
pub mod util;
pub mod wallet_listener;

use crate::{
    configuration::{get_configuration, Settings},
    util::database,
    wallet_listener::TransactionProcessor,
};
use commands::*;
// use opentelemetry::global;
use poise::serenity_prelude::{self as serenity, CacheHttp, ChannelId, UserId};
use secrecy::ExposeSecret;
use sqlx::PgPool;
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::{sync::RwLock, time::interval};
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::{
    fmt::{self, writer::MakeWriterExt},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};
use vrsc::Amount;
use vrsc_rpc::{Client as VerusClient, RpcApi};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

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
    debug!("owners: {owners:?}");
    let owners_clone = owners.clone();

    let options = poise::FrameworkOptions {
        commands: vec![
            admin::adminhelp(),
            admin::setwithdrawfee(),
            admin::rescanfromheight(),
            admin::depositenabled(),
            admin::withdrawenabled(),
            admin::blacklist(),
            admin::checktxid(),
            admin::maintenance(),
            admin::manuallyaddwithdraw(),
            admin::status(),
            misc::help(),
            misc::info(),
            misc::source(),
            misc::register(),
            misc::notifications(),
            chain::chaininfo(),
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
            prefix: Some("!".into()),
            edit_tracker: Some(poise::EditTracker::for_timespan(
                std::time::Duration::from_secs(60 * 60 * 24 * 2), // 48 hours
            )),
            ..Default::default()
        },
        pre_command: |ctx| {
            Box::pin(async move {
                let pool = &ctx.data().database;
                database::insert_discord_user(pool, &ctx.author().id)
                    .await
                    .expect("a discord_user to be added to the database");

                let author = ctx.author().tag();
                let channel_name = ctx
                    .channel_id()
                    .name(&ctx.serenity_context())
                    .await
                    .unwrap_or_else(|| "<unknown>".to_owned());
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

    debug!("connection string: {}", config.database.connection_string());
    info!("starting client");

    poise::Framework::builder()
        .token(config.application.discord.expose_secret())
        .setup(move |ctx, bot, _framework| {
            let http = ctx.http.clone();
            let pool = database.clone();
            let config_clone = config.clone();
            let deposits_enabled = Arc::new(RwLock::new(true));
            let deposits_enabled_clone = deposits_enabled.clone();

            Box::pin(async move {
                tokio::spawn({
                    let ctx = ctx.clone();
                    let pool = pool.clone();

                    info!("starting reactdrop loop");

                    async move {
                        let mut interval = interval(Duration::from_secs(20));

                        loop {
                            interval.tick().await;

                            if let Err(e) = reactdrop::check_running_reactdrops(&ctx, &pool).await {
                                error!("{:?}", e);
                            }
                        }
                    }
                });

                let tx_proc = Arc::new(TransactionProcessor::new(
                    http.clone(),
                    pool.clone(),
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

                info!("listening for daemon notifications");

                let withdrawal_fee =
                    Arc::new(RwLock::new(config.application.global_withdrawal_fee));

                Ok(Data {
                    // maintenance: Arc::new(RwLock::new(false)),
                    _verus: client.unwrap(),
                    _bot_start_time: std::time::Instant::now(),
                    settings: config,
                    _bot_user_id: bot.user.id,
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

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    info!("Encountered error: {:?}", error);

    match error {
        poise::FrameworkError::Command { ctx, error } => {
            let owners = &ctx.data().owners;
            let s = owners
                .into_iter()
                .map(|id| format!("<@{}>", id.0.to_string()))
                .collect::<Vec<_>>()
                .join(", ");

            if let Err(e) = ChannelId(
                ctx.data()
                    .settings
                    .application
                    .discord_admin_thread_id
                    .parse::<u64>()
                    .unwrap(),
            )
            .send_message(ctx.http(), |m| {
                m.content(format!(
                    "
                {s}, the following error occured:\n
                - error message: {error}\n
                - user that encounted error: {}\n
                - command used: {}\n
                - possible arguments used: {}",
                    ctx.author(),
                    ctx.invoked_command_name(),
                    ctx.invocation_string()
                ))
            })
            .await
            {
                error!("{}", e)
            }
        }
        poise::FrameworkError::ArgumentParse {
            error: _,
            input,
            ctx,
        } => {
            let s = format!(
                    "The argument you provided ({}) was incorrect. Press arrow up \u{2191} to change the arguments and press Enter when you're done.",
                     input.unwrap()
                );
            if let Err(e) = ctx.say(s).await {
                warn!("{}", e)
            }
        }
        _ => {
            error!("an unrecoverable error occured")
        }
    }
}

#[derive(Debug)]
pub struct Data {
    _verus: VerusClient,
    _bot_start_time: std::time::Instant,
    settings: Settings,
    _bot_user_id: serenity::UserId,
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

#[tokio::main(worker_threads = 8)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    log_setup()?;

    if let Err(e) = app().await {
        error!("{}", e);
        std::process::exit(1);
    }

    Ok(())
}

fn log_setup() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

    // let tracer = opentelemetry_jaeger::new_agent_pipeline()
    //     .with_service_name("verusbot")
    //     .install_simple()?;

    // let opentelemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var(
            "RUST_LOG",
            "verusbot=info,vrsc-rpc=info,poise=info,serenity=info",
        )
    }

    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    let file_appender = tracing_appender::rolling::hourly("./logs", "error");

    tracing_subscriber::registry()
        // .with(opentelemetry)
        // Continue logging to stdout
        .with(filter_layer)
        .with(fmt::Layer::default())
        .with(
            fmt::Layer::new()
                .json()
                .with_ansi(false)
                .with_writer(file_appender.with_max_level(Level::ERROR)),
        )
        .try_init()?;

    // global::shutdown_tracer_provider(); // sending remaining spans

    Ok(())
}
