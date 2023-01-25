use poise::serenity_prelude::Colour;
use serde_json::Value;
use tracing::{debug, instrument};
use uuid::Uuid;
use vrsc_rpc::RpcApi;

use crate::{Context, Error};
/// Show information about Verus blockchain.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(track_edits, slash_command, category = "Miscellaneous")]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let blockchain_info = &ctx.data().verus.get_blockchain_info()?;
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

/// Show VRSC price information
#[instrument(skip(_ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Miscellaneous")]
pub async fn price(_ctx: Context<'_>) -> Result<(), Error> {
    let resp: Value =
        reqwest::get("https://api.coinpaprika.com/v1/tickers/vrsc-verus-coin?quotes=USD,BTC")
            .await?
            .json()
            .await?;
    debug!("json response: {:#?}", &resp);

    // if resp.is_null()

    let btc_price = get_f64(&resp, "price", "BTC");
    let usd_price = get_f64(&resp, "price", "USD");
    let usd_volume = get_f64(&resp, "volume_24h", "USD");

    debug!("btc_price: {:.8}", &btc_price);
    debug!("usd_price: {:.8}", &usd_price);

    let price_up = get_f64(&resp, "percent_change_24h", "USD").is_sign_positive();

    _ctx.send(|reply| {
        reply.embed(|embed| {
            embed
                .title("VRSC price information")
                .field("USD price", format!("$ {:.4}", &usd_price), true)
                .field("BTC price", format!("₿ {:.8} ", &btc_price), true)
                .field(
                    "% from ATH (USD)",
                    get_f64(&resp, "percent_from_price_ath", "USD"),
                    false,
                )
                .field("Volume 24h (USD)", format!("{:.8}", &usd_volume), false)
                .field(
                    "Circulating supply (VRSC)",
                    format!(
                        "{}",
                        resp.get("circulating_supply")
                            .and_then(|supply| supply.as_f64())
                            .unwrap_or(0.0),
                    ),
                    false,
                )
                .timestamp(resp.get("last_updated").unwrap().as_str().unwrap())
                .color(match price_up {
                    true => Colour::DARK_GREEN,
                    false => Colour::RED,
                })
        })
    })
    .await?;

    Ok(())
}

fn get_f64(obj: &Value, key: &str, denom: &str) -> f64 {
    obj.get("quotes")
        .and_then(|quotes| quotes.get(denom))
        .and_then(|price_obj| price_obj.get(key))
        .and_then(|price| price.as_f64())
        .unwrap()
}
