//! Raw Schwab API requests with response normalization.
//!
//! The `schwab` crate deserializes API responses directly into typed structs,
//! which fails when the Schwab API returns unexpected formats. This module
//! bypasses the crate's typed deserialization to normalize known quirks:
//!
//! - **Object-wrapped arrays** (GitHub #17): account endpoints sometimes
//!   return `{"key": [...]}` instead of a bare `[...]`.
//! - **Bare user preference objects** (GitHub #46): the user preference endpoint
//!   can return a single object where the typed crate expects a one-item array.
//! - **Boolean `false` for absent numerics** (GitHub #18): some numeric fields
//!   serialize as `false` instead of `null` or `0`.

use schwab::{Account, AccountNumberHash, UserPreference};
use serde_json::Value;

use crate::error::AppError;

/// Schwab Trader API accounts endpoint.
const ACCOUNTS_URL: &str = "https://api.schwabapi.com/trader/v1/accounts";

/// Schwab Trader API account number hash endpoint.
const ACCOUNT_NUMBERS_URL: &str = "https://api.schwabapi.com/trader/v1/accounts/accountNumbers";

/// Schwab Trader API user preference endpoint.
const USER_PREFERENCE_URL: &str = "https://api.schwabapi.com/trader/v1/userPreference";

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

/// Fetches account number hashes from the Schwab API with response normalization.
///
/// Makes a direct HTTP request so object-wrapped account metadata responses can
/// be normalized before deserializing into `Vec<AccountNumberHash>`.
///
/// # Errors
///
/// Returns an error when the HTTP request fails, the server returns a
/// non-success status, or the normalized JSON cannot be deserialized into
/// `Vec<AccountNumberHash>`.
pub async fn fetch_account_numbers(bearer_token: &str) -> Result<Vec<AccountNumberHash>, AppError> {
    let value = fetch_json(ACCOUNT_NUMBERS_URL, bearer_token).await?;
    let array = unwrap_array_fields(value, &["accounts", "accountNumbers"]);

    Ok(serde_json::from_value(array)?)
}

/// Fetches user preferences from the Schwab API with response normalization.
///
/// The `schwab` crate expects a sequence, but Schwab can return the preference
/// object directly. This wraps a bare object as a one-item array while still
/// accepting the historical array and named wrapper forms.
///
/// # Errors
///
/// Returns an error when the HTTP request fails, the server returns a
/// non-success status, or the normalized JSON cannot be deserialized into
/// `Vec<UserPreference>`.
pub async fn fetch_user_preference(bearer_token: &str) -> Result<Vec<UserPreference>, AppError> {
    let value = fetch_json(USER_PREFERENCE_URL, bearer_token).await?;
    let array = normalize_user_preference_response(value);

    Ok(serde_json::from_value(array)?)
}

/// Fetches a Schwab API URL as raw JSON using bearer authentication.
async fn fetch_json(url: &str, bearer_token: &str) -> Result<Value, AppError> {
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(bearer_token)
        .send()
        .await
        .map_err(schwab::Error::Request)?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.map_err(schwab::Error::Request)?;
        return Err(schwab::Error::HttpStatus { status, body }.into());
    }

    let text = response.text().await.map_err(schwab::Error::Request)?;

    Ok(serde_json::from_str(&text)?)
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
    unwrap_array_field(value, "accounts")
}

/// Extracts an array from a bare array or any matching object field.
#[must_use]
fn unwrap_array_fields(value: Value, fields: &[&str]) -> Value {
    match &value {
        Value::Object(map) => fields
            .iter()
            .find_map(|field| map.get(*field).filter(|value| value.is_array()).cloned())
            .unwrap_or_else(|| unwrap_array_field(value, "")),
        _ => unwrap_array_field(value, ""),
    }
}

/// Extracts an array from a bare array or an object field.
///
/// Handles the historical single-key object wrapper plus newer multi-key
/// envelopes that expose the desired array by name.
#[must_use]
fn unwrap_array_field(value: Value, field: &str) -> Value {
    match &value {
        Value::Array(_) => value,
        Value::Object(map) if map.get(field).is_some_and(Value::is_array) => map[field].clone(),
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

/// Normalizes user preference responses into an array.
#[must_use]
fn normalize_user_preference_response(value: Value) -> Value {
    match &value {
        Value::Array(_) => value,
        Value::Object(map)
            if map
                .get("userPreferences")
                .or_else(|| map.get("userPreference"))
                .is_some_and(Value::is_array) =>
        {
            map.get("userPreferences")
                .or_else(|| map.get("userPreference"))
                .cloned()
                .unwrap_or(value)
        }
        Value::Object(map) if map.len() == 1 => {
            // Safe: we just checked len() == 1.
            let inner = map.values().next().unwrap();
            if inner.is_array() {
                inner.clone()
            } else {
                Value::Array(vec![value])
            }
        }
        Value::Object(_) => Value::Array(vec![value]),
        _ => value,
    }
}

/// Known boolean field names in the Schwab API response (camelCase).
///
/// These keys carry legitimate `false` values and must not be converted to
/// `null` during normalization.
const BOOLEAN_FIELDS: &[&str] = &["isDayTrader", "isClosingOnlyRestricted"];

/// Recursively replaces `false` with `null` throughout a JSON value tree,
/// except for keys listed in [`BOOLEAN_FIELDS`].
///
/// The Schwab API sometimes serializes absent or zero numeric fields as boolean
/// `false` instead of `null` or `0`. Since the `schwab` crate types use
/// `Option<Number>` for these fields, `false` causes a deserialization error.
///
/// Known boolean fields are preserved so that `false` deserializes as
/// `Some(false)` rather than collapsing to `None`.
///
/// Boolean `true` values are always preserved unchanged.
#[must_use]
fn normalize_false_to_null(value: Value) -> Value {
    match value {
        Value::Bool(false) => Value::Null,
        Value::Array(items) => {
            Value::Array(items.into_iter().map(normalize_false_to_null).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    if BOOLEAN_FIELDS.contains(&k.as_str()) {
                        (k, v)
                    } else {
                        (k, normalize_false_to_null(v))
                    }
                })
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
    fn unwrap_multi_key_accounts_envelope_extracts_accounts() {
        let inner = json!([{"accountNumber": "A1"}]);
        let input = json!({"accounts": inner, "metadata": {"ignored": true}});
        assert_eq!(unwrap_accounts_array(input), inner);
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
        let input = json!({"isDayTrader": true, "balance": false});
        let expected = json!({"isDayTrader": true, "balance": null});
        assert_eq!(normalize_false_to_null(input), expected);
    }

    #[test]
    fn normalize_preserves_false_for_known_boolean_fields() {
        let input = json!({
            "isDayTrader": false,
            "isClosingOnlyRestricted": false,
            "balance": false,
            "equity": false
        });
        let expected = json!({
            "isDayTrader": false,
            "isClosingOnlyRestricted": false,
            "balance": null,
            "equity": null
        });
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
                    "isDayTrader": false,
                    "isClosingOnlyRestricted": true,
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
                    "isDayTrader": false,
                    "isClosingOnlyRestricted": true,
                    "equity": 1000
                }
            }])
        );
    }

    #[test]
    fn account_numbers_pipeline_deserializes_bare_array() {
        let input = json!([{"accountNumber": "12345678", "hashValue": "HASH123"}]);
        let normalized = unwrap_array_field(input, "accounts");
        let hashes: Vec<AccountNumberHash> = serde_json::from_value(normalized).unwrap();

        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].account_number.as_deref(), Some("12345678"));
        assert_eq!(hashes[0].hash_value.as_deref(), Some("HASH123"));
    }

    #[test]
    fn account_numbers_pipeline_deserializes_accounts_envelope() {
        let input = json!({
            "accounts": [{"accountNumber": "12345678", "hashValue": "HASH123"}],
            "metadata": {"ignored": true}
        });
        let normalized = unwrap_array_field(input, "accounts");
        let hashes: Vec<AccountNumberHash> = serde_json::from_value(normalized).unwrap();

        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].account_number.as_deref(), Some("12345678"));
        assert_eq!(hashes[0].hash_value.as_deref(), Some("HASH123"));
    }

    #[test]
    fn account_numbers_pipeline_deserializes_account_numbers_envelope() {
        let input = json!({
            "accountNumbers": [{"accountNumber": "12345678", "hashValue": "HASH123"}],
            "metadata": {"ignored": true}
        });
        let normalized = unwrap_array_fields(input, &["accounts", "accountNumbers"]);
        let hashes: Vec<AccountNumberHash> = serde_json::from_value(normalized).unwrap();

        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].account_number.as_deref(), Some("12345678"));
        assert_eq!(hashes[0].hash_value.as_deref(), Some("HASH123"));
    }

    #[test]
    fn user_preferences_pipeline_deserializes_bare_array() {
        let input = json!([{
            "accounts": [{
                "accountNumber": "12345678",
                "primaryAccount": true,
                "type": "BROKERAGE",
                "nickName": "Trading",
                "displayAcctId": "...5678"
            }],
            "streamerInfo": []
        }]);
        let normalized = normalize_user_preference_response(input);
        let preferences: Vec<UserPreference> = serde_json::from_value(normalized).unwrap();

        assert_eq!(preferences.len(), 1);
        let accounts = preferences[0].accounts.as_ref().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].nick_name.as_deref(), Some("Trading"));
        assert_eq!(accounts[0].display_acct_id.as_deref(), Some("...5678"));
    }

    #[test]
    fn user_preferences_pipeline_deserializes_bare_object() {
        let input = json!({
            "accounts": [{
                "accountNumber": "12345678",
                "primaryAccount": true,
                "type": "BROKERAGE",
                "nickName": "Trading",
                "displayAcctId": "...5678"
            }],
            "streamerInfo": []
        });
        let normalized = normalize_user_preference_response(input);
        let preferences: Vec<UserPreference> = serde_json::from_value(normalized).unwrap();

        assert_eq!(preferences.len(), 1);
        let accounts = preferences[0].accounts.as_ref().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].nick_name.as_deref(), Some("Trading"));
        assert_eq!(accounts[0].display_acct_id.as_deref(), Some("...5678"));
    }

    #[test]
    fn user_preferences_pipeline_deserializes_named_envelope() {
        let input = json!({
            "userPreferences": [{
                "accounts": [{
                    "accountNumber": "12345678",
                    "nickName": "Trading",
                    "displayAcctId": "...5678"
                }],
                "streamerInfo": []
            }],
            "metadata": {"ignored": true}
        });
        let normalized = normalize_user_preference_response(input);
        let preferences: Vec<UserPreference> = serde_json::from_value(normalized).unwrap();

        assert_eq!(preferences.len(), 1);
        let accounts = preferences[0].accounts.as_ref().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].nick_name.as_deref(), Some("Trading"));
        assert_eq!(accounts[0].display_acct_id.as_deref(), Some("...5678"));
    }
}
