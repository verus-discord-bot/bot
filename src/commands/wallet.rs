use std::{cmp::Ordering, fmt};

use qrcode::{render::unicode, QrCode};
use tracing::*;
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::{Client, RpcApi};

use crate::{util::database, Context, Error};

/// Withdraw funds from the tipbot wallet. You can use R*, i* or an existing identity (ends with @).
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn withdraw(
    ctx: Context<'_>,
    withdrawal_amount: f64,
    destination: String,
) -> Result<(), Error> {
    debug!(
        "user {} ({}) demands a withdrawal of {withdrawal_amount}",
        ctx.author().name,
        ctx.author().id
    );

    let client = &ctx.data().verus;
    if !destination_is_valid(&destination, &client) {
        ctx.send(|reply| {
            reply.ephemeral(true).content(format!(
                "Error: The destination you entered cannot be used: {destination}"
            ))
        })
        .await?;

        return Ok(());
    }

    let withdrawal_amount = Amount::from_float_in(withdrawal_amount, vrsc::Denomination::Verus)?;

    // cannot withdraw if amount to withdraw <= 0.0
    if [Ordering::Less, Ordering::Equal].contains(&withdrawal_amount.cmp(&Amount::ZERO)) {
        ctx.send(|reply| {
            reply
                .ephemeral(true)
                .content("Error: Withdrawal amount should be more than 0.0")
        })
        .await?;

        return Ok(());
    }

    let pool = &ctx.data().database;

    if let Some(balance) = database::get_balance_for_user(&pool, ctx.author().id).await? {
        let balance_amount = Amount::from_sat(balance);
        if balance_is_enough(&balance_amount, &withdrawal_amount) {
            // balance is sufficient.
        }

        // let withdrawal_amount:
        trace!("there is a balance for this user: {:?}", &balance_amount);
    } else {
        trace!("there is no balance for this user");

        ctx.send(|reply| reply.ephemeral(true).content("Your balance is: 0"))
            .await?;
    }
    // need to do some checks:
    // - does the user have enough balance?
    // - is the user withdrawing more than 0 verus sats?
    // - is the withdrawal address a valid address/
    // - is the withdrawal address a z_address?
    // - is the withdrawal address an identity?
    //
    // need to build support for the sendcurrency RPC

    Ok(())
}

fn destination_is_valid(dest: &str, client: &Client) -> bool {
    false
    // Let's do some address parsing
    // - is the withdrawal address a valid address/
    // - is the withdrawal address a z_address?
    // - is the withdrawal address an identity?
    // - is the withdrawal address a i-address?
}

fn balance_is_enough(balance: &Amount, amount_to_withdraw: &Amount) -> bool {
    if let Some(positive_result) = balance.checked_sub(*amount_to_withdraw) {
        debug!("{positive_result}");
        return true;
    }

    false
}

#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn balance(ctx: Context<'_>) -> Result<(), Error> {
    debug!(
        "user {} ({}) demands a balance",
        ctx.author().name,
        ctx.author().id
    );
    let pool = &ctx.data().database;

    if let Some(balance) = database::get_balance_for_user(&pool, ctx.author().id).await? {
        let balance_amount = Amount::from_sat(balance);

        trace!(
            "there is a balance for this user, return it: {:?}",
            &balance_amount
        );

        ctx.send(|reply| {
            reply
                .ephemeral(true)
                .content(format!("Your balance is: {}", balance_amount))
        })
        .await?;
    } else {
        trace!("there is no balance for this user");

        ctx.send(|reply| reply.ephemeral(true).content("Your balance is: 0"))
            .await?;
    }

    Ok(())
}

#[derive(Debug)]
struct MyError(String);

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "There is an error: {}", self.0)
    }
}

impl std::error::Error for MyError {}

/// Deposit funds to the tipbot wallet
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn deposit(ctx: Context<'_>) -> Result<(), Error> {
    debug!(
        "user {} ({}) demands a deposit address",
        ctx.author().name,
        ctx.author().id
    );
    let pool = &ctx.data().database;
    let client = &ctx.data().verus;

    if let Some(address) = database::get_address_from_user(&pool, ctx.author().id).await? {
        debug!("address already stored, return it");
        send_address_message(ctx, address).await?;
    } else {
        // for some reason, an error is returned from the client: HttpResponseTooShort (got 0, expected 12)
        // todo for now, redo the get_new_address RPC until we get an address.
        let address;
        let mut i = 0;
        loop {
            match client.get_new_address() {
                Ok(new_address) => {
                    address = new_address;
                    break;
                }
                Err(_e) => {
                    warn!("didn't get address, trying again");
                    if i < 100 {
                        i += 1;
                        continue;
                    } else {
                        error!("could not get an address");
                        return Err(MyError("Could not get a new Verus address".to_string()).into());
                    }
                }
            }
        }
        // simultaneously add row to both `discord_users` and `balance_vrsc` with an initial balance of 0.
        if database::store_new_address_for_user(&pool, ctx.author().id, &address)
            .await
            .is_ok()
        {
            send_address_message(ctx, address).await?;
        }
    }

    Ok(())
}

async fn send_address_message(ctx: Context<'_>, address: Address) -> Result<(), Error> {
    ctx.send(|reply| {
        let qr = QrCode::new(&address.to_string()).unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sufficient_balance() {
        let balance = Amount::from_sat(1000);
        let to_withdraw = Amount::from_sat(500);

        assert!(balance_is_enough(&balance, &to_withdraw));

        let balance = Amount::from_sat(99);
        let to_withdraw = Amount::from_sat(99);

        assert!(balance_is_enough(&balance, &to_withdraw));
    }

    #[test]
    fn insufficient_balance() {
        let balance = Amount::from_sat(1000);
        let to_withdraw = Amount::from_sat(1001);

        assert!(!balance_is_enough(&balance, &to_withdraw));
    }

    #[test]
    fn edge_cases() {
        let balance = Amount::max_value();
        let to_withdraw = Amount::max_value();

        assert!(balance_is_enough(&balance, &to_withdraw));

        let balance = Amount::max_value();
        let to_withdraw = Amount::min_value();

        assert!(balance_is_enough(&balance, &to_withdraw));
    }
}
