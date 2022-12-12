use std::{cmp::Ordering, fmt, str::FromStr, time::Duration};

use qrcode::{render::unicode, QrCode};
use tracing::*;
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::{bitcoin::Txid, Client, RpcApi, SendCurrencyOutput};

use crate::{util::database, Context, Error};

/// Withdraw funds from the tipbot wallet. You can use R*, i* or an existing identity (ends with @).
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn withdraw(
    ctx: Context<'_>,
    #[description = "The amount you want to withdraw"] withdrawal_amount: f64,
    #[description = "You can use any address starting with R* or i*, or use an existing identity (ends with @)."]
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
            reply.ephemeral(false).content(format!(
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
                .ephemeral(false)
                .content("Error: Withdrawal amount should be more than 0.0")
        })
        .await?;

        return Ok(());
    }

    let pool = &ctx.data().database;
    let uuid = Uuid::new_v4();
    let tx_fee = &ctx.data().settings.application.global_withdrawal_fee;

    let balance = database::get_balance_for_user(&pool, ctx.author().id).await?;
    let balance_amount = Amount::from_sat(balance);
    if balance_is_enough(&balance_amount, &withdrawal_amount, &tx_fee) {
        // at this point:
        // - balance is sufficient (also to pay withdrawal_fee).
        // - address is valid.
        trace!("sendcurrency");

        let sco = SendCurrencyOutput::new("vrsctest".to_string(), withdrawal_amount, destination);
        let opid = client.send_currency("*", vec![sco], None, None)?;
        debug!("opid: {:?}", opid);

        if let Some(txid) = wait_for_sendcurrency_finish(&client, &opid).await? {
            // at this point the txid is known. Now blockchain shenanigans could be happening, so we should store everything in the transactions_db table
            database::store_withdraw_transaction(
                &pool,
                &uuid,
                &ctx.author().id,
                &txid,
                &opid,
                &tx_fee,
            )
            .await?;
            database::decrease_balance(&pool, &ctx.author().id, &withdrawal_amount, &tx_fee)
                .await?;

            let new_balance = database::get_balance_for_user(&pool, ctx.author().id).await?;

            ctx.send(|reply| {
                // reply.ephemeral(true).content(format!(
                //     "Withdrawal initiated. [Explorer](https://testex.verus.io/tx/{})",
                //     txid.to_string()
                // ))
                reply.ephemeral(false).embed(|embed| {
                    let embed = embed
                        .title("Withdraw")
                        .field("Amount", withdrawal_amount, false)
                        .field("Fees", tx_fee, false)
                        .field(
                            "Explorer",
                            format!("[link](https://testex.verus.io/tx/{})", txid.to_string()),
                            false,
                        );

                    embed.field("New balance", Amount::from_sat(new_balance), false);

                    embed
                })
            })
            .await?;
        } else {
            // at this point, the sendcurrency didn't finish. Maybe it went through, but we don't know.
            // We should check this out manually, so we'll let the user know to contact support.

            // maybe deposit and withdraw should be separated, where we store more information in the withdraw table.
            // like the operation result with its status etc, and maybe a newly created uuid so we can easily get it from the database.

            let response = format!("Something went wrong trying to process your withdrawal. Please contact support with withdrawal ID: {}",
                uuid.to_string());

            ctx.send(|reply| reply.ephemeral(true).content(&response))
                .await?;
        }
        return Ok(());
    }

    ctx.send(|reply| {
        reply.ephemeral(false).content(format!(
            "Your balance is insufficient to withdraw {withdrawal_amount}"
        ))
    })
    .await?;
    // else the balance is not enough
    // } else the balance does not exist so no cannot withdraw.

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

async fn wait_for_sendcurrency_finish(client: &Client, opid: &str) -> Result<Option<Txid>, Error> {
    // first we need to get operation status to work:
    loop {
        let operation_status = client.z_get_operation_status(vec![&opid])?;

        if let Some(Some(opstatus)) = operation_status.first() {
            if let Some(txid) = &opstatus.result {
                trace!("there was an operation_status");

                return Ok(Some(txid.txid));
            } else {
                // we need to wait for the execution of the sendcurrency to finish.
                trace!("execution hasn't finished yet");

                tokio::time::sleep(Duration::from_millis(77)).await;

                continue;
            }
        } else {
            trace!("there was NO operation_status");
            continue;
        }
    }
}

// Let's do some address parsing
// - is the withdrawal address a valid address?
// (- is the withdrawal address a z_address?)
// - is the withdrawal address an identity?
// - is the withdrawal address a i-address?
fn destination_is_valid(dest: &str, client: &Client) -> bool {
    if Address::from_str(dest).is_ok() {
        // this parses both R* addresses and i* addresses
        // (maybe z-addresses?)
        return true;
    } else {
        debug!("dest: {}", dest);
        // it could be an identity
        if client.get_identity(dest).is_ok() {
            // this is a valid identity, let's use it.
            return true;
        }
    }

    // in all other cases it's invalid.
    false
}

// This function checks if the user has sufficient balance to withdraw and to pay the fees.
fn balance_is_enough(balance: &Amount, amount_to_withdraw: &Amount, tx_fee: &Amount) -> bool {
    debug!("balance: {balance}, amount: {amount_to_withdraw}, tx_fee: {tx_fee}");
    if let Some(total_amount) = amount_to_withdraw.checked_add(*tx_fee) {
        if let Some(positive_result) = balance.checked_sub(total_amount) {
            debug!("{positive_result}");
            return true;
        }
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

    let balance = database::get_balance_for_user(&pool, ctx.author().id).await?;
    let balance_amount = Amount::from_sat(balance);

    trace!(
        "there is a balance for this user, return it: {:?}",
        &balance_amount
    );

    ctx.send(|reply| {
        reply
            .ephemeral(false)
            .content(format!("Your balance is: {}", balance_amount))
    })
    .await?;

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

        reply.ephemeral(false).embed(|embed| {
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
        let balance = Amount::from_sat(51000);
        let to_withdraw = Amount::from_sat(500);
        let tx_fee = Amount::from_sat(50000);

        assert!(balance_is_enough(&balance, &to_withdraw, &tx_fee));

        let balance = Amount::from_sat(50099);
        let to_withdraw = Amount::from_sat(99);

        let tx_fee = Amount::from_sat(50000);

        assert!(balance_is_enough(&balance, &to_withdraw, &tx_fee));
    }

    #[test]
    fn insufficient_balance() {
        let balance = Amount::from_sat(51000);
        let to_withdraw = Amount::from_sat(1001);
        let tx_fee = Amount::from_sat(50000);

        assert!(!balance_is_enough(&balance, &to_withdraw, &tx_fee));
    }

    #[test]
    fn edge_cases() {
        let balance = Amount::max_value();
        let to_withdraw = Amount::max_value();

        let tx_fee = Amount::from_sat(0);

        assert!(balance_is_enough(&balance, &to_withdraw, &tx_fee));
        let balance = Amount::max_value();
        let to_withdraw = Amount::min_value();

        let tx_fee = Amount::from_sat(0);

        assert!(balance_is_enough(&balance, &to_withdraw, &tx_fee));
    }
}
