use poise::{serenity_prelude::UserId, CreateReply};
use tracing::trace;

use crate::{Context, Error};

pub mod admin;
pub mod chain;
pub mod misc;
pub mod tipping;
pub mod wallet;

async fn user_blacklisted(ctx: Context<'_>, user_id: UserId) -> Result<bool, Error> {
    let blacklist = &ctx.data().blacklist;

    if blacklist.lock().unwrap().contains(&user_id) {
        trace!("user is blacklisted");
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content(format!("You have been temporarily suspended")),
        )
        .await?;

        return Ok(true);
    }

    Ok(false)
}
