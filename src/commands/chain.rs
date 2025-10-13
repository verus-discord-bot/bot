use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::Hasher,
};

use charming::{
    ImageRenderer,
    component::{Axis, Grid},
    element::{AxisLabel, AxisTick, JsFunction},
};
use chrono::{DateTime, Datelike, Duration, Utc};
use poise::{
    CreateReply,
    serenity_prelude::{Colour, CreateAttachment, CreateEmbed, CreateEmbedFooter},
};
use serde::Deserialize;
use tracing::{debug, instrument};
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::{
    client::{Client, RpcApi},
    json::GetCurrencyStateResult,
};

use crate::{Context, Error};

/// The price of VRSC / tBTC.vETH in the NATI:owl: basket
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn vrscbtc(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let verus_client = ctx.data().verus()?;

    let filename = format!("chart-{}.png", chrono::Utc::now().timestamp_micros());

    let conversion_data = get_conversion_data(
        &verus_client,
        "iH37kRsdfoHtHK5TottP1Yfq8hBSHz9btw",
        "VRSC",
        "tBTC.vETH",
        720,
        "VRSC",
    )?;

    let img_bytes = get_charming_chart_bytes(conversion_data)?;
    let attachment = CreateAttachment::bytes(img_bytes, &filename);

    ctx.send(
        CreateReply::default()
            .embed(CreateEmbed::new().image(format!("attachment://{filename}")))
            .attachment(attachment),
    )
    .await?;

    Ok(())
}

/// The price of VRSC / vETH in the NATI:owl: basket
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn vrsceth(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let verus_client = ctx.data().verus()?;

    let filename = format!("chart-{}.png", chrono::Utc::now().timestamp_micros());
    let conversion_data = get_conversion_data(
        &verus_client,
        "iH37kRsdfoHtHK5TottP1Yfq8hBSHz9btw",
        "VRSC",
        "vETH",
        720,
        "VRSC",
    )?;

    let img_bytes = get_charming_chart_bytes(conversion_data)?;
    let attachment = CreateAttachment::bytes(img_bytes, &filename);

    ctx.send(
        CreateReply::default()
            .embed(CreateEmbed::new().image(format!("attachment://{filename}")))
            .attachment(attachment),
    )
    .await?;

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct Candle {
    blocktime: u64,
    open: f32,
    high: f32,
    low: f32,
    close: f32,
}

fn get_charming_chart_bytes(data: Vec<Candle>) -> Result<Vec<u8>, Error> {
    // charting lib needs it as 'close, open, low, high'
    let ohlc = data
        .iter()
        .map(|candle| vec![candle.close, candle.open, candle.low, candle.high])
        .collect::<Vec<_>>();

    let lowest = ohlc
        .iter()
        .flatten()
        .map(|p| *p)
        .reduce(f32::min)
        .unwrap_or_default();

    let chart = charming::Chart::new()
        .grid(Grid::new().left("90px").contain_label(true))
        .x_axis(
            Axis::new()
                .data(
                    data.iter()
                        .map(|candle| {
                            let dt = DateTime::from_timestamp(candle.blocktime as i64, 0).unwrap();

                            format!("{}-{}", dt.day(), dt.month())
                        })
                        .collect::<Vec<_>>(),
                )
                .axis_label(AxisLabel::new().font_size(20))
                .axis_tick(AxisTick::new().split_number(15.0)),
        )
        .y_axis(
            Axis::new()
                .start_value(lowest)
                .axis_label(
                    AxisLabel::new()
                        .font_size(20)
                        .formatter(JsFunction::new_with_args(
                            "value",
                            "return Number(value).toFixed(8);",
                        )),
                ),
        )
        .series(charming::series::Candlestick::new().data(ohlc));

    let bytes = ImageRenderer::new(1000, 600)
        .theme(charming::theme::Theme::Dark)
        .render_format(charming::ImageFormat::Png, &chart)?;

    Ok(bytes)
}

fn get_conversion_data(
    client: &Client,
    currency_name: &str,
    base: &str,
    rel: &str,
    step: u64,
    denominated_currency: &str,
) -> Result<Vec<Candle>, Error> {
    let height = client.get_blockchain_info().unwrap().blocks;

    let period = format!("{},{},{step}", height - (step * 50), height);

    let currency_state =
        client.get_currency_state(currency_name, Some(&period), Some(denominated_currency))?;

    let mut res = vec![];
    let mut previous_row = None;

    for cs in currency_state.into_iter() {
        if let GetCurrencyStateResult::Data(ref data) = cs {
            let blocktime = client.get_block_by_height(data.height, 2)?.time;
            if let Some(cd) = data.conversiondata.clone() {
                if let Some(new_row) = cd.volumepairs.into_iter().find_map(|vp| {
                    if vp.currency == rel && vp.convertto == base {
                        return Some(Candle {
                            blocktime,
                            open: vp.open.as_vrsc() as f32,
                            high: vp.high.as_vrsc() as f32,
                            low: vp.low.as_vrsc() as f32,
                            close: vp.close.as_vrsc() as f32,
                        });
                    }

                    None
                }) {
                    res.push(new_row.clone());
                    previous_row = Some(new_row);
                } else {
                    if let Some(row) = previous_row {
                        res.push(row);
                    }
                }
            }
        }
    }

    Ok(res)
}

/// Show information about Verus blockchain.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(track_edits, slash_command, category = "Miscellaneous")]
pub async fn chaininfo(ctx: Context<'_>) -> Result<(), Error> {
    let client = ctx.data().verus()?;
    let blockchain_info = client.get_blockchain_info()?;
    let mining_info = client.get_mining_info()?;

    let testnet_name = match ctx.data().settings.application.testnet {
        true => "Verus (testnet)",
        false => "Verus",
    };

    ctx.send(
        CreateReply::default()
            .embed(
                CreateEmbed::new()
                    .title(format!("{} info", testnet_name))
                    .field("height", blockchain_info.blocks.to_string(), false)
                    .field("difficulty", blockchain_info.difficulty.to_string(), false)
                    .field(
                        "amount staking",
                        Amount::from_vrsc(mining_info.stakingsupply)
                            .unwrap()
                            .to_string_in(vrsc::Denomination::Verus),
                        false,
                    )
                    .field(
                        "average block fees",
                        Amount::from_vrsc(mining_info.averageblockfees)
                            .unwrap()
                            .to_string_in(vrsc::Denomination::Verus),
                        false,
                    ),
            )
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Shows the ip addresses of all the peers that are connected to the bot.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn peerinfo(ctx: Context<'_>) -> Result<(), Error> {
    let client = &ctx.data().verus()?;

    let peer_info = client
        .get_peer_info()?
        .into_iter()
        .filter(|peer| peer.inbound == false)
        .collect::<Vec<_>>();

    ctx.send(CreateReply::default().ephemeral(true).content(format!(
            "Publicly available peers:```{}```",
            peer_info
                .into_iter()
                .map(|peer| peer.addr)
                .collect::<Vec<_>>()
                .join("\n"),
        )))
    .await?;

    Ok(())
}

/// Show VRSC price information
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn price(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let resp: CoinPaprika =
        reqwest::get("https://api.coinpaprika.com/v1/tickers/vrsc-verus-coin?quotes=USD,BTC")
            .await?
            .json()
            .await?;

    // let supply: f64 = reqwest::get("https://explorer.verus.io/ext/getmoneysupply")
    //     .await?
    //     .text()
    //     .await?
    //     .trim()
    //     .parse()?;

    let btc_price = resp
        .quotes
        .get("BTC")
        .and_then(|obj| Some(obj.price))
        .unwrap_or(0.0);

    let usd_price = resp
        .quotes
        .get("USD")
        .and_then(|obj| Some(obj.price))
        .unwrap_or(0.0);

    let usd_volume = resp
        .quotes
        .get("USD")
        .and_then(|obj| Some(obj.volume_24h))
        .unwrap_or(0.0);

    let price_up = resp
        .quotes
        .get("BTC")
        .and_then(|obj| Some(obj.percent_change_24h))
        .unwrap_or(0.0)
        .is_sign_positive();

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .title("VRSC price information")
                .field("USD price", format!("$ {:.4} ", &usd_price), true)
                .field("BTC price", format!("₿ {:.8} ", &btc_price), true)
                .field(
                    "% from ATH (USD)",
                    resp.quotes
                        .get("USD")
                        .and_then(|obj| Some(obj.percent_from_price_ath))
                        .unwrap_or(0.0)
                        .to_string(),
                    false,
                )
                .field("Volume 24h (USD)", format!("{:.8}", &usd_volume), false)
                // .field("Circulating supply (VRSC)", format!("{}", supply), false)
                .timestamp(resp.last_updated)
                .color(match price_up {
                    true => Colour::DARK_GREEN,
                    false => Colour::RED,
                })
                .footer(
                    CreateEmbedFooter::new("Data from CoinPaprika")
                        .icon_url("https://i.imgur.com/wwH60Uf.png"),
                ),
        ),
    )
    .await?;

    Ok(())
}

/// Show currency information
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn currency(ctx: Context<'_>, name: String) -> Result<(), Error> {
    let verus_client = ctx.data().verus()?;
    let mut fields = vec![];

    if let Ok(currency) = verus_client.get_currency(&name) {
        fields.push(("Options", currency.options.to_string(), true));
        fields.push(("Proof protocol", currency.proofprotocol.to_string(), true));
        fields.push((
            "Id registration fees",
            currency.idregistrationfees.to_string(),
            false,
        ));
        fields.push((
            "Supply",
            currency
                .bestcurrencystate
                .map_or(0.0, |cs| cs.supply.as_vrsc())
                .to_string(),
            false,
        ));

        ctx.send(
            CreateReply::default().embed(
                CreateEmbed::new()
                    .title(format!("Currency: **{}**", currency.fullyqualifiedname))
                    .fields(fields)
                    .color(deterministic_color(currency.fullyqualifiedname)),
            ),
        )
        .await?;
    } else {
        ctx.send(
            CreateReply::default()
                .content("Invalid basket or basket not found")
                .ephemeral(true),
        )
        .await?;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct Reserve {
    name: String,
    amount: f64,
    price: f64,
}

async fn _basket(ctx: Context<'_>, basket_name: &str) -> Result<(), Error> {
    let verus_client = ctx.data().verus()?;

    if let Ok(currency) = verus_client.get_currency(basket_name) {
        if let Some(cs) = currency.bestcurrencystate.clone().as_ref() {
            if let Some(reserves) = cs.reservecurrencies.as_ref() {
                // the order of the ordered_reserve is to first always try to show currency
                // prices expressed in DAI, if that does not exist, show it in tBTC, and
                // if all else fails, show it in VRSC because that always is a currency.
                let mut main_reserve = None;
                for ordered_reserve in [
                    "iGBs4DWztRNvNEJBt4mqHszLxfKTNHTkhM", // DAI.vETH
                    "iS8TfRPfVpKo5FVfSUzfHBQxo9KuzpnqLU", // tBTC.vETH
                    "i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV", // VRSC (is always a reserve)
                ] {
                    main_reserve = reserves
                        .iter()
                        .find(|rc| rc.currencyid.to_string().as_str() == ordered_reserve);

                    if main_reserve.is_some() {
                        break;
                    }
                }

                let main_reserve = main_reserve.unwrap();
                let main_reserve_name = currency
                    .currencynames
                    .as_ref()
                    .unwrap()
                    .0
                    .get(&main_reserve.currencyid)
                    .unwrap()
                    .clone();

                let mut basket_reserves = reserves
                    .iter()
                    .filter_map(|rc| {
                        let name = currency
                            .currencynames
                            .as_ref()
                            .unwrap()
                            .0
                            .get(&rc.currencyid)
                            .unwrap()
                            .clone();
                        let rc_reserves = rc.reserves.as_vrsc();
                        // TODO `.checked_div()` needed
                        let price = if rc_reserves == 0.0 {
                            0.0
                        } else {
                            (main_reserve.reserves.as_vrsc() / rc_reserves)
                                / (main_reserve.weight / rc.weight)
                        };

                        Some(Reserve {
                            name,
                            amount: rc_reserves,
                            price,
                        })
                    })
                    .collect::<Vec<Reserve>>();

                let precision: usize = match &*main_reserve_name {
                    "DAI.vETH" => 2,
                    _ => 8,
                };

                let mut fields = vec![];

                let reserves_string = reserve_table_str(&mut basket_reserves, precision);
                debug!("{reserves_string:?}");

                fields.push((
                    format!("Reserves _(price in {main_reserve_name})_"),
                    reserves_string,
                    false,
                ));

                fields.push((
                    "Total value of liquidity".to_string(),
                    format!(
                        "{} {main_reserve_name}",
                        format!(
                            "{:.precision$}",
                            main_reserve.reserves.as_vrsc() / main_reserve.weight
                        )
                    ),
                    false,
                ));

                let blockheight = verus_client.get_blockchain_info()?.blocks;

                if let Ok(currencystate_res) = verus_client.get_currency_state(
                    basket_name,
                    Some(&format!("{},{},{}", blockheight - 1440, blockheight, 1440)),
                    Some(&main_reserve_name),
                ) {
                    if let Some(GetCurrencyStateResult::TotalVolume { totalvolume }) =
                        currencystate_res.last()
                    {
                        fields.push((
                            "Volume (24h)".to_string(),
                            format!(
                                "{} {main_reserve_name}",
                                format!("{:.precision$}", *totalvolume)
                            ),
                            false,
                        ));
                    }
                };

                fields.push((
                    "Supply".to_string(),
                    format!(
                        "{}",
                        format!(
                            "{:.precision$}",
                            currency
                                .bestcurrencystate
                                .as_ref()
                                .map_or(0.0, |cs| cs.supply.as_vrsc())
                        )
                    ),
                    true,
                ));

                fields.push((
                    "Price".to_string(),
                    format!(
                        "{} {main_reserve_name}",
                        format!(
                            "{:.precision$}",
                            (main_reserve.reserves.as_vrsc() / main_reserve.weight)
                                / currency
                                    .bestcurrencystate
                                    .as_ref()
                                    .map_or(0.0, |cs| cs.supply.as_vrsc())
                        )
                    ),
                    true,
                ));

                fields.push(("\u{200b}".to_string(), "\u{200b}".to_string(), true));

                // if in preconversion mode:
                let current_height = verus_client.get_blockchain_info()?.blocks;
                let start_block = currency.startblock;

                if let Some(future_time) = time_until_block(current_height, start_block) {
                    fields.push((
                        "\n\n\n:rotating_light:  PRECONVERSION MODE".to_string(),
                        " ".to_string(),
                        false,
                    ));

                    fields.push((
                        "Preconversion ends at approximately".to_string(),
                        future_time.to_rfc2822(),
                        false,
                    ))
                }

                ctx.send(
                    CreateReply::default().embed(
                        CreateEmbed::new()
                            .title(format!("Basket: **{}**", currency.fullyqualifiedname))
                            .fields(fields)
                            .color(deterministic_color(currency.fullyqualifiedname)),
                    ),
                )
                .await?;
            }
        } else {
            ctx.send(
                CreateReply::default()
                    .content("Invalid basket or basket not found")
                    .ephemeral(true),
            )
            .await?;
        }
    }

    Ok(())
}

#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn basket(ctx: Context<'_>, #[rename = "name"] basket_name: String) -> Result<(), Error> {
    _basket(ctx, &basket_name).await?;

    Ok(())
}

/// Show information about the contents of the VRSC-ETH bridge currency.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn ethbridge(ctx: Context<'_>) -> Result<(), Error> {
    _basket(ctx, "bridge.veth").await?;

    Ok(())
}

/// Show information about the contents of the vARRR bridge currency.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn varrrbridge(ctx: Context<'_>) -> Result<(), Error> {
    _basket(ctx, "bridge.varrr").await?;

    Ok(())
}

/// Show information about the contents of the Pure currency.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn pure(ctx: Context<'_>) -> Result<(), Error> {
    _basket(ctx, "pure").await?;

    Ok(())
}

#[derive(Deserialize, Debug)]
pub struct CoinPaprika {
    #[serde(rename = "id")]
    pub guid: String,
    pub symbol: String,
    pub last_updated: DateTime<Utc>,
    pub quotes: HashMap<String, CoinPaprikaQuoteCoin>,
}

#[derive(Deserialize, Debug)]
pub struct CoinPaprikaQuoteCoin {
    pub price: f64,
    pub volume_24h: f64,
    pub percent_change_24h: f64,
    pub percent_from_price_ath: f64,
    pub ath_price: f64,
    pub ath_date: String,
}

/// Shows the time until the next halving
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn halving(ctx: Context<'_>) -> Result<(), Error> {
    let data = [
        (384.0, 10080, 53279),
        (192.0, 53280, 96479),
        (96.0, 96480, 139679),
        (48.0, 139680, 182879),
        (24.0, 182880, 1277999),
        (12.0, 1278000, 2329919),
        (6.0, 2329920, 3381839),
        (3.0, 3381840, 4433759),
        (1.5, 4433760, 5485679),
        (0.75, 5485680, 6537599),
        (0.375, 6537600, 7589519),
        (0.1875, 7589520, 8641439),
        (0.09375, 8641440, 9693359),
        (0.046875, 9693360, 10745279),
        (0.0234375, 10745280, 11797199),
        (0.01171875, 11797200, 12849119),
        (0.00585938, 12849120, 13901039),
        (0.00292969, 13901040, 14952959),
        (0.00146484, 14952960, 16004879),
        (0.00073242, 16004880, 17056799),
        (0.00036621, 17056800, 18108719),
        (0.00018311, 18108720, 19160639),
        (0.00009155, 19160640, 20212559),
        (0.00004578, 20212560, 21264479),
        (0.00002289, 21264480, 22316399),
        (0.00001144, 22316400, 23368319),
        (0.00000572, 23368320, 24420239),
        (0.00000286, 24420240, 25472159),
        (0.00000143, 25472160, 26524079),
        (0.00000072, 26524080, 27575999),
        (0.00000036, 27576000, 28627919),
        (0.00000018, 28627920, 29679839),
        (0.00000009, 29679840, 30731759),
        (0.00000004, 30731760, 31783679),
        (0.00000002, 31783680, 32835599),
        (0.00000001, 32835600, 33887519),
        (0.00000001, 33887520, 34939439),
        (0.00000000, 34939440, 35991359),
    ];

    let blocks = ctx.data().verus()?.get_blockchain_info()?.blocks;

    let next_halving = data
        .iter()
        .find(|(_br, begin, end)| blocks >= *begin && blocks <= *end)
        .map(|(_, _, end)| end + 1)
        .unwrap_or(0);

    let time_to_halving = time_until_block(blocks, next_halving);

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .title("Next Verus halving")
                .field(
                    " ",
                    time_to_halving
                        .map_or(DateTime::<Utc>::default().to_rfc2822(), |f| f.to_rfc2822()),
                    false,
                )
                .color(Colour::GOLD),
        ),
    )
    .await?;

    Ok(())
}

/// Show the (estimated) time of a block.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous", rename = "time-of-block")]
pub async fn time_of_block(ctx: Context<'_>, block: u64) -> Result<(), Error> {
    let blocks = ctx.data().verus()?.get_blockchain_info()?.blocks;

    let mut fields = vec![];
    if block < blocks {
        let timestamp = ctx.data().verus()?.get_block_by_height(block, 2)?.time;
        let time = DateTime::from_timestamp(timestamp as i64, 0).unwrap();

        fields.push((" ", time.to_rfc2822(), false))
    } else {
        let now = chrono::Utc::now();
        let time = time_until_block(blocks, block).unwrap_or(now);
        let duration = Duration::seconds(((block - blocks) as f64 * 61.95) as i64);
        fields.push((" ", time.to_rfc2822(), false));
        fields.push((
            "Remaining",
            format!(
                "{} days, {} hours and {} minutes",
                duration.num_days(),
                duration.num_hours() % 24,
                duration.num_minutes() % 60
            ),
            false,
        ))
    };

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .title(format!("Time of block {block}"))
                .fields(fields)
                .color(Colour::BLURPLE),
        ),
    )
    .await?;

    Ok(())
}

/// Returns the DateTime in the future if current_height is not yet at future_height
fn time_until_block(current_height: u64, future_height: u64) -> Option<DateTime<Utc>> {
    // actual block time in practice is 61.95s, so we multiply with 1.0325
    // https://discord.com/channels/444621794964537354/449633463394500629/1121389199451500625
    let diff = future_height
        .checked_sub(current_height)
        .map(|d| d as f64 * 1.0325);

    let now = chrono::Utc::now();

    diff.and_then(|diff| now.checked_add_signed(Duration::minutes(diff as i64)))
}

fn reserve_table_str(reserves: &mut Vec<Reserve>, precision: usize) -> String {
    let longest_name_len = reserves
        .iter()
        .max_by_key(|x| x.name.chars().count())
        .unwrap()
        .name
        .len();

    let largest_value = reserves
        .iter()
        .map(|t| t.amount as u64)
        .reduce(|acc, amount| amount.max(acc))
        .unwrap();

    debug!("largest value: {largest_value}");
    let longest_value_len = format!("{:.precision$}", largest_value);
    debug!("longest_value_len: {longest_value_len}");
    let longest_value_len = largest_value.to_string().len() + precision;
    debug!("longest_value_len: {longest_value_len}");

    reserves.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    format!(
        "```{}```",
        reserves
            .iter()
            .map(|tvl| format!(
                "{name:<max_name_len$} : {amount:>max$.*} ({price:.precision$})",
                precision,
                name = tvl.name,
                amount = tvl.amount,
                price = tvl.price,
                max_name_len =
                    longest_name_len - tvl.name.chars().filter(|c| !c.is_ascii()).count(),
                max = longest_value_len + 1
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn deterministic_color<T: std::hash::Hash>(string: T) -> u64 {
    let mut s = DefaultHasher::new();
    string.hash(&mut s);
    s.finish() % 16777215
}
