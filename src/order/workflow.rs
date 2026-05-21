//! Shared order execution workflow.
//!
//! Provides the common execution pipeline (mode dispatch, preview, place,
//! place-from-preview) used by both the equity and option command handlers.
//! Individual handlers build the order payload; this module handles everything
//! after that.

use serde_json::{Value, json};

use crate::error::AppError;

/// Execution mode for an order command.
#[derive(Debug)]
pub enum OrderMode {
    /// Serialize and return the order JSON locally without any API call.
    DryRun,
    /// Preview via API and save the preview payload to disk.
    SavePreview {
        /// Resolved account selector (hash or nickname).
        account: String,
    },
    /// Preview first (API call), then place immediately if accepted.
    PreviewFirst {
        /// Resolved account selector (hash or nickname).
        account: String,
    },
    /// Place the order directly.
    Place {
        /// Resolved account selector (hash or nickname).
        account: String,
    },
}

/// Determines the execution mode from CLI flags.
///
/// # Errors
///
/// Returns `AppError::OrderValidation` when flags conflict or when a flag
/// requiring `--account` is used without it.
pub fn determine_mode(
    account: Option<String>,
    save_preview: bool,
    preview_first: bool,
) -> Result<OrderMode, AppError> {
    match (account, save_preview, preview_first) {
        (None, false, false) => Ok(OrderMode::DryRun),
        (Some(a), false, false) => Ok(OrderMode::Place { account: a }),
        (Some(a), true, false) => Ok(OrderMode::SavePreview { account: a }),
        (Some(a), false, true) => Ok(OrderMode::PreviewFirst { account: a }),
        (Some(_), true, true) => Err(AppError::OrderValidation(
            "cannot use both --save-preview and --preview-first".to_string(),
        )),
        (None, true, _) => Err(AppError::OrderValidation(
            "--save-preview requires --account".to_string(),
        )),
        (None, false, true) => Err(AppError::OrderValidation(
            "--preview-first requires --account".to_string(),
        )),
    }
}

/// Resolves an account selector to its canonical Schwab account hash.
///
/// Uses the auth provider's bearer token for account discovery, then matches
/// the selector against known account hashes and nicknames.
#[cfg_attr(coverage_nightly, coverage(off))]
async fn resolve_account_hash(account: &str) -> Result<String, AppError> {
    let provider = crate::auth::provider()?;
    let token = provider.token().await?;
    let resolved = crate::account::resolve_account(&token, account).await?;
    Ok(resolved.account_hash)
}

/// Executes an order through the appropriate workflow mode.
///
/// Dispatches to dry-run, save-preview, preview-first, or direct-place
/// based on the [`OrderMode`]. Mutable modes (place, preview-first) check
/// the mutable-operations guard before making API calls.
///
/// # Errors
///
/// Returns `AppError` on validation failures, auth issues, Schwab API errors,
/// or when mutable operations are disabled.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn execute_order(
    client: &schwab::Client,
    order: &schwab::OrderBuilder,
    mode: OrderMode,
    command_label: &str,
) -> Result<Value, AppError> {
    match mode {
        OrderMode::DryRun => Ok(serde_json::to_value(order)?),

        OrderMode::Place { account } => {
            crate::config::require_mutable_enabled()?;
            let account_hash = resolve_account_hash(&account).await?;
            place_order(client, order, &account_hash).await
        }

        OrderMode::SavePreview { account } => {
            let account_hash = resolve_account_hash(&account).await?;
            save_preview(client, order, &account_hash, command_label).await
        }

        OrderMode::PreviewFirst { account } => {
            crate::config::require_mutable_enabled()?;
            let account_hash = resolve_account_hash(&account).await?;
            preview_first(client, order, &account_hash).await
        }
    }
}

/// Executes an order workflow with an already-resolved canonical account hash.
///
/// This is useful when a command must fetch a source resource from the same
/// account before routing a new payload through the standard order workflow.
/// It preserves the normal mode behavior without repeating account discovery.
///
/// # Errors
///
/// Returns `AppError` on serialization failures, Schwab API errors, or when
/// mutable operations are disabled for modes that can place orders.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn execute_order_with_account_hash(
    client: &schwab::Client,
    order: &schwab::OrderBuilder,
    mode: OrderMode,
    account_hash: &str,
    command_label: &str,
) -> Result<Value, AppError> {
    match mode {
        OrderMode::DryRun => Ok(serde_json::to_value(order)?),
        OrderMode::Place { .. } => {
            crate::config::require_mutable_enabled()?;
            place_order(client, order, account_hash).await
        }
        OrderMode::SavePreview { .. } => {
            save_preview(client, order, account_hash, command_label).await
        }
        OrderMode::PreviewFirst { .. } => {
            crate::config::require_mutable_enabled()?;
            preview_first(client, order, account_hash).await
        }
    }
}

/// Places an order and returns the post-place verification payload.
async fn place_order(
    client: &schwab::Client,
    order: &schwab::OrderBuilder,
    account_hash: &str,
) -> Result<Value, AppError> {
    let response = client.place_order(account_hash, order).await?;
    let order_json = serde_json::to_value(order)?;

    let result = crate::verify::verify_order(
        client,
        account_hash,
        response.order_id,
        "place",
        response.location,
        Some(order_json),
    )
    .await;

    crate::verify::action_value(result)
}

/// Previews an order and saves a digest for later placement.
async fn save_preview(
    client: &schwab::Client,
    order: &schwab::OrderBuilder,
    account_hash: &str,
    command_label: &str,
) -> Result<Value, AppError> {
    let _preview = client.preview_order(account_hash, order).await?;
    let order_json = serde_json::to_value(order)?;
    let digest = crate::order::preview::save_preview(account_hash, order, command_label)?;

    Ok(json!({
        "order": order_json,
        "preview": "accepted",
        "digest": digest,
        "digest_ttl_seconds": 900,
    }))
}

/// Previews an order, places it, and returns post-place verification.
async fn preview_first(
    client: &schwab::Client,
    order: &schwab::OrderBuilder,
    account_hash: &str,
) -> Result<Value, AppError> {
    let _preview = client.preview_order(account_hash, order).await?;
    place_order(client, order, account_hash).await
}

/// Places an order from a previously saved preview digest.
///
/// Validates the SHA-256 digest, TTL, and account match before submitting
/// the exact saved payload. Includes post-place verification.
///
/// # Errors
///
/// Returns `AppError` on mutable-guard failure, expired/invalid preview,
/// account mismatch, or Schwab API errors.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn place_from_saved_preview(
    client: &schwab::Client,
    account: &str,
    digest: &str,
) -> Result<Value, AppError> {
    crate::config::require_mutable_enabled()?;
    let account_hash = resolve_account_hash(account).await?;
    let saved = crate::order::preview::load_preview(digest, &account_hash)?;
    let response = client.place_order(&account_hash, &saved.order).await?;

    let mut result = crate::verify::verify_order(
        client,
        &account_hash,
        response.order_id,
        "place",
        response.location,
        Some(saved.order),
    )
    .await;

    result.digest = Some(digest.to_string());
    result.original_command = Some(saved.command);

    crate::verify::action_value(result)
}

/// Previews a raw JSON order payload via the Schwab API.
///
/// Optionally saves the preview digest for later `place-from-preview`.
///
/// # Errors
///
/// Returns `AppError` on invalid JSON, auth failures, or Schwab API errors.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn execute_raw_preview(
    client: &schwab::Client,
    account: &str,
    json_str: &str,
    save: bool,
    command_label: &str,
) -> Result<Value, AppError> {
    let order: Value = serde_json::from_str(json_str)
        .map_err(|e| AppError::OrderValidation(format!("invalid JSON: {e}")))?;
    let account_hash = resolve_account_hash(account).await?;
    let _preview = client.preview_order(&account_hash, &order).await?;

    let mut data = json!({
        "order": order,
        "preview": "accepted",
    });

    if save {
        let digest = crate::order::preview::save_preview(&account_hash, &order, command_label)?;
        data["digest"] = Value::String(digest);
        data["digest_ttl_seconds"] = Value::Number(900.into());
    }

    Ok(data)
}

/// Places a raw JSON order payload directly via the Schwab API.
///
/// Includes post-place verification.
///
/// # Errors
///
/// Returns `AppError` on mutable-guard failure, invalid JSON, auth failures,
/// or Schwab API errors.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn execute_raw_place(
    client: &schwab::Client,
    account: &str,
    json_str: &str,
) -> Result<Value, AppError> {
    crate::config::require_mutable_enabled()?;
    let order: Value = serde_json::from_str(json_str)
        .map_err(|e| AppError::OrderValidation(format!("invalid JSON: {e}")))?;
    let account_hash = resolve_account_hash(account).await?;
    let response = client.place_order(&account_hash, &order).await?;

    let result = crate::verify::verify_order(
        client,
        &account_hash,
        response.order_id,
        "place",
        response.location,
        Some(order),
    )
    .await;

    crate::verify::action_value(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::to_number;

    #[test]
    fn no_account_is_dry_run() {
        let mode = determine_mode(None, false, false).unwrap();
        assert!(matches!(mode, OrderMode::DryRun));
    }

    #[test]
    fn account_only_is_place() {
        let mode = determine_mode(Some("HASH".to_string()), false, false).unwrap();
        assert!(matches!(mode, OrderMode::Place { ref account } if account == "HASH"));
    }

    #[test]
    fn account_save_preview_is_save_preview() {
        let mode = determine_mode(Some("HASH".to_string()), true, false).unwrap();
        assert!(matches!(mode, OrderMode::SavePreview { ref account } if account == "HASH"));
    }

    #[test]
    fn account_preview_first_is_preview_first() {
        let mode = determine_mode(Some("HASH".to_string()), false, true).unwrap();
        assert!(matches!(mode, OrderMode::PreviewFirst { ref account } if account == "HASH"));
    }

    #[test]
    fn both_flags_is_error() {
        let err = determine_mode(Some("HASH".to_string()), true, true).unwrap_err();
        assert!(err.to_string().contains("cannot use both"));
    }

    #[test]
    fn save_preview_without_account_is_error() {
        let err = determine_mode(None, true, false).unwrap_err();
        assert!(
            err.to_string()
                .contains("--save-preview requires --account")
        );
    }

    #[test]
    fn preview_first_without_account_is_error() {
        let err = determine_mode(None, false, true).unwrap_err();
        assert!(
            err.to_string()
                .contains("--preview-first requires --account")
        );
    }

    #[test]
    fn both_flags_without_account_hits_save_preview_error() {
        // (None, true, true) matches the (None, true, _) arm
        let err = determine_mode(None, true, true).unwrap_err();
        assert!(
            err.to_string()
                .contains("--save-preview requires --account")
        );
    }

    #[test]
    fn order_mode_debug_includes_variant_name() {
        let dry = determine_mode(None, false, false).unwrap();
        assert!(format!("{dry:?}").contains("DryRun"));

        let place = determine_mode(Some("H".to_string()), false, false).unwrap();
        assert!(format!("{place:?}").contains("Place"));

        let save = determine_mode(Some("H".to_string()), true, false).unwrap();
        assert!(format!("{save:?}").contains("SavePreview"));

        let pf = determine_mode(Some("H".to_string()), false, true).unwrap();
        assert!(format!("{pf:?}").contains("PreviewFirst"));
    }

    fn sample_order() -> schwab::OrderBuilder {
        schwab::OrderBuilder::limit_buy("AAPL", to_number(1.0).unwrap(), to_number(150.25).unwrap())
            .session(schwab::Session::Normal)
            .duration(schwab::Duration::Day)
    }

    fn sample_client() -> schwab::Client {
        schwab::Client::new(schwab::Config::new().bearer_token("TOKEN"))
    }

    #[tokio::test]
    async fn execute_order_dry_run_serializes_order_without_account_lookup() {
        let client = sample_client();
        let value = execute_order(
            &client,
            &sample_order(),
            OrderMode::DryRun,
            "order equity buy",
        )
        .await
        .unwrap();

        assert_eq!(value["orderType"], "LIMIT");
        assert_eq!(value["session"], "NORMAL");
        assert_eq!(value["duration"], "DAY");
        assert_eq!(value["orderLegCollection"][0]["instruction"], "BUY");
        assert_eq!(
            value["orderLegCollection"][0]["instrument"]["symbol"],
            "AAPL"
        );
    }

    #[tokio::test]
    async fn execute_order_with_account_hash_dry_run_ignores_account_hash() {
        let client = sample_client();
        let value = execute_order_with_account_hash(
            &client,
            &sample_order(),
            OrderMode::DryRun,
            "CANONICAL_HASH",
            "order repeat",
        )
        .await
        .unwrap();

        assert_eq!(value["orderType"], "LIMIT");
        assert_eq!(
            value["price"],
            serde_json::to_value(to_number(150.25).unwrap()).unwrap()
        );
    }

    #[tokio::test]
    async fn execute_raw_preview_rejects_invalid_json_before_account_lookup() {
        let client = sample_client();
        let err = execute_raw_preview(&client, "HASH", "{not json", false, "order preview-raw")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("invalid JSON"));
    }
}
