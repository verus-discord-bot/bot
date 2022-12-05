// use futures::{Stream, StreamExt};

use serde_json::to_string;

use crate::{Context, Error};
use std::string::ToString;

// pub async fn autocomplete_chain<'a>(
//     _ctx: Context<'_>,
//     partial: &'a str,
// ) -> impl Stream<Item = String> + 'a {
//     futures::stream::iter(&["Amanda", "Bob", "Christian", "Danny", "Ester", "Falk"])
//         .filter(move |name| futures::future::ready(name.starts_with(partial)))
//         .map(|name| name.to_string())
// }

pub async fn autocomplete_chain<'a>(
    _ctx: Context<'a>,
    partial: &'a str,
) -> impl Iterator<Item = String> + 'a {
    // ctx.framework()
    //     .options()
    //     .commands
    //     .iter()
    //     .filter(move |cmd| cmd.name.starts_with(partial))
    //     .map(|cmd| cmd.name.to_string())

    vec!["andromeda", "gravity"]
        .into_iter()
        .filter(move |name| name.starts_with(partial))
        .map(|cmd| cmd.to_string())
}

// pub async fn autocomplete_command<'a>(
//     ctx: Context<'a>,
//     partial: &'a str,
// ) -> impl Iterator<Item = String> + 'a {
//     ctx.framework()
//         .options()
//         .commands
//         .iter()
//         .filter(move |cmd| cmd.name.starts_with(partial))
//         .map(|cmd| cmd.name.to_string())
