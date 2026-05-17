//! Raw Schwab API requests with response normalization.
//!
//! The `schwab` crate deserializes API responses directly into typed structs,
//! which fails when the Schwab API returns unexpected formats. This module
//! bypasses the crate's typed deserialization to normalize two known quirks:
//!
//! - **Object-wrapped arrays** (GitHub #17): the accounts endpoint sometimes
//!   returns `{"key": [...]}` instead of a bare `[...]`.
//! - **Boolean `false` for absent numerics** (GitHub #18): some numeric fields
//!   serialize as `false` instead of `null` or `0`.

use schwab::Account;
use serde_json::Value;

use crate::error::AppError;

/// Schwab Trader API accounts endpoint.
const ACCOUNTS_URL: &str = "https://api.schwabapi.com/trader/v1/accounts";

/// Fetches accounts from the Schwab API with response normalization.
///
/// Makes a direct HTTP request (bypassing the `schwab` crate's typed
/// deserialization) so the response JSON can be normalized before parsing.
///
/// # Errors
///
/// Returns an error when the HTTP request fails, the server returns a
/// non-success status, or the normalized JSON cannot be deserialized into
/// `Vec<Account>`.
pub async fn fetch_accounts(
    bearer_token: &str,
    fields: Option<&str>,
) -> Result<Vec<Account>, AppError> {
    let http = reqwest::Client::new();
    let mut request = http.get(ACCOUNTS_URL).bearer_auth(bearer_token);

    if let Some(fields) = fields {
        request = request.query(&[("fields", fields)]);
    }

    let response = request.send().await.map_err(schwab::Error::Request)?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.map_err(schwab::Error::Request)?;
        return Err(schwab::Error::HttpStatus { status, body }.into());
    }

    let text = response.text().await.map_err(schwab::Error::Request)?;
    let value: Value = serde_json::from_str(&text)?;

    let array = unwrap_accounts_array(value);
    let normalized = normalize_false_to_null(array);

    Ok(serde_json::from_value(normalized)?)
}

/// Extracts the accounts array from a potential object wrapper.
///
/// The Schwab accounts endpoint sometimes returns a single-key object wrapper
/// around the array instead of a bare JSON array. This function handles both:
///
/// - Already an array: returned unchanged.
/// - Single-key object whose value is an array: the inner array is extracted.
/// - Anything else: returned as-is for the deserializer to report a type error.
#[must_use]
fn unwrap_accounts_array(value: Value) -> Value {
    match &value {
        Value::Array(_) => value,
        Value::Object(map) if map.len() == 1 => {
            // Safe: we just checked len() == 1.
            let inner = map.values().next().unwrap();
            if inner.is_array() {
                inner.clone()
            } else {
                value
            }
        }
        _ => value,
    }
}

/// Recursively replaces `false` with `null` throughout a JSON value tree.
///
/// The Schwab API sometimes serializes absent or zero numeric fields as boolean
/// `false` instead of `null` or `0`. Since the `schwab` crate types use
/// `Option<Number>` for these fields, `false` causes a deserialization error.
///
/// Replacing all `false` values with `null` also affects legitimate boolean
/// fields (e.g., `is_day_trader`, `is_closing_only_restricted`), turning
/// `Some(false)` into `None`. This is an acceptable trade-off because `None`
/// carries the same practical meaning as `false` for these account flags.
///
/// Boolean `true` values are preserved unchanged.
#[must_use]
fn normalize_false_to_null(value: Value) -> Value {
    match value {
        Value::Bool(false) => Value::Null,
        Value::Array(items) => {
            Value::Array(items.into_iter().map(normalize_false_to_null).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, normalize_false_to_null(v)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // --- unwrap_accounts_array ---

    #[test]
    fn unwrap_bare_array_unchanged() {
        let input = json!([{"a": 1}, {"b": 2}]);
        assert_eq!(unwrap_accounts_array(input.clone()), input);
    }

    #[test]
    fn unwrap_single_key_object_extracts_array() {
        let inner = json!([{"a": 1}]);
        let wrapped = json!({"accounts": inner});
        assert_eq!(unwrap_accounts_array(wrapped), inner);
    }

    #[test]
    fn unwrap_single_key_non_array_returns_as_is() {
        let input = json!({"key": "not-an-array"});
        assert_eq!(unwrap_accounts_array(input.clone()), input);
    }

    #[test]
    fn unwrap_multi_key_object_returns_as_is() {
        let input = json!({"a": [1], "b": [2]});
        assert_eq!(unwrap_accounts_array(input.clone()), input);
    }

    #[test]
    fn unwrap_scalar_returns_as_is() {
        let input = json!("just a string");
        assert_eq!(unwrap_accounts_array(input.clone()), input);
    }

    // --- normalize_false_to_null ---

    #[test]
    fn normalize_false_becomes_null() {
        assert_eq!(normalize_false_to_null(json!(false)), Value::Null);
    }

    #[test]
    fn normalize_true_preserved() {
        assert_eq!(normalize_false_to_null(json!(true)), json!(true));
    }

    #[test]
    fn normalize_replaces_false_in_object() {
        let input = json!({"balance": false, "name": "test"});
        let expected = json!({"balance": null, "name": "test"});
        assert_eq!(normalize_false_to_null(input), expected);
    }

    #[test]
    fn normalize_preserves_true_in_object() {
        let input = json!({"is_day_trader": true, "balance": false});
        let expected = json!({"is_day_trader": true, "balance": null});
        assert_eq!(normalize_false_to_null(input), expected);
    }

    #[test]
    fn normalize_handles_nested_structures() {
        let input = json!({
            "outer": {"inner": false, "value": 42},
            "list": [false, true, 1, "text"]
        });
        let expected = json!({
            "outer": {"inner": null, "value": 42},
            "list": [null, true, 1, "text"]
        });
        assert_eq!(normalize_false_to_null(input), expected);
    }

    #[test]
    fn normalize_deeply_nested_false() {
        let input = json!([{"account": {"balances": {"equity": false, "cash": 100.0}}}]);
        let expected = json!([{"account": {"balances": {"equity": null, "cash": 100.0}}}]);
        assert_eq!(normalize_false_to_null(input), expected);
    }

    #[test]
    fn normalize_preserves_existing_nulls() {
        let input = json!({"a": null, "b": false, "c": true});
        let expected = json!({"a": null, "b": null, "c": true});
        assert_eq!(normalize_false_to_null(input), expected);
    }

    #[test]
    fn normalize_empty_structures() {
        assert_eq!(normalize_false_to_null(json!({})), json!({}));
        assert_eq!(normalize_false_to_null(json!([])), json!([]));
    }

    // --- full pipeline ---

    #[test]
    fn pipeline_unwrap_then_normalize() {
        let wrapped = json!({
            "accounts": [{
                "securitiesAccount": {
                    "balance": false,
                    "is_active": true,
                    "equity": 1000
                }
            }]
        });
        let unwrapped = unwrap_accounts_array(wrapped);
        let normalized = normalize_false_to_null(unwrapped);
        assert_eq!(
            normalized,
            json!([{
                "securitiesAccount": {
                    "balance": null,
                    "is_active": true,
                    "equity": 1000
                }
            }])
        );
    }
}
