use poise::{
    CreateReply,
    serenity_prelude::{CreateEmbed, UserId},
};
use sqlx::{Postgres, Transaction};
use std::{str::FromStr, sync::Arc, time::Duration};
use tracing::{debug, error, instrument, trace};
use uuid::Uuid;
use vrsc::{Address, Amount};
use vrsc_rpc::{bitcoin::Txid, client::RpcApi, json::GetTransactionDetailsCategory};

use crate::{
    Context, Error, VRSC_CURRENCY_ID, database,
    wallet_listener::{TransactionProcessor, process_txid},
};

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn adminhelp(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(CreateReply::default().ephemeral(true).content(
        r#"```
!status                         - (financial) status of the bot
!blacklist <user_id>            - blacklists a user (no more tipping, deposits & withdraws)
!rescanfromheight <blockheight> - rescan blockchain from given height
!checktxid <txid>               - manually check txid (in case user balance was not updated)
!withdrawenabled <true/false>   - enable / disable withdraws
!depositenabled <true/false>    - enable / disable deposits
!setwithdrawfee <sats>          - sets the fee a user is charged when withdrawing funds
!maintenance <true/false>       - set maintenance mode (commands are not executed) 
```"#,
    ))
    .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let mut conn = ctx.data().database.acquire().await?;

    let maintenance = *ctx.data().tx_processor.maintenance.read().await;
    let deposits_enabled = *ctx.data().tx_processor.deposits_enabled.read().await;
    let withdrawals_enabled = *ctx.data().withdrawals_enabled.read().await;
    let total_balance = Amount::from_sat(
        database::get_total_balance(&mut conn, &Address::from_str(VRSC_CURRENCY_ID)?).await?,
    );
    let total_tipped = Amount::from_sat(
        database::get_total_tipped(&mut conn, &Address::from_str(VRSC_CURRENCY_ID)?).await?,
    );
    let largest_tip = Amount::from_sat(
        database::get_largest_tip(&mut conn, &Address::from_str(VRSC_CURRENCY_ID)?).await?,
    );
    let total_deposited = totaldeposited(ctx).await?;
    let total_withdrawn = totalwithdrawn(ctx).await?;

    let client = ctx.data().verus()?;

    let daemon_balance = client.get_balance(None, None)?;

    debug!("total balance: {total_balance}");
    debug!("total_tipped: {total_tipped}");
    debug!("largest_tip: {largest_tip}");
    debug!("total_deposited: {total_deposited}");
    debug!("total_withdrawn: {total_withdrawn}");
    debug!("daemon_balance: {daemon_balance}");

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .title("Status report")
                .field("bot in maintenance", maintenance.to_string(), false)
                .field("deposits enabled", deposits_enabled.to_string(), false)
                .field(
                    "withdrawals enabled",
                    withdrawals_enabled.to_string(),
                    false,
                )
                .field("VRSC daemon balance", daemon_balance.to_string(), false)
                .field("Tipbot balance", total_balance.to_string(), false)
                .field("Total deposited", total_deposited.to_string(), false)
                .field("Total withdrawn", total_withdrawn.to_string(), false)
                .field(
                    "Database deposits - withdraws",
                    total_deposited
                        .checked_sub(total_withdrawn)
                        .unwrap_or(Amount::ZERO)
                        .to_string_in(vrsc::Denomination::Verus),
                    false,
                )
                .field("Total tipped", total_tipped.to_string(), false)
                .field("Largest tip", largest_tip.to_string(), false)
                .field(
                    "Bot fees _(minus network fees)_",
                    {
                        if let Some(pos_amount) = daemon_balance.checked_sub(total_balance) {
                            pos_amount
                        } else {
                            Amount::ZERO
                        }
                    }
                    .to_string_in(vrsc::Denomination::Verus),
                    false,
                ),
        ),
    )
    .await?;

    Ok(())
}

async fn totaldeposited(ctx: Context<'_>) -> Result<Amount, Error> {
    ctx.defer().await?;

    let mut conn = ctx.data().database.acquire().await?;
    let client = ctx.data().verus()?;

    let deposit_transactions =
        database::get_all_txids(&mut conn, "deposit", &Address::from_str(VRSC_CURRENCY_ID)?)
            .await?;

    let mut sum = Amount::ZERO;

    for txid in deposit_transactions {
        let raw_tx = client.get_raw_transaction_verbose(&txid)?;

        for vout in raw_tx.vout.iter() {
            if let Some(addresses) = &vout.script_pubkey.addresses {
                for address in addresses {
                    if let Some(user_id) =
                        database::get_user_from_address(&mut conn, address).await?
                    {
                        trace!("there is a user for this address: {user_id}",);
                        sum = sum.checked_add(vout.value_sat).unwrap();
                    }
                }
            } else {
                trace!("no addresses found in scriptpubkey");
            }
        }
    }

    Ok(sum)
}

pub async fn totalwithdrawn(ctx: Context<'_>) -> Result<Amount, Error> {
    ctx.defer().await?;

    let mut conn = ctx.data().database.acquire().await?;
    let client = ctx.data().verus()?;

    let withdraw_transactions = database::get_all_txids(
        &mut *conn,
        "withdraw",
        &Address::from_str(VRSC_CURRENCY_ID)?,
    )
    .await?;

    let mut sum = Amount::ZERO;

    for txid in withdraw_transactions {
        let raw_tx = client.get_raw_transaction_verbose(&txid)?;
        let vout = raw_tx.vout.first().unwrap();

        sum = sum.checked_add(vout.value_sat).unwrap();
    }

    Ok(sum)
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn blacklist(ctx: Context<'_>, user_id: UserId) -> Result<(), Error> {
    debug!("no more fun for {user_id}");
    let mut tx = ctx.data().database.begin().await?;

    if let Some(status) = database::get_blacklist_status(&mut tx, user_id).await? {
        if status == true {
            database::set_blacklist_status(&mut tx, user_id, false).await?;
            if let Ok(mut blacklist) = ctx.data().blacklist.lock() {
                blacklist.remove(&user_id);
            }
            ctx.send(
                CreateReply::default().content(format!("user {user_id} removed from blacklist")),
            )
            .await?;
            trace!("{user_id} has been removed from blacklist");
        } else {
            database::set_blacklist_status(&mut tx, user_id, true).await?;
            if let Ok(mut blacklist) = ctx.data().blacklist.lock() {
                blacklist.insert(user_id);
            }

            ctx.send(CreateReply::default().content(format!("user {user_id} blacklisted")))
                .await?;

            trace!("{user_id} has been added to blacklist");
        }

        tx.commit().await?;
    } else {
        error!("user not in database");
    }

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn setwithdrawfee(ctx: Context<'_>, amount: u64) -> Result<(), Error> {
    let withdrawal_fee = &ctx.data().withdrawal_fee;

    debug!("fee before changing: {:?}", withdrawal_fee);

    let mut write = withdrawal_fee.write().await;
    *write = Amount::from_sat(amount);

    debug!("fee after changing: {:?}", withdrawal_fee);
    ctx.send(CreateReply::default().content(format!("Withdraw fee set to {} sats", amount)))
        .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn rescanfromheight(ctx: Context<'_>, height: u64) -> Result<(), Error> {
    trace!("Initiating a rescan from height {height}");

    let client = &ctx.data().verus()?;
    if let Ok(()) = client.rescan_from_height(height) {
        trace!("rescan done");

        tokio::time::sleep(Duration::from_secs(1)).await;
        ctx.data().tx_processor.process_long_queue().await?;
        ctx.data().tx_processor.process_short_queue().await?;
        ctx.send(CreateReply::default().content("Rescan done"))
            .await?;
    } else {
        trace!("rescan did not succeed")
    }

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn withdrawenabled(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("set withdraws enabled to {value}");

    {
        let withdrawals_enabled = &ctx.data().withdrawals_enabled;
        let mut write = withdrawals_enabled.write().await;
        *write = value;
    }

    ctx.send(CreateReply::default().content(format!("Withdraws enabled: {value}")))
        .await?;

    Ok(())
}

#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn depositenabled(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("set deposits enabled to {value}");

    {
        let mut tx = ctx.data().database.begin().await?;
        let deposits_enabled = &ctx.data().deposits_enabled;
        let mut write = deposits_enabled.write().await;
        if *write == true && value == false {
            trace!("need to process possible unprocessed transactions");

            let tx_proc = Arc::clone(&ctx.data().tx_processor);

            process_stored_txids(&mut tx, tx_proc).await?
        }
        tx.commit().await?;
        *write = value;
    }

    ctx.send(CreateReply::default().content(format!("Deposits enabled: {value}")))
        .await?;

    Ok(())
}

/// Manually checks a tx if it was not caught with rescan
#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn checktxid(ctx: Context<'_>, txid: Txid) -> Result<(), Error> {
    trace!("manually check {txid}");
    let http = ctx.serenity_context().http.clone();
    let mut tx = ctx.data().database.begin().await?;

    let client = &ctx.data().verus()?;

    if let Ok(raw_tx) = client.get_raw_transaction_verbose(&txid) {
        process_txid(http, &mut *tx, &raw_tx).await?;
        tx.commit().await?;
    }

    Ok(())
}

/// Manually add withdraw tx when one didn't register
///
/// Needs discord_user_id, txid, tx_fee (in sats)
#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn manuallyaddwithdraw(
    ctx: Context<'_>,
    user_id: UserId,
    txid: Txid,
    tx_fee: u64,
) -> Result<(), Error> {
    trace!("manually add withdraw: {txid}");
    let mut tx = ctx.data().database.begin().await?;
    let uuid = Uuid::new_v4();

    debug!("manually storing withdraw transaction: {uuid}: {user_id} - {txid} ({tx_fee})");

    database::store_withdraw_transaction(
        &mut *tx,
        &uuid,
        &user_id,
        Some(&txid),
        &format!("opid-{uuid}"),
        &Amount::from_sat(tx_fee),
        &Address::from_str(VRSC_CURRENCY_ID)?,
    )
    .await?;

    tx.commit().await?;

    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .content(format!("{txid} stored.")),
    )
    .await?;

    Ok(())
}

/// Set maintenance mode on or off
#[instrument(skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn maintenance(ctx: Context<'_>, value: bool) -> Result<(), Error> {
    trace!("setting maintenance mode to {value}");

    {
        let mut tx = ctx.data().database.begin().await?;
        let mut write = ctx.data().tx_processor.maintenance.write().await;
        if *write == true && value == false {
            trace!("need to process possible unprocessed transactions");

            let tx_proc = Arc::clone(&ctx.data().tx_processor);

            process_stored_txids(&mut tx, tx_proc).await?
        }
        tx.commit().await?;
        *write = value;
    }

    ctx.send(CreateReply::default().content(format!("Maintenance mode set to {value}")))
        .await?;

    Ok(())
}

async fn process_stored_txids(
    tx: &mut Transaction<'_, Postgres>,
    tx_proc: Arc<TransactionProcessor>,
) -> Result<(), Error> {
    let stored_txids = database::get_stored_txids(&mut *tx).await?;

    for txid in stored_txids {
        trace!("processing {txid}");
        // checks tx and puts them in a queue
        tx_proc.check_tx(txid).await?;

        // process the queue immediately
        tx_proc.process_long_queue().await?;
        tx_proc.process_short_queue().await?;

        database::set_stored_txid_to_processed(&mut *tx, &txid).await?;
    }

    Ok(())
}

#[instrument(err, skip(ctx))]
#[poise::command(dm_only, owners_only, prefix_command, hide_in_help)]
pub async fn batch_convert_transaction_amounts(ctx: Context<'_>, limit: u32) -> Result<(), Error> {
    let verus_client = ctx.data().verus()?;

    let mut created_at = None;

    tracing::trace!("adding amounts from deposits");
    loop {
        let mut tx = ctx.data().database.begin().await?;

        let db_txns = database::get_transactions_without_amount(
            &mut *tx,
            created_at,
            Some("deposit"),
            limit as i64,
        )
        .await?;

        if db_txns.is_empty() {
            tracing::info!("All deposits done");
            break;
        }

        for db_tx in db_txns {
            tracing::trace!("checking {}", db_tx.1);

            match verus_client.get_transaction(&db_tx.1, None) {
                Ok(raw_tx) => {
                    let Some(send_detail) = raw_tx
                        .details
                        .iter()
                        .find(|detail| detail.category == GetTransactionDetailsCategory::Receive)
                    // every deposit should have a receive
                    else {
                        tracing::warn!(
                            "deposit transaction in db does not have receive in daemon tx"
                        );

                        continue;
                    };

                    let amount = (send_detail.amount.abs() * 100_000_000.0) as u64;
                    let address = &send_detail.address.to_string();
                    // fee is irrelevant when depositing

                    database::update_transaction(
                        &mut *tx,
                        &db_tx.1.to_string(),
                        amount,
                        address,
                        None,
                    )
                    .await?;

                    created_at = Some(db_tx.3);
                }
                Err(e) => {
                    tracing::warn!(?e, txid = %db_tx.1, "Transaction not found, needs getrawtransaction");

                    continue;
                }
            }
        }

        tx.commit().await?;
    }

    tracing::trace!("adding amounts from withdrawals");

    loop {
        let mut tx = ctx.data().database.begin().await?;

        let db_txns = database::get_transactions_without_amount(
            &mut *tx,
            created_at,
            Some("withdraw"),
            limit as i64,
        )
        .await?;

        if db_txns.is_empty() {
            tracing::info!("All withdrawals done");
            break;
        }

        for db_tx in db_txns {
            tracing::trace!("checking {}", db_tx.1);

            match verus_client.get_transaction(&db_tx.1, None) {
                Ok(raw_tx) => {
                    let Some(send_detail) = raw_tx
                        .details
                        .iter()
                        .find(|detail| detail.category == GetTransactionDetailsCategory::Send)
                    // every transaction has a send
                    else {
                        tracing::warn!(
                            txid = ?db_tx.1,
                            "withdraw transaction in db does not have Send transaction in daemon"
                        );

                        continue;
                    };
                    let actual_tx_fee = (send_detail.fee.unwrap().abs() * 100_000_000.0) as u64;
                    let amount = (send_detail.amount.abs() * 100_000_000.0) as u64;
                    let address = &send_detail.address.to_string();

                    database::update_transaction(
                        &mut *tx,
                        &db_tx.1.to_string(),
                        amount,
                        address,
                        Some(actual_tx_fee),
                    )
                    .await?;

                    created_at = Some(db_tx.3);
                }
                Err(e) => {
                    tracing::warn!(?e, txid = %db_tx.1, "Transaction not found, needs getrawtransaction");

                    continue;
                }
            }
        }

        tx.commit().await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}

// let raw_tx = verus_client.get_raw_transaction_verbose(&db_tx.1)?;

// match raw_tx.vout.len() {
//     // no change address, so always the user
//     1 => {
//         // todo add actual tx fee
//         let amount = raw_tx.vout.first().unwrap().value_sat.as_sat();
//         database::update_transaction(&mut *tx, &db_tx.1.to_string(), amount).await?;

//         tracing::info!(txid = %db_tx.1, %amount, "stored amount");

//         // no need to do any address lookup
//         continue;
//     }
//     // one address is the users' address, the other should be our change address
//     // verify using `validateaddress`
//     2 => {
//         for tx_vout in raw_tx.vout {
//             let not_mine = tx_vout
//                 .script_pubkey
//                 .addresses
//                 // unwrap: a vout always has addresses
//                 .unwrap()
//                 .iter()
//                 .find(|address| {
//                     !verus_client
//                         .validate_address(&address.to_string())
//                         .unwrap()
//                         .is_mine
//                 })
//                 .cloned();

//             if let Some(user_address) = not_mine {
//                 let amount = tx_vout.value_sat;
//                 tracing::trace!(%user_address, %amount, "found address, storing amount");

//                 // todo:
//                 // filter on amount
//                 // add in actual tx fee.

//                 continue;
//             }
//         }
//     }
//     _ => {
//         tracing::warn!(
//             txid = %db_tx.1,
//             vout = ?raw_tx.vout,
//             "withdraw transaction has 0 or more than 2 outputs, needs investigation"
//         )
//     }
// }
