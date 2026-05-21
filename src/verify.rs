//! Post-action verification for order lifecycle commands.
//!
//! After placing, replacing, or canceling an order, the Schwab API returns minimal data
//! (just a Location header with an order ID for placement, or nothing for
//! cancellation). This module provides a follow-up GET to retrieve the full
//! order status, giving LLM agents immediate confirmation of whether the
//! action succeeded.
//!
//! This mirrors the Go CLI's `fetchOrderActionData()` pattern: fire the
//! mutable action, then immediately GET the order so the agent sees the
//! real status instead of a useless Location header.

use serde::Serialize;
use serde_json::Value;

use crate::error::AppError;
use crate::raw;

/// Verification state after a mutable order action.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VerificationState {
    /// Order was successfully retrieved after the action.
    Verified,
    /// Follow-up GET failed; the action may still have succeeded.
    Unverified,
}

/// Result of a mutable order action (place, replace, cancel) with post-action
/// verification.
///
/// The `order` field preserves the submitted order payload that existed before
/// verification was added. The `verified_order` field contains the full order
/// from the follow-up GET when verification succeeds.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct OrderActionResult {
    /// What action was performed: "place", "replace", or "cancel".
    pub action: String,
    /// Order ID from the action (parsed from Location header for place).
    pub order_id: Option<i64>,
    /// Raw Location header from the API response when provided.
    pub location: Option<String>,
    /// The order payload submitted for mutable actions that send a body.
    pub order: Option<Value>,
    /// Whether the follow-up GET succeeded.
    pub verification_state: VerificationState,
    /// Reasons verification was incomplete (empty when verified).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub verification_failures: Vec<String>,
    /// Full order from the follow-up GET (present when verified).
    pub verified_order: Option<Value>,
    /// Preview digest used for place-from-preview actions.
    pub digest: Option<String>,
    /// Original command that created the preview (e.g., `order.option.buy-to-open`).
    pub original_command: Option<String>,
}

/// Performs a best-effort GET to verify an order after a mutable action.
///
/// Returns an [`OrderActionResult`] with the full order when the GET succeeds,
/// or with [`VerificationState::Unverified`] and failure reasons when it
/// doesn't. The action is still considered successful either way; verification
/// is purely informational for the agent.
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) async fn verify_order(
    client: &schwab::Client,
    account: &str,
    order_id: Option<i64>,
    action: &str,
    location: Option<String>,
    submitted_order: Option<Value>,
) -> OrderActionResult {
    let Some(id) = order_id else {
        return result_without_order_id(action, location, submitted_order);
    };

    match client.get_order(account, id).await {
        Ok(order) => match serde_json::to_value(&order) {
            Ok(value) => result_from_verified_order(
                id,
                action,
                location,
                submitted_order,
                raw::sanitize_order(value),
            ),
            Err(e) => OrderActionResult {
                action: action.to_string(),
                order_id: Some(id),
                location,
                order: submitted_order,
                verification_state: VerificationState::Unverified,
                verification_failures: vec![format!("failed to serialize order: {e}")],
                verified_order: None,
                digest: None,
                original_command: None,
            },
        },
        Err(e) => result_from_retrieval_failure(id, action, location, submitted_order, &e),
    }
}

/// Builds an unverified result when the mutable action did not return an ID.
fn result_without_order_id(
    action: &str,
    location: Option<String>,
    submitted_order: Option<Value>,
) -> OrderActionResult {
    OrderActionResult {
        action: action.to_string(),
        order_id: None,
        location,
        order: submitted_order,
        verification_state: VerificationState::Unverified,
        verification_failures: vec!["no order ID returned by API".to_string()],
        verified_order: None,
        digest: None,
        original_command: None,
    }
}

/// Builds an unverified result when the follow-up GET fails.
fn result_from_retrieval_failure(
    id: i64,
    action: &str,
    location: Option<String>,
    submitted_order: Option<Value>,
    error: &dyn std::fmt::Display,
) -> OrderActionResult {
    OrderActionResult {
        action: action.to_string(),
        order_id: Some(id),
        location,
        order: submitted_order,
        verification_state: VerificationState::Unverified,
        verification_failures: vec![format!("failed to retrieve order: {error}")],
        verified_order: None,
        digest: None,
        original_command: None,
    }
}

/// Builds a result from a successfully retrieved order payload.
fn result_from_verified_order(
    id: i64,
    action: &str,
    location: Option<String>,
    submitted_order: Option<Value>,
    verified_order: Value,
) -> OrderActionResult {
    let verification_failures = verification_failures(action, &verified_order);
    let verification_state = if verification_failures.is_empty() {
        VerificationState::Verified
    } else {
        VerificationState::Unverified
    };

    OrderActionResult {
        action: action.to_string(),
        order_id: Some(id),
        location,
        order: submitted_order,
        verification_state,
        verification_failures,
        verified_order: Some(verified_order),
        digest: None,
        original_command: None,
    }
}

/// Returns verification failures for action-specific post-action checks.
fn verification_failures(action: &str, order: &Value) -> Vec<String> {
    if action != "cancel" {
        return vec![];
    }

    match order.get("status").and_then(Value::as_str) {
        Some("CANCELED") => vec![],
        Some(status) => vec![format!(
            "cancel not confirmed: expected status CANCELED, got {status}"
        )],
        None => vec!["cancel not confirmed: verified order did not include a status".to_string()],
    }
}

/// Serializes an [`OrderActionResult`] into a JSON [`Value`].
///
/// Verification failures are already present in the result's
/// `verification_state` and `verification_failures` fields, so no
/// separate warnings wrapper is needed.
pub(crate) fn action_value(result: OrderActionResult) -> Result<Value, AppError> {
    Ok(serde_json::to_value(&result)?)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn action_value_preserves_unverified_failures() {
        let result = OrderActionResult {
            action: "place".to_string(),
            order_id: Some(12345),
            location: Some("https://api.schwab.com/orders/12345".to_string()),
            order: Some(json!({"orderType": "LIMIT"})),
            verification_state: VerificationState::Unverified,
            verification_failures: vec!["failed to retrieve order: timeout".to_string()],
            verified_order: None,
            digest: None,
            original_command: None,
        };

        let data = action_value(result).unwrap();
        assert_eq!(data["verification_state"], "unverified");
        assert_eq!(
            data["verification_failures"][0],
            "failed to retrieve order: timeout"
        );
    }

    #[test]
    fn action_value_verified_includes_all_fields() {
        let result = OrderActionResult {
            action: "place".to_string(),
            order_id: Some(12345),
            location: Some("https://api.schwab.com/orders/12345".to_string()),
            order: Some(json!({"orderType": "LIMIT"})),
            verification_state: VerificationState::Verified,
            verification_failures: vec![],
            verified_order: Some(json!({"orderId": 12345, "status": "WORKING"})),
            digest: None,
            original_command: None,
        };

        let data = action_value(result).unwrap();
        assert_eq!(data["order_id"], 12345);
        assert_eq!(data["verification_state"], "verified");
        assert_eq!(data["order"]["orderType"], "LIMIT");
        assert_eq!(data["verified_order"]["status"], "WORKING");
    }

    #[test]
    fn action_value_omits_empty_verification_failures() {
        let result = OrderActionResult {
            action: "cancel".to_string(),
            order_id: Some(99999),
            location: None,
            order: None,
            verification_state: VerificationState::Verified,
            verification_failures: vec![],
            verified_order: Some(json!({"orderId": 99999, "status": "CANCELED"})),
            digest: None,
            original_command: None,
        };

        let serialized = serde_json::to_string(&action_value(result).unwrap()).unwrap();
        // verification_failures should be skipped when empty
        assert!(!serialized.contains("verification_failures"));
    }

    #[test]
    fn action_value_includes_submitted_order_when_present() {
        let submitted = json!({"orderType": "LIMIT", "price": 150.0});
        let result = OrderActionResult {
            action: "place".to_string(),
            order_id: Some(12345),
            location: None,
            order: Some(submitted.clone()),
            verification_state: VerificationState::Verified,
            verification_failures: vec![],
            verified_order: Some(json!({"orderId": 12345, "status": "WORKING"})),
            digest: None,
            original_command: None,
        };

        let data = action_value(result).unwrap();
        assert_eq!(data["order"]["price"], 150.0);
    }

    #[test]
    fn unverified_result_without_order_id() {
        let result = result_without_order_id("place", None, Some(json!({"orderType": "MARKET"})));

        let data = action_value(result).unwrap();
        assert!(data["order_id"].is_null());
        assert_eq!(data["verification_state"], "unverified");
        assert_eq!(
            data["verification_failures"][0],
            "no order ID returned by API"
        );
    }

    #[test]
    fn retrieval_failure_result_is_unverified() {
        let result = result_from_retrieval_failure(
            12345,
            "place",
            Some("https://api.schwab.com/orders/12345".to_string()),
            Some(json!({"orderType": "LIMIT"})),
            &"timeout",
        );

        assert!(matches!(
            result.verification_state,
            VerificationState::Unverified
        ));
        assert_eq!(result.order_id, Some(12345));
        assert_eq!(result.order.unwrap()["orderType"], "LIMIT");
        assert_eq!(result.verified_order, None);
        assert_eq!(
            result.verification_failures,
            vec!["failed to retrieve order: timeout"]
        );
    }

    #[test]
    fn action_value_includes_preview_context() {
        let result = OrderActionResult {
            action: "place".to_string(),
            order_id: Some(12345),
            location: None,
            order: Some(json!({"orderType": "LIMIT"})),
            verification_state: VerificationState::Verified,
            verification_failures: vec![],
            verified_order: Some(json!({"orderId": 12345, "status": "WORKING"})),
            digest: Some("abc123def456".to_string()),
            original_command: Some("order.option.buy-to-open".to_string()),
        };

        let data = action_value(result).unwrap();
        assert_eq!(data["digest"], "abc123def456");
        assert_eq!(data["original_command"], "order.option.buy-to-open");
    }

    #[test]
    fn place_verification_accepts_retrieved_order() {
        let result = result_from_verified_order(
            12345,
            "place",
            None,
            Some(json!({"orderType": "LIMIT"})),
            json!({"orderId": 12345, "status": "WORKING"}),
        );

        assert!(matches!(
            result.verification_state,
            VerificationState::Verified
        ));
        assert!(result.verification_failures.is_empty());
        assert_eq!(result.order.unwrap()["orderType"], "LIMIT");
        assert_eq!(result.verified_order.unwrap()["status"], "WORKING");
    }

    #[test]
    fn cancel_verification_requires_canceled_status() {
        let result = result_from_verified_order(
            12345,
            "cancel",
            None,
            None,
            json!({"orderId": 12345, "status": "PENDING_CANCEL"}),
        );

        assert!(matches!(
            result.verification_state,
            VerificationState::Unverified
        ));
        assert_eq!(
            result.verification_failures,
            vec!["cancel not confirmed: expected status CANCELED, got PENDING_CANCEL"]
        );
        assert_eq!(result.verified_order.unwrap()["status"], "PENDING_CANCEL");
    }

    #[test]
    fn cancel_verification_accepts_canceled_status() {
        let result = result_from_verified_order(
            12345,
            "cancel",
            None,
            None,
            json!({"orderId": 12345, "status": "CANCELED"}),
        );

        assert!(matches!(
            result.verification_state,
            VerificationState::Verified
        ));
        assert!(result.verification_failures.is_empty());
    }

    #[test]
    fn cancel_verification_requires_status_field() {
        let result =
            result_from_verified_order(12345, "cancel", None, None, json!({"orderId": 12345}));

        assert!(matches!(
            result.verification_state,
            VerificationState::Unverified
        ));
        assert_eq!(
            result.verification_failures,
            vec!["cancel not confirmed: verified order did not include a status"]
        );
        assert_eq!(result.verified_order.unwrap()["orderId"], 12345);
    }
}
