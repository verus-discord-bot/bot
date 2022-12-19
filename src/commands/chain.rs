use tracing::instrument;
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
