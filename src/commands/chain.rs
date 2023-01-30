use poise::serenity_prelude::Colour;
use serde_json::Value;
use tracing::instrument;
use uuid::Uuid;
use vrsc_rpc::RpcApi;

use crate::{Context, Error};
/// Show information about Verus blockchain.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(track_edits, slash_command, category = "Miscellaneous")]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let blockchain_info = &ctx.data().verus()?.get_blockchain_info()?;
    let testnet_name = match ctx.data().settings.application.testnet {
        true => "Verus (testnet)",
        false => "Verus",
    };

    ctx.send(|reply| {
        reply.embed(|embed| {
            embed
                .title(format!("{} info", testnet_name))
                .field("height", blockchain_info.blocks, false)
                .field("difficulty", blockchain_info.difficulty, false)
        })
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
        reply.content(format!(
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

    let resp: Value =
        reqwest::get("https://api.coingecko.com/api/v3/coins/verus-coin?tickers=true")
            .await?
            .json()
            .await?;

    let btc_price = get_current_price(&resp, "btc");
    let usd_price = get_current_price(&resp, "usd");

    let price_up = get_f64(&resp, "price_change_24h_in_currency", "usd").is_sign_positive();

    ctx.send(|reply| {
        reply.embed(|embed| {
            embed
                .title("VRSC price information")
                .field("USD price", format!("$ {:.4}", &usd_price), true)
                .field("BTC price", format!("₿ {:.8} ", &btc_price), true)
                .field(
                    "% from ATH (USD)",
                    format!("{:.2} %", get_f64(&resp, "ath_change_percentage", "usd")),
                    false,
                )
                .field("Volume 24h (USD)", format!("{:.2}", get_f64(&resp, "total_volume", "usd")), false)
                .field(
                    "Circulating supply (VRSC)",
                    format!(
                        "{}",
                        resp.get("market_data")
                            .and_then(|data| data.get("circulating_supply")
                            .and_then(|supply| supply.as_f64()))
                            .unwrap()
                    ),
                    false,
                )
                .timestamp(resp.get("last_updated").unwrap().as_str().unwrap())
                .color(match price_up {
                    true => Colour::DARK_GREEN,
                    false => Colour::RED,
                })
                .footer(|footer| footer.text("Data from Coingecko").icon_url("https://static.coingecko.com/s/thumbnail-d5a7c1de76b4bc1332e48227dc1d1582c2c92721b5552aae76664eecb68345c9.png"))
        })
    })
    .await?;

    Ok(())
}

fn get_current_price(obj: &Value, ticker: &str) -> f64 {
    obj.get("market_data")
        .and_then(|m_data| {
            m_data
                .get("current_price")
                .and_then(|cur_price| cur_price.get(ticker))
        })
        .and_then(|price| price.as_f64())
        .unwrap()
}

fn get_f64(obj: &Value, key: &str, ticker: &str) -> f64 {
    obj.get("market_data")
        .and_then(|m_data| m_data.get(key).and_then(|key| key.get(ticker)))
        .and_then(|price| price.as_f64())
        .unwrap()
}
