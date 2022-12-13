use tracing::debug;
use vrsc::Amount;

use crate::{Context, Error};

#[poise::command(owners_only, prefix_command, hide_in_help)]
pub async fn set_withdrawal_fee(ctx: Context<'_>, amount: u64) -> Result<(), Error> {
    let withdrawal_fee = &ctx.data().withdrawal_fee;

    debug!("fee before changing: {:?}", withdrawal_fee);

    let mut write = withdrawal_fee.write().await;
    *write = Amount::from_sat(amount);

    debug!("fee after changing: {:?}", withdrawal_fee);

    Ok(())
}
