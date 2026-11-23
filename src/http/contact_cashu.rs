//! NUT-24 response negotiation for the existing encrypted contact endpoint.
use super::*;
use lightning_goats::ledger::ContactAction;
use lightning_goats_cashu_receipt::{
    contact_fee::MAX_TOKEN_BYTES,
    receive_status::{AdmissionOutcome, http_delivery_id},
};

pub(super) async fn checkout(
    state: &AppState,
    intent: &lightning_goats::contact::Intent,
    id: uuid::Uuid,
    capability: &str,
    headers: &HeaderMap,
) -> Response {
    let invalid = || {
        private_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error":"invalid contact tender or conflicting intent"}),
        )
    };
    let pending = || {
        private_response(
            StatusCode::ACCEPTED,
            serde_json::json!({"id":id,"state":"pending","fee_sats":100}),
        )
    };
    let unavailable = || {
        private_response(
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({"error":"private checkout unavailable"}),
        )
    };
    if state.ledger.create_contact_intent(intent).await.is_err() {
        return invalid();
    }
    let status = match state.ledger.contact_status(id, capability).await {
        Ok(Some(value)) => value,
        _ => return unavailable(),
    };
    if matches!(status.state.as_str(), "paid" | "delivered") {
        return private_response(
            StatusCode::OK,
            serde_json::to_value(status).unwrap_or_default(),
        );
    }
    let mut values = headers.get_all("x-cashu").iter();
    let tender = match values.next() {
        None => None,
        Some(value) => {
            let Ok(token) = value.to_str() else {
                return invalid();
            };
            if values.next().is_some()
                || token.len() > MAX_TOKEN_BYTES
                || !token.starts_with("cashuB")
            {
                return invalid();
            }
            Some(serde_json::json!(token).to_string())
        }
    };
    let delivery = tender.as_ref().map(|s| http_delivery_id(s.as_bytes()));
    let Some(wallet) = &state.cashu else {
        return unavailable();
    };
    let mut action = state.ledger.contact_tender(intent, None, delivery).await;
    if matches!(action, Ok(ContactAction::NeedsDescriptor)) {
        let Ok(Some(descriptor)) = wallet.public_request().await else {
            return unavailable();
        };
        action = state
            .ledger
            .contact_tender(intent, Some(&descriptor), delivery)
            .await;
    }
    match action {
        Ok(ContactAction::NeedsDescriptor) => unavailable(),
        Ok(ContactAction::Fulfilled) => match state.ledger.contact_status(id, capability).await {
            Ok(Some(status)) => private_response(
                StatusCode::OK,
                serde_json::to_value(status).unwrap_or_default(),
            ),
            _ => unavailable(),
        },
        Ok(ContactAction::Challenge(request)) => {
            let Ok(header) = axum::http::HeaderValue::from_str(&request) else {
                return unavailable();
            };
            let mut response = private_response(
                StatusCode::PAYMENT_REQUIRED,
                serde_json::json!({"id":id,"fee_sats":100}),
            );
            response.headers_mut().insert("x-cashu", header);
            response
        }
        Ok(ContactAction::Pending) => pending(),
        Ok(ContactAction::Invalid) => invalid(),
        Ok(ContactAction::Submit(terms)) => {
            let (Some(delivery), Some(token)) = (delivery, tender) else {
                return invalid();
            };
            match wallet.submit_contact_tender(delivery, token, terms).await {
                Ok(reply) if matches!(reply.result, AdmissionOutcome::Rejected) => {
                    if state
                        .ledger
                        .reject_contact_tender(id, delivery)
                        .await
                        .is_err()
                    {
                        return pending();
                    }
                    invalid()
                }
                _ => pending(),
            }
        }
        Err(_) => unavailable(),
    }
}
