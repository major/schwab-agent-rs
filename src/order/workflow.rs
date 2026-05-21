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
    /// Preview first (API call), then place immediately if Schwab returns success.
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

/// Returns the current Schwab bearer token.
#[cfg_attr(coverage_nightly, coverage(off))]
async fn bearer_token() -> Result<String, AppError> {
    let provider = crate::auth::provider()?;
    Ok(provider.token().await?)
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
            save_preview(order, &account_hash, command_label).await
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
        OrderMode::SavePreview { .. } => save_preview(order, account_hash, command_label).await,
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
    order: &schwab::OrderBuilder,
    account_hash: &str,
    command_label: &str,
) -> Result<Value, AppError> {
    let token = bearer_token().await?;
    let http = reqwest::Client::new();
    let preview = crate::raw::preview_order_with_client(&http, &token, account_hash, order).await?;
    let order_json = serde_json::to_value(order)?;
    let digest = crate::order::preview::save_preview(account_hash, order, command_label)?;
    let warnings = crate::raw::preview_warnings(&preview);

    preview_output(order_json, Some(digest), warnings)
}

/// Previews an order, places it, and returns post-place verification.
async fn preview_first(
    client: &schwab::Client,
    order: &schwab::OrderBuilder,
    account_hash: &str,
) -> Result<Value, AppError> {
    let token = bearer_token().await?;
    let http = reqwest::Client::new();
    let _preview =
        crate::raw::preview_order_with_client(&http, &token, account_hash, order).await?;
    place_order(client, order, account_hash).await
}

/// Builds the accepted preview output payload.
fn preview_output(
    order: Value,
    digest: Option<String>,
    warnings: Vec<crate::raw::PreviewWarning>,
) -> Result<Value, AppError> {
    let summary = preview_summary(&order);
    let mut data = json!({
        "order": order,
        "preview": "accepted",
    });

    if let Some(summary) = summary {
        data["summary"] = Value::String(summary);
    }

    if let Some(digest) = digest {
        data["digest"] = Value::String(digest);
        data["digest_ttl_seconds"] = Value::Number(900.into());
    }

    if !warnings.is_empty() {
        data["warnings"] = serde_json::to_value(warnings)?;
    }

    Ok(data)
}

/// Builds a human-readable preview summary for Schwab order JSON.
fn preview_summary(order: &Value) -> Option<String> {
    let mut lines = Vec::new();
    summarize_order(order, "Order", None, &mut lines);

    if lines.is_empty() {
        None
    } else {
        Some(format!("Preview accepted:\n  {}", lines.join("\n  ")))
    }
}

fn summarize_order(order: &Value, label: &str, relation: Option<&str>, lines: &mut Vec<String>) {
    let strategy = string_field(order, "orderStrategyType")
        .unwrap_or("SINGLE")
        .to_ascii_uppercase();

    if strategy == "OCO" {
        summarize_oco(order, label, relation, lines);
        return;
    }

    if let Some(legs) = leg_summaries(order) {
        let leg_label = if strategy == "TRIGGER" && label == "Order" {
            "Parent"
        } else {
            label
        };
        lines.push(format_summary_line(leg_label, &legs, relation));
    }

    let child_relation = if strategy == "TRIGGER" {
        Some("activates on parent fill")
    } else {
        relation
    };

    if let Some(children) = child_orders(order) {
        for (index, child) in children.iter().enumerate() {
            let child_label = format!("Child {}", index + 1);
            summarize_order(child, &child_label, child_relation, lines);
        }
    }
}

fn summarize_oco(order: &Value, label: &str, relation: Option<&str>, lines: &mut Vec<String>) {
    if let Some(children) = child_orders(order) {
        for (index, child) in children.iter().enumerate() {
            let branch_label = if label == "Order" || label.starts_with("Child ") {
                format!("OCO branch {}", index + 1)
            } else {
                format!("{label} / OCO branch {}", index + 1)
            };
            let branch_relation = combine_relations(relation, "one-cancels-other branch");
            summarize_order(child, &branch_label, branch_relation.as_deref(), lines);
        }
        return;
    }

    if let Some(legs) = leg_summaries(order) {
        let branch_relation = combine_relations(relation, "one-cancels-other branch");
        lines.push(format_summary_line(
            label,
            &legs,
            branch_relation.as_deref(),
        ));
    }
}

fn leg_summaries(order: &Value) -> Option<String> {
    let legs = order
        .get("orderLegCollection")
        .and_then(Value::as_array)?
        .iter()
        .map(|leg| leg_summary(order, leg))
        .collect::<Vec<_>>();

    if legs.is_empty() {
        None
    } else {
        Some(legs.join(" + "))
    }
}

fn leg_summary(order: &Value, leg: &Value) -> String {
    let instruction = string_field(leg, "instruction").unwrap_or("UNKNOWN");
    let quantity = number_field(leg, "quantity").unwrap_or_else(|| "?".to_string());
    let symbol = leg
        .get("instrument")
        .and_then(|instrument| string_field(instrument, "symbol"))
        .map(render_symbol)
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let order_type = string_field(order, "orderType").unwrap_or("UNKNOWN");
    let duration = string_field(order, "duration").unwrap_or("UNKNOWN");
    let price = price_summary(order, order_type);

    format!("{instruction} {quantity} {symbol} {order_type}{price} {duration}")
}

fn price_summary(order: &Value, order_type: &str) -> String {
    match order_type {
        "LIMIT" => money_field(order, "price")
            .map(|price| format!(" @ {price}"))
            .unwrap_or_default(),
        "STOP" => money_field(order, "stopPrice")
            .map(|price| format!(" @ {price}"))
            .unwrap_or_default(),
        "STOP_LIMIT" => match (money_field(order, "price"), money_field(order, "stopPrice")) {
            (Some(price), Some(stop)) => format!(" @ {price} stop {stop}"),
            (Some(price), None) => format!(" @ {price}"),
            (None, Some(stop)) => format!(" stop {stop}"),
            (None, None) => String::new(),
        },
        _ => String::new(),
    }
}

fn format_summary_line(label: &str, legs: &str, relation: Option<&str>) -> String {
    match relation {
        Some(relation) => format!("{label}: {legs} ({relation})"),
        None => format!("{label}: {legs}"),
    }
}

fn combine_relations(existing: Option<&str>, addition: &str) -> Option<String> {
    match existing {
        Some(existing) => Some(format!("{existing}, {addition}")),
        None => Some(addition.to_string()),
    }
}

fn child_orders(order: &Value) -> Option<&Vec<Value>> {
    order.get("childOrderStrategies").and_then(Value::as_array)
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn number_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).map(number_value)
}

fn money_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .map(|number| format!("${}", number_value(number)))
}

fn number_value(value: &Value) -> String {
    match value {
        Value::Number(number) => number
            .as_f64()
            .map(format_decimal)
            .unwrap_or_else(|| number.to_string()),
        Value::String(value) => value.clone(),
        _ => "?".to_string(),
    }
}

fn format_decimal(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        let value = format!("{value:.4}");
        value
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn render_symbol(symbol: &str) -> String {
    decode_occ_symbol(symbol).unwrap_or_else(|| symbol.to_string())
}

fn decode_occ_symbol(symbol: &str) -> Option<String> {
    if symbol.len() != 21 {
        return None;
    }

    let underlying = symbol.get(0..6)?.trim();
    let date = symbol.get(6..12)?;
    let side = symbol.get(12..13)?;
    let strike = symbol.get(13..21)?.parse::<u64>().ok()?;
    let month = month_name(date.get(2..4)?.parse::<u8>().ok()?)?;
    let day = date.get(4..6)?.parse::<u8>().ok()?;
    let option_type = match side {
        "C" => "CALL",
        "P" => "PUT",
        _ => return None,
    };
    let strike = format_decimal(strike as f64 / 1000.0);

    Some(format!(
        "{underlying} {month}{day}'{} {strike} {option_type}",
        date.get(0..2)?
    ))
}

fn month_name(month: u8) -> Option<&'static str> {
    match month {
        1 => Some("Jan"),
        2 => Some("Feb"),
        3 => Some("Mar"),
        4 => Some("Apr"),
        5 => Some("May"),
        6 => Some("Jun"),
        7 => Some("Jul"),
        8 => Some("Aug"),
        9 => Some("Sep"),
        10 => Some("Oct"),
        11 => Some("Nov"),
        12 => Some("Dec"),
        _ => None,
    }
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
    account: &str,
    json_str: &str,
    save: bool,
    command_label: &str,
) -> Result<Value, AppError> {
    let order: Value = serde_json::from_str(json_str)
        .map_err(|e| AppError::OrderValidation(format!("invalid JSON: {e}")))?;
    let account_hash = resolve_account_hash(account).await?;

    execute_raw_preview_with_account_hash(&account_hash, order, save, command_label).await
}

/// Previews an already-parsed raw JSON order for a resolved account hash.
async fn execute_raw_preview_with_account_hash(
    account_hash: &str,
    order: Value,
    save: bool,
    command_label: &str,
) -> Result<Value, AppError> {
    let token = bearer_token().await?;
    let http = reqwest::Client::new();
    let preview =
        crate::raw::preview_order_with_client(&http, &token, account_hash, &order).await?;
    let warnings = crate::raw::preview_warnings(&preview);
    let digest = if save {
        Some(save_preview_digest(account_hash, &order, command_label)?)
    } else {
        None
    };

    preview_output(order, digest, warnings)
}

fn save_preview_digest(
    account_hash: &str,
    order: &Value,
    command_label: &str,
) -> Result<String, AppError> {
    crate::order::preview::save_preview(account_hash, order, command_label)
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
    use std::ffi::OsString;
    use std::future::Future;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::thread::JoinHandle;

    use schwab::auth::{TokenData, TokenFile};

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
        let err = execute_raw_preview("HASH", "{not json", false, "order preview-raw")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("invalid JSON"));
    }

    #[test]
    fn preview_output_without_digest_or_warnings_is_minimal() {
        let output = preview_output(json!({"orderType": "LIMIT"}), None, Vec::new()).unwrap();

        assert_eq!(output["preview"], "accepted");
        assert_eq!(output["order"]["orderType"], "LIMIT");
        assert!(output.get("digest").is_none());
        assert!(output.get("digest_ttl_seconds").is_none());
        assert!(output.get("summary").is_none());
        assert!(output.get("warnings").is_none());
    }

    #[test]
    fn preview_output_summarizes_single_equity_leg() {
        let output = preview_output(
            json!({
                "orderType": "LIMIT",
                "duration": "DAY",
                "price": 150.25,
                "orderLegCollection": [{
                    "instruction": "BUY",
                    "quantity": 10,
                    "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
                }]
            }),
            None,
            Vec::new(),
        )
        .unwrap();

        assert_eq!(
            output["summary"],
            "Preview accepted:\n  Order: BUY 10 AAPL LIMIT @ $150.25 DAY"
        );
    }

    #[test]
    fn preview_output_summarizes_trigger_parent_and_child() {
        let output = preview_output(
            json!({
                "orderStrategyType": "TRIGGER",
                "orderType": "LIMIT",
                "duration": "DAY",
                "price": 150.0,
                "orderLegCollection": [{
                    "instruction": "BUY",
                    "quantity": 10,
                    "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
                }],
                "childOrderStrategies": [{
                    "orderStrategyType": "SINGLE",
                    "orderType": "STOP",
                    "duration": "GOOD_TILL_CANCEL",
                    "stopPrice": 140.0,
                    "orderLegCollection": [{
                        "instruction": "SELL",
                        "quantity": 10,
                        "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
                    }]
                }]
            }),
            None,
            Vec::new(),
        )
        .unwrap();

        assert_eq!(
            output["summary"],
            "Preview accepted:\n  Parent: BUY 10 AAPL LIMIT @ $150 DAY\n  Child 1: SELL 10 AAPL STOP @ $140 GOOD_TILL_CANCEL (activates on parent fill)"
        );
    }

    #[test]
    fn preview_output_summarizes_oco_branches() {
        let output = preview_output(
            json!({
                "orderStrategyType": "OCO",
                "childOrderStrategies": [
                    {
                        "orderStrategyType": "SINGLE",
                        "orderType": "LIMIT",
                        "duration": "GOOD_TILL_CANCEL",
                        "price": 155.0,
                        "orderLegCollection": [{
                            "instruction": "SELL",
                            "quantity": 10,
                            "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
                        }]
                    },
                    {
                        "orderStrategyType": "SINGLE",
                        "orderType": "STOP",
                        "duration": "GOOD_TILL_CANCEL",
                        "stopPrice": 140.0,
                        "orderLegCollection": [{
                            "instruction": "SELL",
                            "quantity": 10,
                            "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
                        }]
                    }
                ]
            }),
            None,
            Vec::new(),
        )
        .unwrap();

        assert_eq!(
            output["summary"],
            "Preview accepted:\n  OCO branch 1: SELL 10 AAPL LIMIT @ $155 GOOD_TILL_CANCEL (one-cancels-other branch)\n  OCO branch 2: SELL 10 AAPL STOP @ $140 GOOD_TILL_CANCEL (one-cancels-other branch)"
        );
    }

    #[test]
    fn preview_output_summarizes_trigger_parent_with_oco_children() {
        let output = preview_output(
            json!({
                "orderStrategyType": "TRIGGER",
                "orderType": "LIMIT",
                "duration": "DAY",
                "price": 180.0,
                "orderLegCollection": [{
                    "instruction": "BUY",
                    "quantity": 100,
                    "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
                }],
                "childOrderStrategies": [{
                    "orderStrategyType": "OCO",
                    "childOrderStrategies": [
                        {
                            "orderStrategyType": "SINGLE",
                            "orderType": "LIMIT",
                            "duration": "GOOD_TILL_CANCEL",
                            "price": 200.0,
                            "orderLegCollection": [{
                                "instruction": "SELL",
                                "quantity": 100,
                                "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
                            }]
                        },
                        {
                            "orderStrategyType": "SINGLE",
                            "orderType": "STOP",
                            "duration": "GOOD_TILL_CANCEL",
                            "stopPrice": 170.0,
                            "orderLegCollection": [{
                                "instruction": "SELL",
                                "quantity": 100,
                                "instrument": {"symbol": "AAPL", "assetType": "EQUITY"}
                            }]
                        }
                    ]
                }]
            }),
            None,
            Vec::new(),
        )
        .unwrap();

        assert_eq!(
            output["summary"],
            "Preview accepted:\n  Parent: BUY 100 AAPL LIMIT @ $180 DAY\n  OCO branch 1: SELL 100 AAPL LIMIT @ $200 GOOD_TILL_CANCEL (activates on parent fill, one-cancels-other branch)\n  OCO branch 2: SELL 100 AAPL STOP @ $170 GOOD_TILL_CANCEL (activates on parent fill, one-cancels-other branch)"
        );
    }

    #[test]
    fn preview_output_decodes_option_occ_symbols() {
        let output = preview_output(
            json!({
                "orderType": "LIMIT",
                "duration": "DAY",
                "price": 5.5,
                "orderLegCollection": [{
                    "instruction": "BUY_TO_OPEN",
                    "quantity": 1,
                    "instrument": {"symbol": "AAPL  260120C00100000", "assetType": "OPTION"}
                }]
            }),
            None,
            Vec::new(),
        )
        .unwrap();

        assert_eq!(
            output["summary"],
            "Preview accepted:\n  Order: BUY_TO_OPEN 1 AAPL Jan20'26 100 CALL LIMIT @ $5.5 DAY"
        );
    }

    #[test]
    fn preview_output_includes_digest_and_sanitized_warnings() {
        let warnings = vec![crate::raw::PreviewWarning {
            code: "order.preview_warning",
            severity: "WARN".to_string(),
            message: Some("Review stop risk.".to_string()),
            activity_message: None,
            validation_rule_name: Some("STOP_ORDER_RISK".to_string()),
        }];

        let output = preview_output(
            json!({"orderType": "STOP"}),
            Some("abc123".to_string()),
            warnings,
        )
        .unwrap();

        assert_eq!(output["preview"], "accepted");
        assert_eq!(output["digest"], "abc123");
        assert_eq!(output["digest_ttl_seconds"], 900);
        assert_eq!(output["warnings"][0]["code"], "order.preview_warning");
        assert_eq!(output["warnings"][0]["severity"], "WARN");
        assert_eq!(output["warnings"][0]["message"], "Review stop risk.");
        assert!(output["warnings"][0].get("activityMessage").is_none());
        assert_eq!(
            output["warnings"][0]["validationRuleName"],
            "STOP_ORDER_RISK"
        );
    }

    #[test]
    fn execute_order_with_account_hash_save_preview_uses_raw_preview_warnings() {
        let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let _env = setup_auth_env(temp_dir.path());
        let (base_url, requests) = spawn_json_sequence(vec![(
            "/accounts/HASH123/previewOrder",
            "HTTP/1.1 200 OK",
            r#"{"orderValidationResult":{"warns":[{"originalSeverity":"WARN","message":"Review stop risk."}]}}"#,
        )]);
        let _preview_url =
            crate::raw::set_preview_order_url_prefix_for_tests(format!("{base_url}/accounts"));
        let client = schwab::Client::new(schwab::Config::new().bearer_token("TOKEN123"));
        let order = test_order();

        let output = run_async(execute_order_with_account_hash(
            &client,
            &order,
            OrderMode::SavePreview {
                account: "HASH123".to_string(),
            },
            "HASH123",
            "order equity buy",
        ))
        .unwrap();

        assert_eq!(output["preview"], "accepted");
        assert_eq!(output["warnings"][0]["severity"], "WARN");
        assert_eq!(output["warnings"][0]["message"], "Review stop risk.");
        assert!(
            output["digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty())
        );
        assert_eq!(output["digest_ttl_seconds"], 900);

        let requests = requests.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("authorization: Bearer TOKEN123"));
        assert!(requests[0].contains("POST /accounts/HASH123/previewOrder HTTP/1.1"));
        assert!(requests[0].contains("\"orderType\":\"MARKET\""));
    }

    #[test]
    fn preview_first_reuses_raw_preview_client_before_placing_order() {
        let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let _env = setup_auth_env(temp_dir.path());
        let (base_url, requests) = spawn_json_sequence(vec![
            (
                "/accounts/HASH123/previewOrder",
                "HTTP/1.1 200 OK",
                r#"{"orderValidationResult":{"warns":[]}}"#,
            ),
            ("/accounts/HASH123/orders", "HTTP/1.1 201 Created", ""),
        ]);
        let _preview_url =
            crate::raw::set_preview_order_url_prefix_for_tests(format!("{base_url}/accounts"));
        let client = schwab::Client::new(
            schwab::Config::new()
                .bearer_token("TOKEN123")
                .trader_base_url(&base_url)
                .unwrap(),
        );
        let order = test_order();

        let output = run_async(preview_first(&client, &order, "HASH123")).unwrap();

        assert_eq!(output["action"], "place");
        assert_eq!(output["verification_state"], "unverified");
        assert_eq!(
            output["verification_failures"][0],
            "no order ID returned by API"
        );

        let requests = requests.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("POST /accounts/HASH123/previewOrder HTTP/1.1"));
        assert!(requests[1].contains("POST /accounts/HASH123/orders HTTP/1.1"));
    }

    #[test]
    fn execute_raw_preview_with_account_hash_returns_sanitized_output() {
        let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let _previous_client_id = EnvVarGuard::set("SCHWAB_CLIENT_ID", "previous-client-id");
        let _env = setup_auth_env(temp_dir.path());
        let (base_url, requests) = spawn_json_sequence(vec![(
            "/accounts/HASH123/previewOrder",
            "HTTP/1.1 200 OK",
            r#"{"orderValidationResult":{"warns":[{"overrideSeverity":"WARN","activityMessage":"Stop may trigger."}]}}"#,
        )]);
        let _preview_url =
            crate::raw::set_preview_order_url_prefix_for_tests(format!("{base_url}/accounts"));

        let output = run_async(execute_raw_preview_with_account_hash(
            "HASH123",
            json!({"orderType": "STOP"}),
            true,
            "order preview-raw",
        ))
        .unwrap();

        assert_eq!(output["preview"], "accepted");
        assert_eq!(output["order"]["orderType"], "STOP");
        assert_eq!(
            output["warnings"][0]["activityMessage"],
            "Stop may trigger."
        );
        assert!(
            output["digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty())
        );
        assert_eq!(output["digest_ttl_seconds"], 900);

        let requests = requests.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains(r#"{"orderType":"STOP"}"#));
    }

    #[test]
    fn execute_raw_preview_rejects_invalid_json_before_auth() {
        let err = run_async(execute_raw_preview(
            "HASH123",
            "{",
            false,
            "order preview-raw",
        ))
        .unwrap_err();

        assert_eq!(err.code(), "order.validation_failed");
        assert!(err.to_string().contains("invalid JSON"));
    }

    #[test]
    fn execute_order_save_preview_resolves_account_and_uses_raw_preview() {
        let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let _env = setup_auth_env(temp_dir.path());
        let (base_url, requests) = spawn_mock_schwab_server(3);
        let _raw_url = crate::raw::set_raw_url_prefix_for_tests(base_url.clone());
        let _preview_url =
            crate::raw::set_preview_order_url_prefix_for_tests(format!("{base_url}/accounts"));
        let client = schwab::Client::new(schwab::Config::new().bearer_token("TOKEN123"));
        let order = test_order();

        let output = run_async(execute_order(
            &client,
            &order,
            OrderMode::SavePreview {
                account: "HASH123".to_string(),
            },
            "order equity buy",
        ))
        .unwrap();

        assert_eq!(output["preview"], "accepted");
        assert!(
            output["digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty())
        );
        assert_eq!(output["warnings"][0]["severity"], "WARN");

        let requests = requests.join().unwrap();
        assert!(
            requests
                .iter()
                .any(|request| request.contains("GET /accounts/accountNumbers"))
        );
        assert!(
            requests
                .iter()
                .any(|request| request.contains("GET /userPreference"))
        );
        assert!(
            requests.iter().any(|request| {
                request.contains("POST /accounts/HASH123/previewOrder HTTP/1.1")
            })
        );
    }

    #[test]
    fn execute_raw_preview_resolves_account_and_saves_digest() {
        let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let _env = setup_auth_env(temp_dir.path());
        let (base_url, requests) = spawn_mock_schwab_server(3);
        let _raw_url = crate::raw::set_raw_url_prefix_for_tests(base_url.clone());
        let _preview_url =
            crate::raw::set_preview_order_url_prefix_for_tests(format!("{base_url}/accounts"));

        let output = run_async(execute_raw_preview(
            "HASH123",
            r#"{"orderType":"STOP"}"#,
            true,
            "order preview-raw",
        ))
        .unwrap();

        assert_eq!(output["preview"], "accepted");
        assert_eq!(output["digest_ttl_seconds"], 900);
        assert!(
            output["digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty())
        );

        let requests = requests.join().unwrap();
        assert!(
            requests.iter().any(|request| {
                request.contains("POST /accounts/HASH123/previewOrder HTTP/1.1")
            })
        );
    }

    #[test]
    fn execute_raw_preview_with_account_hash_can_skip_saving_digest() {
        let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let _env = setup_auth_env(temp_dir.path());
        let (base_url, requests) = spawn_json_sequence(vec![(
            "/accounts/HASH123/previewOrder",
            "HTTP/1.1 200 OK",
            r#"{"orderValidationResult":{"warns":[]}}"#,
        )]);
        let _preview_url =
            crate::raw::set_preview_order_url_prefix_for_tests(format!("{base_url}/accounts"));

        let output = run_async(execute_raw_preview_with_account_hash(
            "HASH123",
            json!({"orderType": "STOP"}),
            false,
            "order preview-raw",
        ))
        .unwrap();

        assert_eq!(output["preview"], "accepted");
        assert!(output.get("digest").is_none());
        assert_eq!(requests.join().unwrap().len(), 1);
    }

    #[test]
    fn place_from_saved_preview_includes_digest_context() {
        let _lock = crate::config::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let _env = setup_auth_env(temp_dir.path());
        write_mutable_config(temp_dir.path());
        let order = test_order();
        let digest =
            crate::order::preview::save_preview("HASH123", &order, "order equity buy").unwrap();
        let (base_url, requests) = spawn_mock_schwab_server(3);
        let _raw_url = crate::raw::set_raw_url_prefix_for_tests(base_url.clone());
        let client = schwab::Client::new(
            schwab::Config::new()
                .bearer_token("TOKEN123")
                .trader_base_url(&base_url)
                .unwrap(),
        );

        let output = run_async(place_from_saved_preview(&client, "HASH123", &digest)).unwrap();

        assert_eq!(output["action"], "place");
        assert_eq!(output["digest"], digest);
        assert_eq!(output["original_command"], "order equity buy");
        assert_eq!(output["verification_state"], "unverified");

        let requests = requests.join().unwrap();
        assert!(
            requests
                .iter()
                .any(|request| request.contains("POST /accounts/HASH123/orders"))
        );
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    fn setup_auth_env(root: &Path) -> Vec<EnvVarGuard> {
        let token_path = root.join("token.json");
        let state_path = root.join("state");
        let config_path = root.join("config");
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let token_file = TokenFile {
            creation_timestamp: now,
            token: TokenData {
                access_token: "TOKEN123".to_string(),
                token_type: Some("Bearer".to_string()),
                expires_in: Some(3_600),
                refresh_token: Some("REFRESH123".to_string()),
                scope: Some("readonly".to_string()),
                expires_at: Some(now + 3_600),
            },
        };
        std::fs::write(&token_path, serde_json::to_vec(&token_file).unwrap()).unwrap();

        vec![
            EnvVarGuard::set_path("SCHWAB_TOKEN_PATH", &token_path),
            EnvVarGuard::set_path("XDG_STATE_HOME", &state_path),
            EnvVarGuard::set_path("XDG_CONFIG_HOME", &config_path),
            EnvVarGuard::set("SCHWAB_CLIENT_ID", "client-id"),
            EnvVarGuard::set("SCHWAB_CLIENT_SECRET", "client-secret"),
            EnvVarGuard::set("SCHWAB_CALLBACK_URL", "https://127.0.0.1:8182"),
        ]
    }

    fn write_mutable_config(root: &Path) {
        let config_dir = root.join("config").join("schwab-agent");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.json"),
            r#"{"i-also-like-to-live-dangerously": true}"#,
        )
        .unwrap();
    }

    fn test_order() -> schwab::OrderBuilder {
        schwab::OrderBuilder::market_buy("AAPL", crate::shared::to_number(1.0).unwrap())
    }

    fn run_async<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn spawn_json_sequence(
        responses: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            responses
                .into_iter()
                .map(|(path, status_line, body)| {
                    let (mut stream, _) = listener.accept().unwrap();
                    let request = read_http_request(&mut stream);
                    assert!(request.starts_with(&format!("POST {path} HTTP/1.1")));

                    let response = format!(
                        "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).unwrap();

                    request
                })
                .collect()
        });

        (base_url, handle)
    }

    fn spawn_mock_schwab_server(request_count: usize) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            (0..request_count)
                .map(|_| {
                    let (mut stream, _) = listener.accept().unwrap();
                    let request = read_http_request(&mut stream);
                    let (status_line, body) = mock_schwab_response(&request);
                    let response = format!(
                        "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    request
                })
                .collect()
        });

        (base_url, handle)
    }

    fn mock_schwab_response(request: &str) -> (&'static str, &'static str) {
        if request.starts_with("GET /accounts/accountNumbers") {
            return (
                "HTTP/1.1 200 OK",
                r#"[{"accountNumber":"A1","hashValue":"HASH123"}]"#,
            );
        }
        if request.starts_with("GET /userPreference") {
            return ("HTTP/1.1 200 OK", "[]");
        }
        if request.starts_with("GET /accounts ") || request.starts_with("GET /accounts?") {
            return (
                "HTTP/1.1 200 OK",
                r#"[{"securitiesAccount":{"type":"MARGIN","accountNumber":"A1"}}]"#,
            );
        }
        if request.starts_with("POST /accounts/HASH123/previewOrder") {
            return (
                "HTTP/1.1 200 OK",
                r#"{"orderValidationResult":{"warns":[{"originalSeverity":"WARN","message":"Stop may trigger."}]}}"#,
            );
        }
        if request.starts_with("POST /accounts/HASH123/orders") {
            return ("HTTP/1.1 201 Created", "{}");
        }

        panic!("unexpected request: {request}");
    }

    #[test]
    fn mock_schwab_response_covers_accounts_and_place_branches() {
        assert_eq!(
            mock_schwab_response("GET /accounts HTTP/1.1").1,
            r#"[{"securitiesAccount":{"type":"MARGIN","accountNumber":"A1"}}]"#
        );
        assert_eq!(
            mock_schwab_response("POST /accounts/HASH123/orders HTTP/1.1"),
            ("HTTP/1.1 201 Created", "{}")
        );
    }

    #[test]
    #[should_panic(expected = "unexpected request")]
    fn mock_schwab_response_rejects_unexpected_requests() {
        let _ = mock_schwab_response("GET /unexpected HTTP/1.1");
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0; 16];

        loop {
            let read = stream.read(&mut buffer).unwrap();
            assert_ne!(read, 0, "client closed before headers were complete");
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }

        let headers_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let headers = String::from_utf8_lossy(&request[..headers_end]).to_ascii_lowercase();
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or_default();

        while request.len() - headers_end < content_length {
            let read = stream.read(&mut buffer).unwrap();
            assert_ne!(read, 0, "client closed before body was complete");
            request.extend_from_slice(&buffer[..read]);
        }

        String::from_utf8(request).unwrap()
    }
}
