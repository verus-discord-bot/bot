use std::{ops::Sub, str::FromStr, time::Duration};

use fast_qr::convert::{Builder, Shape, image::ImageBuilder};
use fast_qr::qr::QRBuilder;
use poise::CreateReply;
use poise::serenity_prelude::{CreateAttachment, CreateEmbed};
use sqlx::{Postgres, Transaction};
use tracing::*;
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::{
    bitcoin::Txid,
    client::{Client, RpcApi, SendCurrencyOutput},
};

use crate::commands::user_blacklisted;
use crate::{Context, Error, VRSC_CURRENCY_ID, database};

/// Withdraw funds from the tipbot wallet.
///
/// -------- :robot: **Withdraw an amount** --------
/// Withdraws the amount you enter to an address or VerusID that you specify. Valid withdrawal addresses are:
/// - an address that starts with R* or i*
/// - an existing VerusID (ends with an `@`)
///
/// A withdrawal fee will be subtracted from your remaining balance.
/// You will encounter an error when the amount you want to withdraw is more than (your balance - withdrawal fee).
///
/// -------- :robot: **Withdraw all** --------
/// Zero out your balance by withdrawing everything to an address or VerusID that you specify. Valid withdrawal addresses are:
/// - an address that starts with R* or i*
/// - an existing VerusID (ends with an `@`)
///
/// A withdrawal fee will be subtracted from the total balance before withdrawal.
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

/// Withdraw everything from the tipbot wallet
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn all(
    ctx: Context<'_>,
    #[description = "You can use any address starting with R* or i*, or use an existing VerusID (ends with @)."]
    destination: String,
) -> Result<(), Error> {
    if *ctx.data().withdrawals_enabled.read().await == false {
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content(format!("Withdrawals are temporarily disabled.")),
        )
        .await?;

        return Ok(());
    }

    if user_blacklisted(ctx, ctx.author().id).await? {
        return Ok(());
    }

    let client = &ctx.data().verus()?;
    if !destination_is_valid(&destination, &client) {
        ctx.send(CreateReply::default().ephemeral(true).content(format!(
            "Error: The destination you entered cannot be used: {destination}"
        )))
        .await?;

        return Ok(());
    }

    debug!(
        name = %ctx.author().name,
        user_id = %ctx.author().id,
        "user demands a withdrawal of his full balance",
    );

    let mut tx = ctx.data().database.begin().await?;
    let uuid = Uuid::new_v4();
    let tx_fee = &ctx.data().withdrawal_fee.read().await.clone();

    if let Some(balance) = database::get_balance_for_user(
        &mut *tx,
        &ctx.author().id,
        &Address::from_str(VRSC_CURRENCY_ID)?,
    )
    .await?
    {
        let balance_amount = Amount::from_sat(balance);
        let withdrawal_amount = balance_amount.sub(*tx_fee); // no need to check for underflow, tx_fee is always low.

        if withdrawal_amount > Amount::ZERO {
            debug!(
                "withdrawal_amount: {withdrawal_amount}, tx_fee: {tx_fee} must together be balance_amount: {balance_amount}"
            );

            let sco = SendCurrencyOutput::new(None, &withdrawal_amount, &destination, None, None);
            let opid = client.send_currency("*", vec![sco], None, None)?;

            debug!("sendcurrency opid: {:?}", &opid);

            if let Some(txid) = wait_for_sendcurrency_finish(&mut tx, &client, &opid).await? {
                // at this point the txid is known. Now blockchain shenanigans could be happening, so we should store everything in the transactions_db table
                database::store_withdraw_transaction(
                    &mut *tx,
                    &uuid,
                    &ctx.author().id,
                    Some(&txid),
                    &opid,
                    &tx_fee,
                    &Address::from_str(VRSC_CURRENCY_ID)?,
                )
                .await?;

                trace!(
                    "transaction {txid} stored in db, now decrease balance with ({withdrawal_amount} + {tx_fee})"
                );
                database::decrease_balance(
                    &mut *tx,
                    &ctx.author().id,
                    &withdrawal_amount,
                    &tx_fee,
                    &Address::from_str(VRSC_CURRENCY_ID)?,
                )
                .await?;

                let new_balance = database::get_balance_for_user(
                    &mut *tx,
                    &ctx.author().id,
                    &Address::from_str(VRSC_CURRENCY_ID)?,
                )
                .await?;

                tx.commit().await?;

                ctx.send(CreateReply::default().ephemeral(true).embed({
                    let mut embed = CreateEmbed::new()
                        .title("Withdraw")
                        .field("Amount", withdrawal_amount.to_string(), false)
                        .field("Fees", tx_fee.to_string(), false)
                        .field(
                            "Explorer",
                            format!("[link](https://insight.verus.io/tx/{})", txid.to_string()),
                            false,
                        );

                    if let Some(new_balance) = new_balance {
                        embed = embed.field(
                            "New balance",
                            Amount::from_sat(new_balance).to_string(),
                            false,
                        );
                    }

                    embed
                }))
                .await?;
            } else {
                // at this point, the sendcurrency didn't finish. Maybe it went through, but we
                // don't know.
                // We should check this manually, so we'll let the user know to contact support
                // and we'll store the op-id in the database.
                let response = format!(
                    "Something went wrong trying to process your withdrawal. \
                Please contact support with withdrawal ID: {}",
                    uuid.to_string()
                );

                database::store_withdraw_transaction(
                    &mut *tx,
                    &uuid,
                    &ctx.author().id,
                    None,
                    &opid,
                    &tx_fee,
                    &Address::from_str(VRSC_CURRENCY_ID)?,
                )
                .await?;

                tx.commit().await?;
                ctx.send(CreateReply::default().ephemeral(true).content(&response))
                    .await?;
            }

            return Ok(());
        } else {
            ctx.send(CreateReply::default().ephemeral(true).content(format!(
                "Your balance is insufficient to withdraw everything.\nMax available balance for \
                withdraw: {}",
                withdrawal_amount.checked_sub(*tx_fee).unwrap_or(Amount::ZERO)
            )))
            .await?;
        }
    } else {
        trace!("The user has no balance, abort");
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content(format!("Your balance is insufficient to withdraw")),
        )
        .await?;
    }

    Ok(())
}

/// Withdraw an amount from the tipbot wallet.
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn amount(
    ctx: Context<'_>,
    #[description = "The amount you want to tip"] withdrawal_amount: f64,
    #[description = "You can use any address starting with R* or i*, or use an existing \
    identity (ends with @)."]
    destination: String,
) -> Result<(), Error> {
    if *ctx.data().withdrawals_enabled.read().await == false {
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content(format!("Withdrawals are temporarily disabled.")),
        )
        .await?;

        return Ok(());
    }

    if user_blacklisted(ctx, ctx.author().id).await? {
        return Ok(());
    }

    if !withdrawal_amount.is_sign_positive() || !withdrawal_amount.is_normal() {
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content("Error: Withdrawal amount should be more than 0.0"),
        )
        .await?;

        return Ok(());
    }

    debug!(
        "user {} ({}) demands a withdrawal of {withdrawal_amount}",
        ctx.author().name,
        ctx.author().id
    );

    let client = &ctx.data().verus()?;
    if !destination_is_valid(&destination, &client) {
        ctx.send(CreateReply::default().ephemeral(true).content(format!(
            "Error: The destination you entered cannot be used: {destination}"
        )))
        .await?;

        return Ok(());
    }

    let withdrawal_amount = Amount::from_vrsc(withdrawal_amount)?;

    let mut tx = ctx.data().database.begin().await?;
    let uuid = Uuid::new_v4();
    let tx_fee = ctx.data().withdrawal_fee.read().await.clone();

    // can we let the database return something meaningful when the withdraw is not possible?
    if get_and_check_balance(&ctx, withdrawal_amount, tx_fee)
        .await?
        .is_some()
    {
        trace!("balance is sufficient, withdrawal address is valid; starting sendcurrency");

        let sco = SendCurrencyOutput::new(None, &withdrawal_amount, &destination, None, None);
        let opid = client.send_currency("*", vec![sco], None, None)?;

        debug!("sendcurrency opid: {:?}", &opid);

        if let Some(txid) = wait_for_sendcurrency_finish(&mut tx, &client, &opid).await? {
            // at this point the txid is known. Now blockchain shenanigans could be happening,
            // so we should store everything in the transactions_db table
            database::store_withdraw_transaction(
                &mut *tx,
                &uuid,
                &ctx.author().id,
                Some(&txid),
                &opid,
                &tx_fee,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;

            trace!("transaction stored, now decrease balance");
            database::decrease_balance(
                &mut *tx,
                &ctx.author().id,
                &withdrawal_amount,
                &tx_fee,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;

            let new_balance = database::get_balance_for_user(
                &mut *tx,
                &ctx.author().id,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;

            ctx.send(CreateReply::default().ephemeral(true).embed({
                let mut embed = CreateEmbed::new()
                    .title("Withdraw")
                    .field("Amount", withdrawal_amount.to_string(), false)
                    .field("Fees", tx_fee.to_string(), false)
                    .field(
                        "Explorer",
                        format!("[link](https://insight.verus.io/tx/{})", txid.to_string()),
                        false,
                    );

                if let Some(new_balance) = new_balance {
                    embed = embed.field(
                        "New balance",
                        Amount::from_sat(new_balance).to_string(),
                        false,
                    );
                }

                embed
            }))
            .await?;
        } else {
            // at this point, the sendcurrency didn't finish. Maybe it went through, but we don't know.
            // We should check this manually, so we'll let the user know to contact support and we'll store the op-id in the database.
            let response = format!(
                "Something went wrong trying to process your withdrawal.
                Please contact support with withdrawal ID: {}",
                uuid.to_string()
            );

            database::store_withdraw_transaction(
                &mut *tx,
                &uuid,
                &ctx.author().id,
                None,
                &opid,
                &tx_fee,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;

            ctx.send(CreateReply::default().ephemeral(true).content(&response))
                .await?;
        }

        tx.commit().await?;

        return Ok(());
    }

    ctx.send(CreateReply::default().ephemeral(true).content(format!(
            "Your balance is insufficient to withdraw {withdrawal_amount}.\n
            Max available balance for withdraw: {}",
            withdrawal_amount
                .checked_sub(tx_fee)
                .unwrap_or(Amount::ZERO)
        )))
    .await?;

    Ok(())
}

/// Donate the given amount to the `Verus Coin Foundation@` VerusID.
///
/// Donate some VRSC to the Verus Coin Foundation@ VerusID.
/// This will be an on-chain transaction, but no withdrawal fees are incurred.
#[instrument(err, skip(ctx))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn donate_to_foundation(ctx: Context<'_>, amount: f64) -> Result<(), Error> {
    if !(*ctx.data().withdrawals_enabled.read().await) {
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content("Withdrawals are temporarily disabled.".to_string()),
        )
        .await?;

        return Ok(());
    }

    if !amount.is_sign_positive() || !amount.is_normal() {
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content("Error: Withdrawal amount should be more than 0.0"),
        )
        .await?;

        return Ok(());
    }

    let withdrawal_amount = Amount::from_vrsc(amount)?;

    if withdrawal_amount == Amount::ZERO {
        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content("Error: Withdrawal amount including fees should be more than 0.0"),
        )
        .await?;

        return Ok(());
    }

    let mut tx = ctx.data().database.begin().await?;
    let destination = "Verus Coin Foundation@";

    if get_and_check_balance(&ctx, withdrawal_amount, Amount::ZERO)
        .await?
        .is_some()
    {
        trace!("balance is sufficient, withdrawal address is valid; starting sendcurrency");

        let client = &ctx.data().verus()?;

        let sco = SendCurrencyOutput::new(None, &withdrawal_amount, destination, None, None);
        let opid = client.send_currency("*", vec![sco], None, None)?;

        debug!("sendcurrency opid: {:?}", &opid);

        if let Some(txid) = wait_for_sendcurrency_finish(&mut tx, client, &opid).await? {
            // at this point the txid is known. Now blockchain shenanigans could be happening,
            // so we should store everything in the transactions_db table
            database::store_withdraw_transaction(
                &mut tx,
                &Uuid::new_v4(),
                &ctx.author().id,
                Some(&txid),
                &opid,
                &Amount::ZERO,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;

            database::decrease_balance(
                &mut tx,
                &ctx.author().id,
                &withdrawal_amount,
                &Amount::ZERO,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;

            tx.commit().await?;

            ctx.send(CreateReply::default().content(format!(
                "<@{}> donated {} VRSC to the Verus Coin Foundation!",
                ctx.author().id,
                withdrawal_amount.as_vrsc()
            )))
            .await?;
        } else {
            // at this point, the sendcurrency didn't finish. Maybe it went through, but we don't know.
            // We should check this manually, so we'll let the user know to contact support and we'll store the op-id in the database.
            let uuid = Uuid::new_v4();
            let response = format!(
                "Something went wrong trying to process your withdrawal.
                Please contact support with withdrawal ID: {uuid}"
            );

            database::store_withdraw_transaction(
                &mut tx,
                &uuid,
                &ctx.author().id,
                None,
                &opid,
                &Amount::ZERO,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;

            tx.commit().await?;

            ctx.send(CreateReply::default().ephemeral(true).content(&response))
                .await?;
        }
    }

    Ok(())
}

/// Shows your balance
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn balance(ctx: Context<'_>) -> Result<(), Error> {
    let mut conn = ctx.data().database.acquire().await?;

    let balance = Amount::from_sat(
        database::get_balance_for_user(
            &mut conn,
            &ctx.author().id,
            &Address::from_str(VRSC_CURRENCY_ID)?,
        )
        .await?
        .unwrap_or(0),
    );

    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .content(format!("Your balance is: {}", balance)),
    )
    .await?;

    Ok(())
}

/// Get an address to deposit funds to the tipbot wallet
#[instrument(skip(ctx), fields(request_id = %Uuid::new_v4() ))]
#[poise::command(slash_command, category = "Wallet")]
pub async fn deposit(ctx: Context<'_>) -> Result<(), Error> {
    let mut tx = ctx.data().database.begin().await?;
    debug!(
        "user {} ({}) demands a deposit address",
        ctx.author().name,
        ctx.author().id
    );

    let address = match database::get_address_from_user(
        &mut *tx,
        &ctx.author().id,
        &Address::from_str(VRSC_CURRENCY_ID)?,
    )
    .await?
    {
        Some(address) => address,
        None => {
            let client = &ctx.data().verus().unwrap();
            let address = client.get_new_address().unwrap();
            database::store_new_address_for_user(
                &mut *tx,
                &ctx.author().id,
                &address,
                &Address::from_str(VRSC_CURRENCY_ID)?,
            )
            .await?;

            address
        }
    };

    tx.commit().await?;

    send_deposit_address_msg(ctx, &address).await?;

    Ok(())
}

async fn send_deposit_address_msg(ctx: Context<'_>, address: &Address) -> Result<(), Error> {
    let qr = QRBuilder::new(address.to_string())
        .build()
        .map_err(|e| format!("QR builder error: {:?}", e))?;

    let img_bytes = ImageBuilder::default()
        .shape(Shape::RoundedSquare)
        .fit_width(400)
        .module_color([49, 101, 212, 255])
        .background_color([255, 255, 255, 0])
        .to_bytes(&qr)
        .map_err(|e| format!("Image build error: {}", e))?;

    let filename = format!("{address}.png");
    let embed = CreateEmbed::default()
        .image(format!("attachment://{filename}"))
        .field("Address", format!("{}", address.to_string()), false);
    let attachment = CreateAttachment::bytes(img_bytes, &filename);

    ctx.send(
        CreateReply::default()
            .embed(embed)
            .ephemeral(true)
            .attachment(attachment),
    )
    .await?;

    Ok(())
}

// Sendcurrency works with op-ids because it can work with zk-transactions. Therefore the txid of
// a transaction is not always known directly after sending.
// This function waits a bit and gets the txid once the operation_status RPC gives one.
// if it doesn't give one, the user is notified and the op-id is stored in the database.
async fn wait_for_sendcurrency_finish(
    tx: &mut Transaction<'_, Postgres>,
    client: &Client,
    opid: &str,
) -> Result<Option<Txid>, Error> {
    // from https://buildmedia.readthedocs.org/media/pdf/zcash/english-docs/zcash.pdf
    // status can be one of queued, executing, failed or success.
    // we should sleep if status is one of queued or executing
    // we should return when status is one of failed or success.
    loop {
        trace!("getting operation status: {}", &opid);
        let operation_status = client.z_get_operation_status(vec![&opid])?;
        trace!("got operation status: {:?}", &operation_status);

        if let Some(Some(opstatus)) = operation_status.first() {
            if ["queued", "executing"].contains(&opstatus.status.as_ref()) {
                tokio::time::sleep(Duration::from_millis(100)).await;
                trace!("opid still executing");
                continue;
            }

            let params = opstatus.params.first().as_ref().unwrap().as_ref().unwrap();

            if let Some(txid) = &opstatus.result {
                trace!(
                    "there was an operation_status, operation was executed with status: {}",
                    opstatus.status
                );

                database::store_opid(
                    &mut *tx,
                    &opid,
                    &opstatus.status,
                    opstatus.creation_time as i64,
                    opstatus.result.as_ref().map(|txid| txid.txid),
                    &params.address,
                    params.amount,
                    &params.currency.as_ref().unwrap_or(&String::from("VRSC")),
                )
                .await?;
                return Ok(Some(txid.txid));
            } else {
                error!("execution failed with status: {}", opstatus.status);

                database::store_opid(
                    &mut *tx,
                    &opid,
                    &opstatus.status,
                    opstatus.creation_time as i64,
                    opstatus.result.as_ref().map(|txid| txid.txid),
                    &params.address,
                    params.amount,
                    &params.currency.as_ref().unwrap(),
                )
                .await?;
            }
        } else {
            trace!("there was NO operation_status");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

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

// In this context, get the balance of the sending user, check if it is sufficient, and return it.
pub async fn get_and_check_balance(
    ctx: &Context<'_>,
    amount_to_check: Amount,
    tx_fee: Amount,
) -> Result<Option<Amount>, Error> {
    let mut conn = ctx.data().database.acquire().await?;

    if let Some(balance) = database::get_balance_for_user(
        &mut conn,
        &ctx.author().id,
        &Address::from_str(VRSC_CURRENCY_ID)?,
    )
    .await?
    {
        trace!("tipper has balance");

        if balance_is_enough(
            &Amount::from_sat(balance),
            &amount_to_check,
            &tx_fee, // no fees for tipping
        ) {
            trace!("tipper has sufficient balance");
            return Ok(Some(Amount::from_sat(balance)));
        } else {
            trace!("balance is insufficient");
            ctx.send(
                CreateReply::default()
                    .ephemeral(true)
                    .content(format!("Your balance is insufficient to tip that amount!")),
            )
            .await?;

            return Ok(None);
        }
    } else {
        trace!("tipper has no balance");
        warn!("user {} should have a balance!", ctx.author());

        ctx.send(
            CreateReply::default()
                .ephemeral(true)
                .content(format!("Your balance is insufficient to tip that amount!")),
        )
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
