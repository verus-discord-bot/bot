use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use poise::serenity_prelude::Colour;
use serde::Deserialize;
use tracing::{debug, instrument};
use uuid::Uuid;
use vrsc::Amount;
use vrsc_rpc::client::RpcApi;

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
                .field(
                    "Circulating supply (VRSC)",
                    format!("{}", resp.circulating_supply),
                    false,
                )
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

/// Show currency information
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn currency(ctx: Context<'_>, currency: String) -> Result<(), Error> {
    let verus_client = ctx.data().verus()?;
    let price: CoinPaprika =
        reqwest::get("https://api.coinpaprika.com/v1/tickers/vrsc-verus-coin?quotes=USD,BTC")
            .await?
            .json()
            .await?;

    let usd_price = price
        .quotes
        .get("USD")
        .and_then(|obj| Some(obj.price))
        .unwrap_or(0.0);

    let mut fields = vec![];

    if let Ok(currency_state) = verus_client.get_currency_state(&currency) {
        let currency_state = currency_state.first().unwrap();
        fields.push((
            "Supply",
            format!("`{}`", currency_state.currencystate.supply.as_vrsc()),
            false,
        ));

        if let Some(reserve_currencies) = currency_state.currencystate.reservecurrencies.as_ref() {
            let mut baskets = reserve_currencies
                .iter()
                .filter_map(|rc| {
                    let name = ctx.data().to_currency_name(&rc.currencyid).ok().unwrap();
                    Some((name, rc.reserves.as_vrsc()))
                })
                .collect::<Vec<(String, f64)>>();

            let longest_name_len = baskets.iter().max_by_key(|x| x.0.len()).unwrap().0.len();
            let longest_value_len = format!(
                "{}",
                baskets
                    .iter()
                    .map(|t| t.1 * 100_000_000.0)
                    .reduce(|acc, amount| amount.max(acc))
                    .unwrap()
            )
            .len();

            debug!("{longest_value_len}");

            baskets.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

            let tvl_str = format!(
                "```{}```",
                baskets
                    .iter()
                    .map(|tvl| format!(
                        "{name:<max_name_len$}: {value:>max$.*}",
                        8,
                        name = tvl.0,
                        value = tvl.1,
                        max_name_len = longest_name_len + 1,
                        max = longest_value_len + 1
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            );

            println!("{}", tvl_str);

            fields.push(("Baskets", tvl_str, false));

            // divide supply by the lastconversionprice of verus
            if ctx.data().settings.application.testnet {
                let price = dbg!(reserve_currencies
                    .iter()
                    .find(|c| c.currencyid.to_string() == "iJhCezBExJHvtyH3fGhNnt2NhU4Ztkf2yq")
                    .and_then(|c| Some(c.priceinreserve))
                    .unwrap_or(Amount::ZERO));

                let vrsc_value_of_currency_supply = dbg!(
                    currency_state.currencystate.supply.as_vrsc() * price.as_vrsc() //.unwrap_or(Amount::ZERO)
                );

                let dollar_value_of_currency_supply =
                    dbg!(vrsc_value_of_currency_supply * usd_price);

                fields.push((
                    "est. currency value (USD)",
                    format!("$ {dollar_value_of_currency_supply:.2}"),
                    false,
                ));
            }
        }

        ctx.send(|reply| {
            reply.embed(|embed| {
                embed
                    .title(format!("`{}` currency information", currency))
                    .fields(fields)
            })
        })
        .await?;
    }

    Ok(())
}

/// Show information about the contents of the VRSC-ETH bridge currency.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn ethbridge(ctx: Context<'_>) -> Result<(), Error> {
    // the contents will be DAI, MKR, VRSC and ETH.
    // we need to get the actual Dollar price of DAI, MKR and ETH.

    let verus_client = ctx.data().verus()?;
    let all_prices: Vec<CoinPaprika> =
        reqwest::get("https://api.coinpaprika.com/v1/tickers?quotes=USD")
            .await?
            .json()
            .await?;

    let mut fields = vec![];

    // let currency = verus_client.get_currency("bridge.vETH")?;
    let start_block: u64 = 2758800;
    let cur_height = verus_client.get_blockchain_info()?.blocks;
    // let diff = start_block
    //     .checked_sub(cur_height)
    //     // actual block time is 61.95s, so we multiply with 1.0325
    //     // https://discord.com/channels/444621794964537354/449633463394500629/1121389199451500625
    //     .map(|d| d as f64 * 1.0325);

    if let Ok(currency_state) = verus_client.get_currency_state("bridge.vETH") {
        let currency_state = currency_state.first().unwrap();

        if let Some(reserve_currencies) = currency_state.currencystate.reservecurrencies.as_ref() {
            let dai_reserves = reserve_currencies
                .iter()
                .find(|c| &c.currencyid.to_string() == "iGBs4DWztRNvNEJBt4mqHszLxfKTNHTkhM")
                .and_then(|f| Some(f.reserves.as_vrsc()))
                .unwrap_or(0.0);

            let mut baskets = reserve_currencies
                .iter()
                .filter_map(|rc| {
                    let name = ctx.data().to_currency_name(&rc.currencyid).ok().unwrap();
                    let dai_price = dai_reserves / rc.reserves.as_vrsc();

                    Some((name, rc.reserves.as_vrsc(), dai_price))
                })
                .collect::<Vec<(String, f64, f64)>>();

            let longest_name_len = baskets.iter().max_by_key(|x| x.0.len()).unwrap().0.len();
            let largest_value = baskets
                .iter()
                .map(|t| t.1 * 100_000_000.0)
                .reduce(|acc, amount| amount.max(acc))
                .unwrap();

            debug!("largest value: {largest_value}");
            let longest_value_len = format!("{:.8}", largest_value);
            debug!("longest_value_len: {longest_value_len}");
            let longest_value_len = largest_value.to_string().len() - 4;
            debug!("longest_value_len: {longest_value_len}");

            baskets.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

            let tvl_str = format!(
                "```{}```",
                baskets
                    .iter()
                    .map(|tvl| format!(
                        "{name:<max_name_len$}: {value:>max$.*} ({dai:.2})",
                        4,
                        name = tvl.0,
                        value = tvl.1,
                        dai = tvl.2,
                        max_name_len = longest_name_len + 1,
                        max = longest_value_len + 1
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            );

            fields.push(("Reserves (price in DAI)", tvl_str, false));

            fields.push((
                "Total value of liquidity",
                format!("{:.2} DAI", baskets.len() as f64 * dai_reserves),
                false,
            ));

            // if in preconversion mode:
            if let Some(future_time) = time_until_block(cur_height, start_block) {
                fields.push((
                    "\n\n\n------ PRECONVERSION MODE ------",
                    " ".to_string(),
                    false,
                ));

                fields.push((
                    "Preconversion ends at approximately",
                    future_time.to_rfc2822(),
                    false,
                ))
            } else {
                fields.push((
                    "Supply",
                    format!("{}", currency_state.currencystate.supply.as_vrsc()),
                    false,
                ));
            }
        }
    }

    ctx.send(|reply| {
        reply.embed(|embed| {
            embed
                .title("VRSC-ETH Bridge information")
                .fields(fields)
                .color(Colour::DARK_BLUE)
        })
    })
    .await?;

    Ok(())
}

#[derive(Deserialize, Debug)]
pub struct CoinPaprika {
    #[serde(rename = "id")]
    pub guid: String,
    pub symbol: String,
    pub circulating_supply: u64,
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

/// All-time high information
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn ath(ctx: Context<'_>) -> Result<(), Error> {
    let price: CoinPaprika =
        reqwest::get("https://api.coinpaprika.com/v1/tickers/vrsc-verus-coin?quotes=USD,BTC,ETH")
            .await?
            .json()
            .await?;

    let btc_ath = price.quotes.get("BTC").unwrap();
    let usd_ath = price.quotes.get("USD").unwrap();
    let eth_ath = price.quotes.get("ETH").unwrap();

    ctx.send(|message| {
        message.embed(|embed| {
            embed
                .field("BTC", format!("₿ {:.8}", btc_ath.ath_price), true)
                .field("ETH", format!("Ξ {:.8}", eth_ath.ath_price), true)
                .field("USD", format!("$ {:.2}", usd_ath.ath_price), true)
                .field(
                    "% from ATH",
                    format!("{}%", btc_ath.percent_from_price_ath),
                    true,
                )
                .field(
                    "% from ATH",
                    format!("{}%", eth_ath.percent_from_price_ath),
                    true,
                )
                .field(
                    "% from ATH",
                    format!("{}%", usd_ath.percent_from_price_ath),
                    true,
                )
        })
    })
    .await?;

    Ok(())
}
