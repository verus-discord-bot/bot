pub mod commands;
pub mod config;
pub mod reactdrop;
pub mod util;
pub mod wallet_listener;

use crate::{
    config::{get_configuration, Config},
    util::database,
    wallet_listener::TransactionProcessor,
};
use commands::*;
use poise::{
    serenity_prelude::{self as serenity, ChannelId, ClientBuilder, CreateMessage, UserId},
    CreateReply,
};
use secrecy::ExposeSecret;
use sqlx::PgPool;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::RwLock, time::interval};
use tracing::{debug, error, info, instrument, warn, Level};
use tracing_subscriber::{
    fmt::{self, writer::MakeWriterExt},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};
use vrsc::{Address, Amount};
use vrsc_rpc::client::{Client as VerusClient, RpcApi};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[tokio::main(worker_threads = 1)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    log_setup()?;

    if let Err(e) = app().await {
        error!("{}", e);
        return Err(e);
    }

    Ok(())
}

#[instrument(err)]
async fn app() -> Result<(), Error> {
    let config = get_configuration()?;
    let pg_url = &config.database.connection_string();
    let database = PgPool::connect_lazy(pg_url)?;
    sqlx::migrate!("./migrations").run(&database).await?;

    let owners = config
        .application
        .owners
        .iter()
        .map(|x| UserId::new(x.parse::<u64>().unwrap()))
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
            chain::basket(),
            chain::ethbridge(),
            chain::varrrbridge(),
            chain::pure(),
            chain::halving(),
            // chain::time_of_block(),
            chain::currency(),
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
                    ctx.send(CreateReply::default().content(
                            ":tools: The bot is in maintenance mode, we'll be right back :tools:",
                        ).ephemeral(true)
                    )
                    .await?;

                    return Ok(false);
                }

                Ok(true)
            })
        }),
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some("!".into()),
            ..Default::default()
        },
        pre_command: |ctx| {
            Box::pin(async move {
                let pool = &ctx.data().database;
                database::insert_discord_user(pool, &ctx.author().id)
                    .await
                    .expect("a discord_user to be added to the database");

                let channel_name = ctx
                    .channel_id()
                    .name(&ctx.serenity_context())
                    .await
                    .unwrap_or_else(|_| "<unknown>".to_owned());

                tracing::info!(user = ?ctx.author().tag(), ?channel_name, invocation_string = ?ctx.invocation_string())
            })
        },
        on_error: |error| Box::pin(on_error(error)),
        owners,

        ..Default::default()
    };

    let client = vrsc_rpc::client::Client::vrsc(
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

    let client = client?;

    info!("starting client");

    let config_clone = config.clone();
    let token = config_clone.application.discord.clone();

    let framework = poise::Framework::builder()
        .setup(move |ctx, bot, _framework| {
            let http = ctx.http.clone();
            let pool = database.clone();
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

                tokio::spawn({
                    let config = config.clone();

                    async move {
                        let verus = vrsc_rpc::client::Client::vrsc(
                            config.application.testnet,
                            vrsc_rpc::Auth::UserPass(
                                format!("http://127.0.0.1:{}", config.application.rpc_port),
                                config.application.rpc_user.clone(),
                                config.application.rpc_password.clone(),
                            ),
                        )
                        .expect("verus client could not be created");

                        loop {
                            if let Err(e) = tx_proc_clone
                                .clone()
                                .listen_wallet_notifications(&verus)
                                .await
                            {
                                error!("listening for new tx failed: {e:?}");
                            };
                        }
                    }
                });

                let tx_proc_clone = tx_proc.clone();
                tokio::spawn(async move {
                    if let Err(e) = tx_proc_clone.clone().listen_block_notifications().await {
                        panic!("listening for new blocks failed: {e:?}");
                    }
                });

                info!("listening for daemon notifications");

                let withdrawal_fee =
                    Arc::new(RwLock::new(config.application.global_withdrawal_fee));

                Ok(Data {
                    // maintenance: Arc::new(RwLock::new(false)),
                    _verus: client,
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
                    currency_names: HashMap::new(),
                })
            })
        })
        .options(options)
        .build();

    let mut client = ClientBuilder::new(
        token.expose_secret(),
        serenity::GatewayIntents::non_privileged()
            | serenity::GatewayIntents::GUILD_MEMBERS
            | serenity::GatewayIntents::MESSAGE_CONTENT
            | serenity::GatewayIntents::GUILD_PRESENCES,
    )
    .framework(framework)
    .await?;

    client.start().await?;

    Ok(())
}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    info!("Encountered error: {:?}", error);

    match error {
        poise::FrameworkError::Command { ctx, error, .. } => {
            let owners = &ctx.data().owners;
            let s = owners
                .into_iter()
                .map(|id| format!("<@{}>", id.get().to_string()))
                .collect::<Vec<_>>()
                .join(", ");

            if let Err(e) = ChannelId::new(
                ctx.data()
                    .settings
                    .application
                    .discord_admin_thread_id
                    .parse::<u64>()
                    .unwrap(),
            )
            .send_message(
                ctx.http(),
                CreateMessage::new().content(format!(
                    "
                {s}, the following error occured:\n
                - error message: {error}\n
                - user that encounted error: {}\n
                - command used: {}\n
                - possible arguments used: {}",
                    ctx.author().name,
                    ctx.invoked_command_name(),
                    ctx.invocation_string()
                )),
            )
            .await
            {
                error!("{}", e)
            }
        }
        poise::FrameworkError::ArgumentParse {
            error: _,
            input,
            ctx,
            ..
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
    settings: Config,
    _bot_user_id: serenity::UserId,
    database: sqlx::PgPool,
    withdrawal_fee: Arc<RwLock<Amount>>,
    withdrawals_enabled: Arc<RwLock<bool>>,
    deposits_enabled: Arc<RwLock<bool>>,
    blacklist: std::sync::Mutex<HashSet<UserId>>,
    tx_processor: Arc<TransactionProcessor>,
    owners: HashSet<UserId>,
    currency_names: HashMap<Address, String>,
}

impl Data {
    pub fn verus(&self) -> Result<VerusClient, Error> {
        vrsc_rpc::client::Client::vrsc(
            self.settings.application.testnet,
            vrsc_rpc::Auth::UserPass(
                format!("http://127.0.0.1:{}", self.settings.application.rpc_port),
                self.settings.application.rpc_user.clone(),
                self.settings.application.rpc_password.clone(),
            ),
        )
        .map_err(|e| e.into())
    }

    pub fn to_currency_name(&self, address: &Address) -> Result<String, Error> {
        if let Some(name) = self.currency_names.get(address) {
            return Ok(name.to_owned());
        } else {
            let client = self.verus()?;

            let currency = client.get_currency(&address.to_string())?;
            let currency_name = currency.fullyqualifiedname;
            return Ok(currency_name);
        }
    }
}

fn log_setup() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        .with(filter_layer)
        .with(fmt::Layer::default().with_file(true).with_line_number(true))
        .with(
            fmt::Layer::new()
                .json()
                .with_ansi(false)
                .with_writer(file_appender.with_max_level(Level::ERROR)),
        )
        .try_init()?;

    Ok(())
}
