use std::{fmt::Display, str::FromStr, sync::Arc, time::Duration};

use poise::serenity_prelude::{
    ArgumentConvert, ChannelId, CreateMessage, EditMessage, Http, Message, MessageId, ReactionType,
    UserId,
};
use sqlx::{
    PgPool,
    types::chrono::{self, DateTime, Utc},
};
use tokio::time::sleep;
use tokio_graceful_shutdown::{IntoSubsystem, SubsystemHandle};
use tracing::{debug, error, info, trace};
use vrsc::Amount;

use crate::{Error, commands, database};

#[derive(Debug)]
pub enum ReactdropState {
    Pending,
    Processed,
}

impl Display for ReactdropState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Processed => write!(f, "processed"),
        }
    }
}

impl From<String> for ReactdropState {
    fn from(value: String) -> Self {
        match value.as_ref() {
            "pending" => ReactdropState::Pending,
            "processed" => ReactdropState::Processed,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
pub struct Reactdrop {
    pub author: UserId,
    pub status: ReactdropState,
    pub emoji: String,
    pub tip_amount: Amount,
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub finish_time: DateTime<Utc>,
}

pub struct Subsystem {
    pub http: Arc<Http>,
    pub pool: PgPool,
}

impl Subsystem {
    pub async fn check_running_reactdrops(&self) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        let pending_reactdrops = database::get_pending_reactdrops(&mut *tx).await?;

        if pending_reactdrops.is_empty() {
            return Ok(());
        }

        let now = chrono::Utc::now();
        trace!(
            "number of pending reactdrops.{} at.{}",
            pending_reactdrops.len(),
            now
        );

        for reactdrop in pending_reactdrops {
            let mut message: Message = ArgumentConvert::convert(
                &self.http,
                None,
                Some(reactdrop.channel_id),
                reactdrop.message_id.to_string().as_ref(),
            )
            .await?;

            let diff = reactdrop.finish_time.signed_duration_since(now);
            let diff_fmt = || -> String {
                match diff.num_seconds() {
                    t @ 0..=3600 => format!("{} minute(s)", t / 60),
                    t @ _ => {
                        format!("{} hour(s) and {} minute(s)", t / (60 * 60), (t / 60) % 60)
                    }
                }
            };
            debug!("{diff:?}");

            let content: &str = message.content.as_ref();
            let split = content.find("Time remaining: ").unwrap();
            let new_content = format!("{}Time remaining: {}", &content[..split], diff_fmt());

            message
                .edit(&self.http, EditMessage::new().content(new_content))
                .await?;

            if reactdrop.finish_time <= now {
                let mut last_user = None;
                let mut reaction_users = vec![];

                while let Ok(users) = message
                    .reaction_users(
                        &self.http,
                        ReactionType::from_str(&reactdrop.emoji)?,
                        Some(50),
                        last_user,
                    )
                    .await
                {
                    debug!("appending {} users", users.len());
                    reaction_users.extend(users.clone());

                    debug!("{users:?}");

                    last_user = users.last().map(|user| user.id);
                    if last_user.is_none() {
                        break;
                    }
                }

                debug!(
                    "retrieved {} users who reacted on reactdrop tip\n{:#?}",
                    reaction_users.len(),
                    reaction_users
                );

                let reaction_users = reaction_users
                    .iter()
                    .filter(|user| !user.bot)
                    .map(|u| u.id)
                    .collect::<Vec<_>>();

                if reaction_users.len() == 0 {
                    trace!("no users to tip, abort");
                } else {
                    trace!("tipping {} users in reactdrop", reaction_users.len());

                    if let Err(e) = commands::tipping::tip_multiple_users(
                        &mut tx,
                        reactdrop.author,
                        &self.http,
                        &reactdrop.channel_id,
                        &reaction_users,
                        &reactdrop.tip_amount,
                        "reactdrop",
                    )
                    .await
                    {
                        error!("{e:?}");

                        reactdrop
                            .channel_id
                            .send_message(
                                &self.http,
                                CreateMessage::new().content(format!(
                                    "<@{}> didn't have enough funds, reactdrop failed",
                                    &message.author.id,
                                )),
                            )
                            .await?;
                    }
                }

                reactdrop
                    .channel_id
                    .delete_reaction_emoji(
                        &self.http,
                        message,
                        ReactionType::from_str(&reactdrop.emoji)?,
                    )
                    .await?;

                database::update_reactdrop(
                    &mut *tx,
                    reactdrop.channel_id.get() as i64,
                    reactdrop.message_id.get() as i64,
                    ReactdropState::Processed,
                )
                .await?;

                info!("processed reactdrop: {reactdrop:#?}");
            }
        }

        tx.commit().await?;

        Ok(())
    }
}

impl IntoSubsystem<Box<dyn std::error::Error + Send + Sync>> for Subsystem {
    async fn run(self, subsys: &mut SubsystemHandle) -> Result<(), Error> {
        while !subsys.is_shutdown_requested() {
            if let Err(e) = self.check_running_reactdrops().await {
                tracing::error!(?e, "Something went wrong when checking reactdrops");
            }

            tokio::select! {
                _ = sleep(Duration::from_secs(10)) => {}
                _ = subsys.on_shutdown_requested() => {}
            }
        }

        Ok(())
    }
}
