use serde::Serialize;
use serde_json::{Value, json};

use schwab::Client;

use super::types::compute_dte;
use crate::error::AppError;

/// Fetches option expiration dates for a symbol and returns row-based output.
///
/// Calls the Schwab expiration chain endpoint and transforms the response into
/// a sorted table of expiration dates with client-computed days-to-expiration,
/// expiration type, and settlement type.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn handle(client: &Client, symbol: &str) -> Result<Value, AppError> {
    let chain =
        client
            .get_expiration_chain(symbol)
            .await
            .map_err(|_| AppError::OptionsSymbolNotFound {
                symbol: symbol.to_string(),
            })?;

    let expirations = chain.expiration_list.unwrap_or_default();
    Ok(format_expirations(symbol, &expirations))
}

/// Transforms a list of expirations into sorted row-based JSON output.
///
/// Rows contain expiration date, client-computed DTE, expiration type, and
/// settlement type. Entries with unparseable dates are silently skipped.
#[must_use]
pub(crate) fn format_expirations(symbol: &str, expirations: &[schwab::Expiration]) -> Value {
    let mut rows: Vec<Vec<Value>> = expirations
        .iter()
        .filter_map(|exp| {
            let date = exp.expiration.as_deref()?;
            let dte = compute_dte(date)?;
            Some(vec![
                Value::String(date.to_string()),
                Value::from(dte),
                enum_to_value(&exp.expiration_type),
                enum_to_value(&exp.settlement_type),
            ])
        })
        .collect();

    rows.sort_by(|a, b| {
        let date_a = a[0].as_str().unwrap_or("");
        let date_b = b[0].as_str().unwrap_or("");
        date_a.cmp(date_b)
    });

    let row_count = rows.len();
    json!({
        "underlying": symbol,
        "columns": ["expiration", "dte", "expirationType", "settlementType"],
        "rows": rows,
        "rowCount": row_count,
    })
}

/// Serializes an optional serde-compatible enum to its JSON representation.
#[must_use]
fn enum_to_value<T: Serialize>(value: &Option<T>) -> Value {
    value
        .as_ref()
        .and_then(|v| serde_json::to_value(v).ok())
        .unwrap_or_default()
}
