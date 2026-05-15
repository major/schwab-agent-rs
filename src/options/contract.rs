//! Fetches a single option contract by exact match on underlying, expiration,
//! strike, and contract type.

use schwab::{Client, OptionChain, OptionChainOptions, OptionContract};
use serde::Serialize;
use serde_json::Value;

use crate::cli::ContractArgs;
use crate::error::AppError;

use super::types::{FlatContract, compute_dte, flatten_chain};

/// Looks up a single option contract and returns a curated flat JSON object.
///
/// Narrows the chain request server-side to a single expiration, strike, and
/// contract type, then extracts the first matching contract with curated
/// pricing, greeks, and analytics fields.
///
/// # Errors
///
/// Returns [`AppError::OptionsValidation`] if no matching contract is found.
/// Returns [`AppError::OptionsSymbolNotFound`] if the symbol has no listed options.
pub async fn handle(client: &Client, args: &ContractArgs) -> Result<Value, AppError> {
    let contract_type = if args.call { "CALL" } else { "PUT" };

    let options = OptionChainOptions::new(&args.symbol)
        .parameter("contractType", contract_type)
        .number_parameter("strike", args.strike)
        .parameter("fromDate", &args.expiration)
        .parameter("toDate", &args.expiration)
        .parameter("strategy", "SINGLE")
        .include_underlying_quote(true);

    let chain = client.get_option_chain(options).await.map_err(|e| {
        if is_symbol_error(&e) {
            AppError::OptionsSymbolNotFound {
                symbol: args.symbol.clone(),
            }
        } else {
            AppError::from(e)
        }
    })?;

    let contracts = flatten_chain(&chain);
    if contracts.is_empty() {
        return Err(AppError::OptionsValidation {
            message: format!(
                "no contract found for {} {} {} {} - use `option chain` to see available contracts",
                args.symbol, args.expiration, args.strike, contract_type
            ),
        });
    }

    let flat = &contracts[0];
    let raw = find_raw_contract(&chain, contract_type);
    let dte = compute_dte(&flat.expiration).unwrap_or(flat.dte);

    Ok(build_output(args, flat, raw, dte, contract_type))
}

/// Builds the curated flat JSON output from matched contract data.
pub(super) fn build_output(
    args: &ContractArgs,
    flat: &FlatContract,
    raw: Option<&OptionContract>,
    dte: i32,
    contract_type: &str,
) -> Value {
    let mut map = serde_json::Map::new();

    // Core
    map.insert("underlying".into(), Value::String(args.symbol.clone()));
    map.insert("symbol".into(), value_or_null(&flat.symbol));
    map.insert("description".into(), value_or_null(&flat.description));
    map.insert("expiration".into(), Value::String(flat.expiration.clone()));
    map.insert("dte".into(), Value::from(dte));
    map.insert(
        "strike".into(),
        serde_json::to_value(flat.strike).unwrap_or_default(),
    );
    map.insert("type".into(), Value::String(contract_type.to_string()));

    // Price
    map.insert("bid".into(), value_or_null(&flat.bid));
    map.insert("ask".into(), value_or_null(&flat.ask));
    map.insert("mark".into(), value_or_null(&flat.mark));
    map.insert("last".into(), value_or_null(&flat.last));

    // Activity
    map.insert("volume".into(), value_or_null(&flat.volume));
    map.insert("openInterest".into(), value_or_null(&flat.oi));

    // Greeks (coerce missing to 0.0)
    map.insert("delta".into(), greek_or_zero(&flat.delta));
    map.insert("gamma".into(), greek_or_zero(&flat.gamma));
    map.insert("theta".into(), greek_or_zero(&flat.theta));
    map.insert("vega".into(), greek_or_zero(&flat.vega));
    map.insert("rho".into(), greek_or_zero(&flat.rho));

    // Analytics
    map.insert("iv".into(), value_or_null(&flat.iv));
    map.insert(
        "theoreticalValue".into(),
        raw_field(raw, |c| c.theoretical_option_value),
    );
    map.insert(
        "intrinsicValue".into(),
        raw_field(raw, |c| c.intrinsic_value),
    );
    map.insert("extrinsicValue".into(), raw_field(raw, |c| c.time_value));

    // Status
    map.insert("inTheMoney".into(), value_or_null(&flat.itm));
    map.insert("multiplier".into(), raw_field(raw, |c| c.multiplier));
    map.insert("exerciseType".into(), Value::Null);
    map.insert(
        "settlementType".into(),
        raw_field(raw, |c| c.settlement_type.clone()),
    );
    map.insert(
        "expirationType".into(),
        raw_field(raw, |c| c.expiration_type.clone()),
    );

    Value::Object(map)
}

/// Finds the first raw [`OptionContract`] in the chain for the given side.
pub(super) fn find_raw_contract<'a>(
    chain: &'a OptionChain,
    contract_type: &str,
) -> Option<&'a OptionContract> {
    let map = match contract_type {
        "CALL" => chain.call_exp_date_map.as_ref()?,
        _ => chain.put_exp_date_map.as_ref()?,
    };

    for strikes in map.values() {
        for contracts in strikes.values() {
            if let Some(contract) = contracts.first() {
                return Some(contract);
            }
        }
    }
    None
}

/// Coerces a missing greek value to `0.0` for consistent agent output.
fn greek_or_zero(value: &Option<Value>) -> Value {
    value.clone().unwrap_or(Value::from(0.0))
}

/// Unwraps an optional JSON value, returning `null` for `None`.
fn value_or_null(value: &Option<Value>) -> Value {
    value.clone().unwrap_or_default()
}

/// Extracts a serializable field from the raw [`OptionContract`].
fn raw_field<T, F>(raw: Option<&OptionContract>, extractor: F) -> Value
where
    T: Serialize,
    F: FnOnce(&OptionContract) -> Option<T>,
{
    raw.and_then(extractor)
        .and_then(|v| serde_json::to_value(v).ok())
        .unwrap_or_default()
}

/// Returns true when a Schwab error indicates the symbol was not found.
fn is_symbol_error(error: &schwab::Error) -> bool {
    matches!(error, schwab::Error::HttpStatus { status: 404, .. })
}
