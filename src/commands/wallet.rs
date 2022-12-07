use qrcode::{render::unicode, QrCode};
use tracing::*;
use uuid::Uuid;
use vrsc_rpc::RpcApi;

use crate::{Context, Error};
/// Deposit funds to the tipbot wallet
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn deposit(ctx: Context<'_>) -> Result<(), Error> {
    info!(
        "user {} ({}) demands a deposit address",
        ctx.author().name,
        ctx.author().id
    );
    let pool = &ctx.data().database;
    let client = &ctx.data().verus;

    if let Some(row) = sqlx::query_as!(
        DiscordUserDBData,
        "SELECT discord_id, vrsc_address FROM discord_users WHERE discord_id = $1",
        i64::from(ctx.author().id)
    )
    .fetch_optional(pool)
    .await?
    {
        info!("address already stored, return it");
        send_address_message(ctx, row.vrsc_address).await?;
    } else {
        let address = client.get_new_address()?;
        // simultaneously add row to both `discord_users` and `balance_vrsc` with an initial balance of 0.
        sqlx::query!(
            "WITH inserted_row AS (
                INSERT INTO discord_users (discord_id, vrsc_address) 
                VALUES ($1, $2)
            )
            INSERT INTO balance_vrsc (discord_id)
            VALUES ($1)
            ",
            i64::from(ctx.author().id),
            &address.to_string()
        )
        .execute(pool)
        .await?;

        send_address_message(ctx, address.to_string()).await?;
    }

    Ok(())
}

async fn send_address_message(ctx: Context<'_>, address: String) -> Result<(), Error> {
    ctx.send(|reply| {
        let qr = QrCode::new(&address).unwrap();
        let image_str = qr
            .render::<unicode::Dense1x2>()
            .module_dimensions(1, 1)
            .build();

        reply.ephemeral(true).embed(|embed| {
            embed.title(format!("Deposit address: {}", &address)).field(
                "code",
                format!("```{image_str}```"),
                false,
            )
        })
    })
    .await?;

    Ok(())
}

#[allow(dead_code)]
struct DiscordUserDBData {
    discord_id: i64,
    vrsc_address: String,
}
