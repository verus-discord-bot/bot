use std::{cmp::Ordering, fmt, ops::Sub, str::FromStr, time::Duration};

use qrcode::{render::unicode, QrCode};
use tracing::*;
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::{bitcoin::Txid, Client, RpcApi, SendCurrencyOutput};

use crate::{util::database, Context, Error};

/// Withdraw funds from the tipbot wallet.
#[instrument(skip(_ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet", subcommands("amount", "all"))]
pub async fn withdraw(
    _ctx: Context<'_>,
    #[description = "The amount you want to tip"] withdrawal_amount: f64,
    #[description = "You can use any address starting with R* or i*, or use an existing identity (ends with @)."]
    destination: String,
) -> Result<(), Error> {
    Ok(())
}

/// Withdraw a given amount from the tipbot wallet.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn all(
    ctx: Context<'_>,
    // #[description = "The amount you want to tip"] withdrawal_amount: f64,
    #[description = "You can use any address starting with R* or i*, or use an existing identity (ends with @)."]
    destination: String,
) -> Result<(), Error> {
    debug!(
        "user {} ({}) demands a withdrawal of his whole balance",
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

    let pool = &ctx.data().database;
    let uuid = Uuid::new_v4();
    let tx_fee = &ctx.data().withdrawal_fee.read().await.clone();

    // the user ALWAYS has balance because of the pre_command function.
    if let Some(balance) = database::get_balance_for_user(&pool, &ctx.author().id).await? {
        let balance_amount = Amount::from_sat(balance);
        let withdrawal_amount = balance_amount.sub(*tx_fee); // no need to check for underflow, tx_fee is always low.

        if withdrawal_amount > Amount::ZERO {
            debug!("withdrawal_amount: {withdrawal_amount}, tx_fee: {tx_fee} must together be balance_amount: {balance_amount}");

            let currency = match ctx.data().settings.application.testnet {
                true => Some("vrsctest"),
                false => None,
            };
            let sco = SendCurrencyOutput::new(currency, &withdrawal_amount, &destination);
            let opid = client.send_currency("*", vec![sco], None, None)?;
            debug!("sendcurrency opid: {:?}", &opid);

            if let Some(txid) = wait_for_sendcurrency_finish(&client, &opid).await? {
                // at this point the txid is known. Now blockchain shenanigans could be happening, so we should store everything in the transactions_db table
                database::store_withdraw_transaction(
                    &pool,
                    &uuid,
                    &ctx.author().id,
                    Some(&txid),
                    &opid,
                    &tx_fee,
                )
                .await?;

                trace!("transaction stored, now decrease balance");
                database::decrease_balance(&pool, &ctx.author().id, &withdrawal_amount, &tx_fee)
                    .await?;

                let new_balance = database::get_balance_for_user(&pool, &ctx.author().id).await?;

                ctx.send(|reply| {
                    reply.ephemeral(false).embed(|embed| {
                        let embed = embed
                            .title("Withdraw")
                            .field("Amount", withdrawal_amount, false)
                            .field("Fees", tx_fee, false)
                            .field(
                                "Explorer",
                                format!("[link](https://insight.verus.io/tx/{})", txid.to_string()),
                                false,
                            );

                        if let Some(new_balance) = new_balance {
                            embed.field("New balance", Amount::from_sat(new_balance), false);
                        }

                        embed
                    })
                })
                .await?;
            } else {
                // at this point, the sendcurrency didn't finish. Maybe it went through, but we don't know.
                // We should check this manually, so we'll let the user know to contact support and we'll store the op-id in the database.
                let response = format!("Something went wrong trying to process your withdrawal. Please contact support with withdrawal ID: {}",
                uuid.to_string());

                database::store_withdraw_transaction(
                    &pool,
                    &uuid,
                    &ctx.author().id,
                    None,
                    &opid,
                    &tx_fee,
                )
                .await?;

                ctx.send(|reply| reply.ephemeral(true).content(&response))
                    .await?;
            }

            return Ok(());
        }

        ctx.send(|reply| {
            reply.ephemeral(false).content(format!(
                "Your balance is insufficient to withdraw everything.\nMax available balance for withdraw: {}", withdrawal_amount.checked_sub(*tx_fee).unwrap_or(Amount::ZERO)
            ))
        })
        .await?;
    }
    // the user ALWAYS has balance because of the pre_command function.
    // So this is an unreachable place, theoretically.
    error!("User should have had balance at this point, something is wrong");

    Ok(())
}

/// Withdraw a given amount from the tipbot wallet.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn amount(
    ctx: Context<'_>,
    #[description = "The amount you want to tip"] withdrawal_amount: f64,
    #[description = "You can use any address starting with R* or i*, or use an existing identity (ends with @)."]
    destination: String,
) -> Result<(), Error> {
    debug!(
        "user {} ({}) demands a withdrawal of {withdrawal_amount}",
        ctx.author().name,
        ctx.author().id
    );

    let withdrawal_amount = Amount::from_float_in(withdrawal_amount, vrsc::Denomination::Verus)?;

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

    // if amount to withdraw <= 0.0
    // the reason this has to be done this way is because Amount is an abstraction over floats (f64) and 2 floats with the same value are not equal
    // according to some IEEE standard.
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

    let tx_fee = &ctx.data().withdrawal_fee.read().await.clone();
    if let Some(balance) = database::get_balance_for_user(&pool, &ctx.author().id).await? {
        let balance_amount = Amount::from_sat(balance);

        // gets the withdrawal fee and clones it to prevent deadlock
        debug!("tx_fee: {tx_fee}");

        if balance_is_enough(&balance_amount, &withdrawal_amount, &tx_fee) {
            trace!("balance is sufficient, withdrawal address is valid; starting sendcurrency");

            let currency = match ctx.data().settings.application.testnet {
                true => Some("vrsctest"),
                false => None,
            };
            let sco = SendCurrencyOutput::new(currency, &withdrawal_amount, &destination);
            let opid = client.send_currency("*", vec![sco], None, None)?;
            debug!("sendcurrency opid: {:?}", &opid);

            if let Some(txid) = wait_for_sendcurrency_finish(&client, &opid).await? {
                // at this point the txid is known. Now blockchain shenanigans could be happening, so we should store everything in the transactions_db table
                database::store_withdraw_transaction(
                    &pool,
                    &uuid,
                    &ctx.author().id,
                    Some(&txid),
                    &opid,
                    &tx_fee,
                )
                .await?;

                trace!("transaction stored, now decrease balance");
                database::decrease_balance(&pool, &ctx.author().id, &withdrawal_amount, &tx_fee)
                    .await?;

                let new_balance = database::get_balance_for_user(&pool, &ctx.author().id).await?;

                ctx.send(|reply| {
                    reply.ephemeral(false).embed(|embed| {
                        let embed = embed
                            .title("Withdraw")
                            .field("Amount", withdrawal_amount, false)
                            .field("Fees", tx_fee, false)
                            .field(
                                "Explorer",
                                format!("[link](https://insight.verus.io/tx/{})", txid.to_string()),
                                false,
                            );

                        if let Some(new_balance) = new_balance {
                            embed.field("New balance", Amount::from_sat(new_balance), false);
                        }

                        embed
                    })
                })
                .await?;
            } else {
                // at this point, the sendcurrency didn't finish. Maybe it went through, but we don't know.
                // We should check this manually, so we'll let the user know to contact support and we'll store the op-id in the database.
                let response = format!("Something went wrong trying to process your withdrawal. Please contact support with withdrawal ID: {}",
                uuid.to_string());

                database::store_withdraw_transaction(
                    &pool,
                    &uuid,
                    &ctx.author().id,
                    None,
                    &opid,
                    &tx_fee,
                )
                .await?;

                ctx.send(|reply| reply.ephemeral(true).content(&response))
                    .await?;
            }

            return Ok(());
        }
    }

    ctx.send(|reply| {
        reply.ephemeral(false).content(format!(
            "Your balance is insufficient to withdraw {withdrawal_amount}.\nMax available balance for withdraw: {}", withdrawal_amount.checked_sub(*tx_fee).unwrap_or(Amount::ZERO)
        ))
    })
    .await?;

    Ok(())
}

#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn balance(ctx: Context<'_>) -> Result<(), Error> {
    debug!(
        "user {} ({}) demands a balance",
        ctx.author().name,
        ctx.author().id
    );

    if let Some(balance) = check_and_get_balance(&ctx, Amount::ZERO).await? {
        ctx.send(|reply| {
            reply
                .ephemeral(false)
                .content(format!("Your balance is: {}", balance))
        })
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

    if let Some(address) = database::get_address_from_user(&pool, &ctx.author().id).await? {
        ctx.send(|reply| {
            let qr = QrCode::new(&address.to_string()).unwrap();
            let image_str = qr
                .render::<unicode::Dense1x2>()
                .module_dimensions(1, 1)
                .build();

            reply.ephemeral(false).embed(|embed| {
                embed
                    // .title("Deposit")
                    .field("Address", format!("{}", &address), false)
                    .field(
                        "Scan this QR with the Verus Mobile app",
                        format!("```{image_str}```"),
                        false,
                    )
            })
        })
        .await?;
    }

    Ok(())
}

// Sendcurrency works with op-ids because it can work with zk-transactions. Therefore the txid of a transactions is not always known directly after sending.
// This function waits a bit and gets the txid once the operation_status RPC gives one.
// if it doesn't give one, the user is notified and the op-id is stored in the database.
async fn wait_for_sendcurrency_finish(client: &Client, opid: &str) -> Result<Option<Txid>, Error> {
    let mut i = 0;
    loop {
        trace!("getting operation status: {}", &opid);
        let operation_status = client.z_get_operation_status(vec![&opid])?;
        trace!("got operation status: {:?}", &operation_status);

        if let Some(Some(opstatus)) = operation_status.first() {
            if let Some(txid) = &opstatus.result {
                trace!("there was an operation_status");

                return Ok(Some(txid.txid));
            } else {
                // we need to wait for the execution of the sendcurrency to finish.
                trace!("execution hasn't finished yet");

                tokio::time::sleep(Duration::from_millis(77)).await;
            }
        } else {
            trace!("there was NO operation_status");
        }
        if i > 100 {
            return Ok(None);
        }
        i += 1;
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
pub fn balance_is_enough(balance: &Amount, amount_to_withdraw: &Amount, tx_fee: &Amount) -> bool {
    debug!("balance: {balance}, amount: {amount_to_withdraw}, tx_fee: {tx_fee}");
    if let Some(total_amount) = amount_to_withdraw.checked_add(*tx_fee) {
        if let Some(positive_result) = balance.checked_sub(total_amount) {
            debug!("{positive_result}");

            return true;
        }
    }

    false
}

// In this context, get the balance of the sending user and return it.
pub async fn check_and_get_balance(
    ctx: &Context<'_>,
    amount_to_check: Amount,
) -> Result<Option<Amount>, Error> {
    let pool = &ctx.data().database;

    if let Some(balance) = database::get_balance_for_user(&pool, &ctx.author().id).await? {
        trace!("tipper has balance");

        if balance_is_enough(
            &Amount::from_sat(balance),
            &amount_to_check,
            &Amount::ZERO, // no fees for tipping
        ) {
            trace!("tipper has sufficient balance");
            return Ok(Some(Amount::from_sat(balance)));
        } else {
            trace!("balance is insufficient");
            ctx.send(|reply| {
                reply
                    .ephemeral(false)
                    .content(format!("Your balance is insufficient to tip that amount!"))
            })
            .await?;

            return Ok(None);
        }
    } else {
        trace!("tipper has no balance");
        warn!("user {} should have a balance!", ctx.author());

        ctx.send(|reply| {
            reply
                .ephemeral(false)
                .content(format!("Your balance is insufficient to tip that amount!"))
        })
        .await?;

        return Ok(None);
    }
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
