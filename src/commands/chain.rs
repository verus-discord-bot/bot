use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::Hasher,
};

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use poise::serenity_prelude::Colour;
use serde::Deserialize;
use thousands::Separable;
use tracing::{debug, instrument};
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::client::{Client, RpcApi};

use crate::{Context, Error};

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

    ctx.send(|reply| {
        reply
            .embed(|embed| {
                embed
                    .title(format!("{} info", testnet_name))
                    .field("height", blockchain_info.blocks, false)
                    .field("difficulty", blockchain_info.difficulty, false)
                    .field(
                        "amount staking",
                        Amount::from_vrsc(mining_info.stakingsupply).unwrap(),
                        false,
                    )
                    .field(
                        "average block fees",
                        Amount::from_vrsc(mining_info.averageblockfees).unwrap(),
                        false,
                    )
            })
            .ephemeral(true)
    })
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

    ctx.send(|reply| {
        reply.ephemeral(true).content(format!(
            "Publicly available peers:```{}```",
            peer_info
                .into_iter()
                .map(|peer| peer.addr)
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    })
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

    // TODO: get current circulating supply
    // The below does not work because it is very slow (5+ secs)
    //
    // let reqw_client = reqwest::Client::new();
    // let res: Value = reqw_client
    //     .post("https://api.verus.services")
    //     .json(&json!({"method":"coinsupply", "params":[]}))
    //     .send()
    //     .await?
    //     .json()
    //     .await?;

    // let supply: f64 = if let Some(result) = res["result"].as_object() {
    //     result["supply"].as_f64().unwrap_or(0.0)
    // } else {
    //     reqwest::get("https://explorer.verus.io/ext/getmoneysupply")
    //         .await?
    //         .text()
    //         .await?
    //         .parse::<f64>()?
    // };

    // dbg!(supply);

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

    ctx.send(|reply| {
        reply.embed(|embed| {
            embed
                .title("VRSC price information")
                .field("USD price", format!("$ {:.4} ", &usd_price), true)
                .field("BTC price", format!("₿ {:.8} ", &btc_price), true)
                .field(
                    "% from ATH (USD)",
                    resp.quotes
                        .get("USD")
                        .and_then(|obj| Some(obj.percent_from_price_ath))
                        .unwrap_or(0.0),
                    false,
                )
                .field("Volume 24h (USD)", format!("{:.8}", &usd_volume), false)
                // .field("Circulating supply (VRSC)", format!("{}", supply), false)
                .timestamp(resp.last_updated)
                .color(match price_up {
                    true => Colour::DARK_GREEN,
                    false => Colour::RED,
                })
                .footer(|footer| {
                    footer
                        .text("Data from CoinPaprika")
                        .icon_url("https://i.imgur.com/wwH60Uf.png")
                })
        })
    })
    .await?;

    Ok(())
}

// for all reserve currencies:
// (reserves of DAI / reserves of currency) == price of reserve, DAI, VRSC, vETH, or MKR in DAI
// for the basket currency
// (reserves of DAI * number of currencies, which will be 4 for the bridge / reserves of currency) == price of basket currency, Bridge.vETH, in DAI

/// Show currency information
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn currency(ctx: Context<'_>, name: String) -> Result<(), Error> {
    let verus_client = ctx.data().verus()?;
    let mut fields = vec![];

    if let Ok(currency) = verus_client.get_currency(&name) {
        // let currency = verus_client.get_currency(&name)?;
        fields.push(("Options", currency.options.to_string(), true));
        fields.push(("Proof protocol", currency.proofprotocol.to_string(), true));
        fields.push((
            "Id registration fees",
            currency.idregistrationfees.to_string(),
            false,
        ));
        fields.push((
            "Supply",
            currency.bestcurrencystate.supply.as_vrsc().to_string(),
            false,
        ));

        ctx.send(|reply| {
            reply.embed(|embed| {
                embed
                    .title(format!("Currency: **{}**", currency.fullyqualifiedname))
                    .fields(fields)
                    .color(Colour::DARK_GREEN)
            })
        })
        .await?;
    } else {
        ctx.send(|reply| {
            reply
                .content("Invalid basket or basket not found")
                .ephemeral(true)
        })
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

fn reserve_table_str(reserves: &mut Vec<Reserve>, precision: usize) -> String {
    let longest_name_len = reserves
        .iter()
        .max_by_key(|x| x.name.len())
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
                "{name:<max_name_len$}: {amount:>max$.*} ({price:.precision$})",
                precision,
                name = tvl.name,
                amount = tvl.amount,
                price = tvl.price,
                max_name_len = longest_name_len + 1,
                max = longest_value_len + 1
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn format_amount(amount: f64, precision: usize) -> String {
    // let amount = amount.separate_with_commas();
    // let mut v = amount.split('.');

    // let first = v.nth(0).unwrap();
    // let last = v.nth(0).unwrap_or("0000000000");

    // let (last, _) = last.split_at(precision);

    // let amount = format!("{}.{}", first, last);

    format!("{:.precision$}", amount)
}

async fn _basket(ctx: Context<'_>, basket_name: &str) -> Result<(), Error> {
    let verus_client = ctx.data().verus()?;

    if let Ok(currency) = verus_client.get_currency(basket_name) {
        if let Some(reserves) = currency.bestcurrencystate.reservecurrencies.as_ref() {
            // need to find reserve in this order:
            let mut main_reserve = None;
            for ordered_reserve in [
                "iS8TfRPfVpKo5FVfSUzfHBQxo9KuzpnqLU", // tBTC.vETH
                "iGBs4DWztRNvNEJBt4mqHszLxfKTNHTkhM", // DAI.vETH
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

            // for (i, b) in basket_reserves.iter().enumerate() {
            //     let amount = format_amount(b.amount, precision);
            //     let price = format_amount(b.price, precision);

            //     fields.push((b.name.clone(), format!("{}\n({})", &amount, price), true));
            //     if i % 3 == 1 {
            //         fields.push(("\u{200b}".to_string(), "\u{200b}".to_string(), true));
            //     }
            // }

            // while fields.len() % 3 != 0 {
            //     fields.push(("\u{200b}".to_string(), "\u{200b}".to_string(), true));
            // }

            fields.push((
                "Total value of liquidity".to_string(),
                format!(
                    "{} {main_reserve_name}",
                    format_amount(
                        main_reserve.reserves.as_vrsc() / main_reserve.weight,
                        precision,
                    ),
                ),
                false,
            ));

            fields.push((
                "Supply".to_string(),
                format!(
                    "{}",
                    format_amount(currency.bestcurrencystate.supply.as_vrsc(), precision)
                ),
                true,
            ));

            fields.push((
                "Price".to_string(),
                format!(
                    "{} {main_reserve_name}",
                    format_amount(
                        (main_reserve.reserves.as_vrsc() / main_reserve.weight)
                            / currency.bestcurrencystate.supply.as_vrsc(),
                        precision
                    )
                ),
                true,
            ));

            fields.push(("\u{200b}".to_string(), "\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}\u{200b}".to_string(), true));

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

            ctx.send(|reply| {
                reply.embed(|embed| {
                    embed
                        .title(format!("Basket: **{}**", currency.fullyqualifiedname))
                        // .description(format!("_(price in {})_", main_reserve_name))
                        .fields(fields)
                        .color(deterministic_color(basket_name))
                })
            })
            .await?;
        }
    } else {
        ctx.send(|reply| {
            reply
                .content("Invalid basket or basket not found")
                .ephemeral(true)
        })
        .await?;
    }

    Ok(())
}

fn deterministic_color<T: std::hash::Hash>(string: T) -> u64 {
    let mut s = DefaultHasher::new();
    string.hash(&mut s);
    dbg!(s.finish() % 16777215)
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

/// Show information about the contents of the VRSC-ETH bridge currency.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn varrrbridge(ctx: Context<'_>) -> Result<(), Error> {
    _basket(ctx, "bridge.varrr").await?;

    Ok(())
}

/// Show information about the contents of the VRSC-ETH bridge currency.
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
    // pub circulating_supply: u64,
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
    let next_halving = 3381840;
    let blocks = ctx.data().verus()?.get_blockchain_info()?.blocks;

    let time_to_halving = time_until_block(blocks, next_halving);

    ctx.send(|reply| {
        reply.embed(|embed| {
            embed
                .title("Next Verus halving")
                .field(
                    " ",
                    time_to_halving
                        .map_or(DateTime::<Utc>::default().to_rfc2822(), |f| f.to_rfc2822()),
                    false,
                )
                .color(Colour::GOLD)
        })
    })
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
        let time = NaiveDateTime::from_timestamp_opt(timestamp as i64, 0)
            .unwrap()
            .and_utc();

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

    ctx.send(|reply| {
        reply.embed(|embed| {
            embed
                .title(format!("Time of block {block}"))
                .fields(fields)
                .color(Colour::BLURPLE)
        })
    })
    .await?;

    Ok(())
}

/// Returns the DateTime in the future if current_height is not yet at future_height
fn time_until_block(current_height: u64, future_height: u64) -> Option<DateTime<Utc>> {
    // actual block time is 61.95s, so we multiply with 1.0325
    // https://discord.com/channels/444621794964537354/449633463394500629/1121389199451500625
    let diff = future_height
        .checked_sub(current_height)
        .map(|d| d as f64 * 1.0325);

    let now = chrono::Utc::now();

    diff.and_then(|diff| now.checked_add_signed(Duration::minutes(diff as i64)))
}

#[allow(unused)]
fn get_vrsc_price_in_dai(verus_client: &Client) -> Option<Amount> {
    if let Ok(currency_state) = verus_client.get_currency_state("bridge.vETH") {
        let currency_state = currency_state.first().unwrap();

        if let Some(reserve_currencies) = currency_state.currencystate.reservecurrencies.as_ref() {
            let dai_reserves = reserve_currencies
                .iter()
                .find(|c| &c.currencyid.to_string() == "iGBs4DWztRNvNEJBt4mqHszLxfKTNHTkhM")
                .and_then(|f| Some(f.reserves.as_vrsc()))
                .unwrap_or(0.0);

            let vrsc_reserves = reserve_currencies
                .iter()
                .find(|c| &c.currencyid.to_string() == "i5w5MuNik5NtLcYmNzcvaoixooEebB6MGV")
                .and_then(|f| Some(f.reserves.as_vrsc()))
                .unwrap_or(0.0);

            let vrsc_price_in_dai = dai_reserves / vrsc_reserves;

            return Some(
                Amount::from_str_in(
                    &format!("{:.8}", vrsc_price_in_dai),
                    vrsc::Denomination::Verus,
                )
                .unwrap_or(Amount::ZERO),
            );
        }
    }

    None
}
