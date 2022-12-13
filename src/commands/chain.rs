use poise::ChoiceParameter;
use tracing::{debug, instrument};
use uuid::Uuid;
use vrsc_rpc::RpcApi;

use crate::{Context, Error};
/// Show information about Verus blockchain.
#[instrument(skip(ctx, set), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(track_edits, slash_command, category = "Miscellaneous")]
pub async fn info(ctx: Context<'_>, set: Pbaas) -> Result<(), Error> {
    debug!("chosen coin: {:?}", set);
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

#[derive(Debug, ChoiceParameter)]
pub enum Pbaas {
    #[name = "Andromeda"]
    Andromeda,
    #[name = "Gravity"]
    Gravity,
    #[name = "Quantum"]
    Quantum,
}
