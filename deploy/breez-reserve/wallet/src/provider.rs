use anyhow::{Context, Result, ensure};
use breez_sdk_spark::{
    BreezSdk, GetPaymentRequest, Network, Payment, PaymentDetails, PaymentRequest, PaymentStatus,
    PaymentType, PrepareSendPaymentRequest, PrepareSendPaymentResponse, SdkBuilder, Seed,
    SendPaymentMethod, SendPaymentOptions, SendPaymentRequest, default_config,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;

use crate::{Create, Description, Invoice, Pay};

pub async fn connect(
    state: &Path,
    network: &str,
    mnemonic: String,
    api_key: Option<String>,
) -> Result<BreezSdk> {
    let mut config = default_config(if network == "mainnet" {
        Network::Mainnet
    } else {
        Network::Regtest
    });
    config.api_key = api_key;
    config.real_time_sync_server_url = None;
    config.prefer_spark_over_lightning = false;
    config.sync_interval_secs = 300;
    let seed = Seed::Mnemonic {
        mnemonic,
        passphrase: None,
    };
    Ok(SdkBuilder::new(config, seed)
        .with_default_storage(
            state
                .join("spark")
                .to_str()
                .context("invalid SDK storage path")?
                .to_owned(),
        )
        .build()
        .await?)
}

pub async fn create(sdk: &BreezSdk, request: &Create) -> Result<(String, String)> {
    let (memo, digest) = match &request.description {
        Description::MintQuote { memo } => (memo.clone(), None),
        Description::ProjectQuote {
            description_hash, ..
        } => (
            String::new(),
            Some(
                hex::decode(description_hash)?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("digest length"))?,
            ),
        ),
    };
    Ok(sdk
        .goats_create_invoice(request.amount_sat, memo, digest, request.expiry_secs)
        .await?)
}

pub async fn receive_status(sdk: &BreezSdk, invoice: &Invoice) -> Result<Option<Value>> {
    let Some(payment) = sdk
        .goats_receive_transfer(&invoice.lookup_reference)
        .await?
    else {
        return Ok(None);
    };
    if payment.status != PaymentStatus::Completed {
        return Ok(None);
    }
    ensure!(
        payment.payment_type == PaymentType::Receive
            && payment.amount == u128::from(invoice.amount_sat),
        "recipient transfer mismatch"
    );
    let _ = proof(&payment, &invoice.invoice, &invoice.payment_hash)?;
    Ok(Some(
        json!({"state": "settled", "payment_hash": invoice.payment_hash,
        "amount_sat": invoice.amount_sat, "transfer_id": payment.id, "settled_at": payment.timestamp}),
    ))
}

pub async fn prepare(
    sdk: &BreezSdk,
    invoice: &str,
    amount: u64,
) -> Result<(PrepareSendPaymentResponse, u64)> {
    let prepared = sdk
        .prepare_send_payment(PrepareSendPaymentRequest {
            payment_request: PaymentRequest::Input {
                input: invoice.to_owned(),
            },
            amount: None,
            token_identifier: None,
            conversion_options: None,
            fee_policy: None,
        })
        .await?;
    ensure!(
        prepared.amount == u128::from(amount)
            && prepared.conversion_estimate.is_none()
            && prepared.token_identifier.is_none(),
        "unexpected prepared payment"
    );
    let SendPaymentMethod::Bolt11Invoice {
        ref invoice_details,
        lightning_fee_sats,
        ..
    } = prepared.payment_method
    else {
        anyhow::bail!("non-Lightning payment path");
    };
    ensure!(
        invoice_details.invoice.bolt11.eq_ignore_ascii_case(invoice),
        "prepared invoice mismatch"
    );
    Ok((prepared, lightning_fee_sats))
}

pub async fn pay(
    sdk: &BreezSdk,
    prepared: PrepareSendPaymentResponse,
    operation: Uuid,
) -> Result<Payment> {
    Ok(sdk
        .send_payment(SendPaymentRequest {
            prepare_response: prepared,
            options: Some(SendPaymentOptions::Bolt11Invoice {
                prefer_spark: false,
                completion_timeout_secs: Some(5),
            }),
            idempotency_key: Some(operation.to_string()),
        })
        .await?
        .payment)
}

pub async fn outgoing(sdk: &BreezSdk, operation: Uuid) -> Option<Payment> {
    sdk.get_payment(GetPaymentRequest {
        payment_id: operation.to_string(),
    })
    .await
    .ok()
    .map(|response| response.payment)
}

pub fn send_status(request: &Pay, payment: &Payment, hash: &str, amount: u64) -> Result<Value> {
    ensure!(
        payment.id == request.operation_id.to_string() && payment.payment_type == PaymentType::Send,
        "outgoing transfer identity"
    );
    match payment.status {
        PaymentStatus::Completed => {
            ensure!(
                payment.amount == u128::from(amount)
                    && payment.fees <= u128::from(request.max_fee_sat),
                "outgoing amount or fee mismatch"
            );
            let preimage = proof(payment, &request.invoice, hash)?;
            Ok(
                json!({ "state": "settled", "operation_id": request.operation_id, "payment_hash": hash,
                "amount_sat": amount, "fee_sat": u64::try_from(payment.fees)?, "preimage": preimage }),
            )
        }
        PaymentStatus::Failed => {
            // Failed means a terminal SDK transfer for this exact invoice. Incomplete metadata stays held.
            let _ = matching_details(payment, &request.invoice, hash)?;
            Ok(
                json!({"state": "failed", "operation_id": request.operation_id, "payment_hash": hash}),
            )
        }
        _ => Ok(json!({"state": "pending", "operation_id": request.operation_id})),
    }
}

fn matching_details<'a>(
    payment: &'a Payment,
    invoice: &str,
    hash: &str,
) -> Result<&'a breez_sdk_spark::SparkHtlcDetails> {
    let Some(PaymentDetails::Lightning {
        invoice: recorded,
        htlc_details,
        ..
    }) = &payment.details
    else {
        anyhow::bail!("missing Lightning payment details");
    };
    ensure!(
        recorded.eq_ignore_ascii_case(invoice) && htlc_details.payment_hash == hash,
        "Lightning transfer binding mismatch"
    );
    Ok(htlc_details)
}

fn proof<'a>(payment: &'a Payment, invoice: &str, hash: &str) -> Result<&'a str> {
    let details = matching_details(payment, invoice, hash)?;
    let preimage = details
        .preimage
        .as_deref()
        .context("payment proof absent")?;
    let decoded: [u8; 32] = hex::decode(preimage)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("payment proof length"))?;
    ensure!(
        hex::encode(Sha256::digest(decoded)) == hash,
        "payment proof mismatch"
    );
    Ok(preimage)
}
