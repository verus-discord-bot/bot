use serenity::{
    async_trait,
    model::prelude::{interaction::Interaction, GuildId, Ready},
    prelude::{Context, EventHandler},
};
use tracing::{debug, info, instrument, trace};
use uuid::Uuid;

use crate::global_data::AppConfig;

#[derive(Debug)]
pub struct Handler {}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _ready: Ready) {
        let app_config = {
            let data_read = ctx.data.read().await;
            data_read.get::<AppConfig>().unwrap().clone()
        };

        let guild_id = GuildId(
            app_config
                .application
                .discord_guild_id
                .parse::<u64>()
                .expect("a discord guild id"),
        );

        let commands = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands.create_application_command(|cmd| {
                cmd.name("help").description("List all bot commands")
            })
        });

        let result = commands.await;
        debug!("Registered commands: {:?}", result);
        if let Err(error) = result {
            panic!("Commands were not registered successfully:\n{:#?}", error);
        }
    }

    // with every new interaction, a random uuid is generated to trace the interaction through the various steps, like db processing or verus daemon interaction.
    // if something goes wrong, the request_id gives the ability to trace back to where things went south.
    #[instrument(level = "trace", skip(ctx, interaction), fields(
        request_id = %Uuid::new_v4()
    ))]
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            trace!(
                "got interaction `{:?}` from `{} ({}#{})`",
                &command.data.name,
                &command.user.name,
                &command.user.id,
                &command.user.discriminator
            );
            match command.data.name.as_str() {
                "help" => {
                    command
                        .create_interaction_response(&ctx.http, |response| {
                            response.interaction_response_data(|data| {
                                data.embed(|e| {
                                    e.title("Verusbot help")
                                        .field("1", "talk about 1", false)
                                        .field("2", "**talk about 2**", false)
                                        .field("3", "_talk about 3_", false)
                                        .field("4", "~~talk about 4~~", false)
                                        .field("5", "`talk about 5`", false)
                                        .thumbnail("https://media.tenor.com/dzvvois22BoAAAAi/verus-vrsc.gif")
                                });
                                data.ephemeral(true)
                            })
                        })
                        .await
                        .expect("a response to a /help interaction");
                }
                _ => {}
            };
        }
    }
}
