pub mod commands;
pub mod config;
pub(crate) mod database;
pub mod reactdrop;
pub mod util;
pub mod wallet_listener;

use crate::{
    config::{Config, get_configuration},
    wallet_listener::TransactionProcessor,
};
use commands::*;
use poise::{
    CreateReply,
    serenity_prelude::{self as serenity, ChannelId, ClientBuilder, CreateMessage, UserId},
};
use secrecy::ExposeSecret;
use sqlx::PgPool;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::{pin, sync::RwLock};
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle, Toplevel};
use tracing::{Level, debug, error, info, instrument, warn};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, writer::MakeWriterExt},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};
use vrsc::{Address, Amount};
use vrsc_rpc::client::{Client as VerusClient, RpcApi};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

pub const VRSC_CURRENCY_ID: &str = "i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV";

#[tokio::main(worker_threads = 1)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    log_setup()?;

    let config = get_configuration()?;
    let database = PgPool::connect_lazy(&config.database.connection_string())?;
    // sqlx::migrate!("./migrations").run(&database).await?;

    let bot = Bot {
        client: app(config, database.clone()).await?,
        db: database,
    };

    Toplevel::new(async |s: &mut SubsystemHandle| {
        s.start(SubsystemBuilder::new("bot", bot.into_subsystem()));
    })
    .catch_signals()
    .handle_shutdown_requests(Duration::from_secs(30))
    .await
    .unwrap();

    Ok(())
}

struct Bot {
    client: serenity::Client,
    db: PgPool,
}

impl IntoSubsystem<Box<dyn std::error::Error + Send + Sync>> for Bot {
    async fn run(
        self,
        subsys: &mut SubsystemHandle,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = self.client;
        let http = client.http.clone();

        let reactdrop_service = reactdrop::Subsystem {
            http,
            pool: self.db.clone(),
        };

        subsys.start(SubsystemBuilder::new(
            "ReactdropService",
            reactdrop_service.into_subsystem(),
        ));

        pin!(client);

        while !subsys.is_shutdown_requested() {
            tokio::select! {
                res = client.start() => {
                    if let Err(e) = res {
                        error!("{e:?}");
                    }
                },
                _ = subsys.on_shutdown_requested() => {
                    break;
                }
            }
        }

        Ok(())
    }
}

#[instrument(err)]
async fn app(config: Config, database: PgPool) -> Result<serenity::Client, Error> {
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
            admin::batch_convert_transaction_amounts(),
            misc::help(),
            misc::info(),
            misc::source(),
            misc::register(),
            misc::notifications(),
            // chain::chart(),
            chain::vrscbtc(),
            chain::vrsceth(),
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
            wallet::donate_to_foundation(),
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
                let mut tx = ctx.data().database.begin().await.unwrap();
                database::insert_discord_user(&mut *tx, &ctx.author().id)
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

        return Err("Verus client not ready".into());
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

    let client = ClientBuilder::new(
        token.expose_secret(),
        serenity::GatewayIntents::non_privileged()
            | serenity::GatewayIntents::GUILD_MEMBERS
            | serenity::GatewayIntents::MESSAGE_CONTENT
            | serenity::GatewayIntents::GUILD_PRESENCES,
    )
    .framework(framework)
    .await?;

    Ok(client)
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
