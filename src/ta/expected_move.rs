//! Expected move command handler.

// The TA command dispatch is wired in a later migration step, so this module is
// intentionally implemented and tested before production call sites exist.
#![allow(dead_code)]

use schwab::{Client, OptionChain, OptionChainOptions, OptionContract};
use serde_json::{Value, to_value};

use crate::cli::ExpectedMoveArgs;
use crate::error::AppError;
use crate::ta::types::ExpectedMoveOutput;

const INDICATOR: &str = "expected-move";
const PRICE_MIDPOINT_DIVISOR: f64 = 2.0;

/// Raw expected move calculation from ATM straddle pricing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpectedMoveCalc {
    /// Sum of the ATM call and put prices.
    pub straddle_price: f64,
    /// Expected move in price terms.
    pub expected_move: f64,
    /// Expected move as a percentage of the underlying price.
    pub expected_move_percent: f64,
    /// Upper expected move bound.
    pub upper_range: f64,
    /// Lower expected move bound.
    pub lower_range: f64,
}

#[derive(Debug, Clone)]
struct ExpirationSelection {
    key: String,
    date: String,
    dte: u32,
}

#[derive(Debug, Clone, Copy)]
struct AtmContracts<'a> {
    call: &'a OptionContract,
    put: &'a OptionContract,
    call_strike: f64,
    put_strike: f64,
    call_price: f64,
    put_price: f64,
}

/// Computes the raw expected move from ATM call and put prices.
///
/// The `dte` parameter is accepted to keep the calculation signature aligned
/// with the command inputs, but the raw ATM straddle formula is intentionally
/// not annualized by time.
#[must_use]
pub fn compute_expected_move(
    underlying_price: f64,
    call_price: f64,
    put_price: f64,
    dte: u32,
) -> Result<ExpectedMoveCalc, AppError> {
    let _ = dte;
    if underlying_price <= 0.0 {
        return Err(calculation_error(format!(
            "underlying price must be greater than zero, got {underlying_price}"
        )));
    }
    if call_price < 0.0 {
        return Err(calculation_error(format!(
            "call price must be greater than or equal to zero, got {call_price}"
        )));
    }
    if put_price < 0.0 {
        return Err(calculation_error(format!(
            "put price must be greater than or equal to zero, got {put_price}"
        )));
    }

    let straddle_price = call_price + put_price;
    if straddle_price <= 0.0 {
        return Err(calculation_error(
            "ATM straddle price must be greater than zero".to_string(),
        ));
    }

    Ok(ExpectedMoveCalc {
        straddle_price,
        expected_move: straddle_price,
        expected_move_percent: (straddle_price / underlying_price) * 100.0,
        upper_range: underlying_price + straddle_price,
        lower_range: underlying_price - straddle_price,
    })
}

/// Fetches an option chain and returns expected move output as JSON.
///
/// # Errors
///
/// Returns [`AppError`] when Schwab chain retrieval fails, the chain lacks a
/// usable underlying price, no matching expiration/ATM contracts exist, or the
/// selected contracts cannot be priced.
pub async fn expected_move(client: &Client, args: &ExpectedMoveArgs) -> Result<Value, AppError> {
    let chain = client
        .get_option_chain(option_chain_options(&args.symbol))
        .await
        .map_err(|error| map_chain_error(error, &args.symbol))?;

    render_expected_move(&chain, args)
}

fn render_expected_move(chain: &OptionChain, args: &ExpectedMoveArgs) -> Result<Value, AppError> {
    let underlying_price = chain_underlying_price(chain, &args.symbol)?;
    let expiration = find_expiration(chain, args.dte, &args.symbol)?;
    let atm = find_atm_contracts(chain, &expiration.key, underlying_price)?;
    let calc = compute_expected_move(
        underlying_price,
        atm.call_price,
        atm.put_price,
        expiration.dte,
    )?;

    to_value(ExpectedMoveOutput {
        symbol: args.symbol.clone(),
        underlying_price,
        expiration: expiration.date,
        dte: expiration.dte,
        straddle_price: calc.straddle_price,
        expected_move: calc.expected_move,
        expected_move_percent: calc.expected_move_percent,
        upper_range: calc.upper_range,
        lower_range: calc.lower_range,
        implied_volatility: average_implied_volatility(atm.call, atm.put),
        call_price: atm.call_price,
        put_price: atm.put_price,
    })
    .map_err(AppError::from)
}

fn option_chain_options(symbol: &str) -> OptionChainOptions {
    OptionChainOptions::new(symbol)
        .parameter("strategy", "SINGLE")
        .parameter("contractType", "ALL")
        .parameter("range", "NTM")
        .include_underlying_quote(true)
}

fn chain_underlying_price(chain: &OptionChain, symbol: &str) -> Result<f64, AppError> {
    let price = chain
        .underlying
        .as_ref()
        .and_then(|underlying| positive_number(underlying.mark))
        .or_else(|| {
            chain
                .underlying
                .as_ref()
                .and_then(|underlying| positive_number(underlying.last))
        })
        .or_else(|| positive_number(chain.underlying_price));

    price.ok_or_else(|| {
        insufficient_data(format!("unable to determine underlying price for {symbol}"))
    })
}

fn find_expiration(
    chain: &OptionChain,
    target_dte: u32,
    symbol: &str,
) -> Result<ExpirationSelection, AppError> {
    let call_expirations = chain
        .call_exp_date_map
        .as_ref()
        .ok_or_else(|| insufficient_data(format!("no call options available for {symbol}")))?;
    let put_expirations = chain
        .put_exp_date_map
        .as_ref()
        .ok_or_else(|| insufficient_data(format!("no put options available for {symbol}")))?;

    call_expirations
        .keys()
        .filter(|key| put_expirations.contains_key(*key))
        .filter_map(|key| parse_expiration_key(key))
        .min_by(|left, right| {
            left.dte
                .abs_diff(target_dte)
                .cmp(&right.dte.abs_diff(target_dte))
                .then_with(|| left.key.cmp(&right.key))
        })
        .ok_or_else(|| insufficient_data(format!("no option expirations available for {symbol}")))
}

fn find_atm_contracts<'a>(
    chain: &'a OptionChain,
    expiration_key: &str,
    underlying_price: f64,
) -> Result<AtmContracts<'a>, AppError> {
    let call_strikes = chain
        .call_exp_date_map
        .as_ref()
        .and_then(|expirations| expirations.get(expiration_key))
        .ok_or_else(|| {
            insufficient_data(format!("no call strikes for expiration {expiration_key}"))
        })?;
    let put_strikes = chain
        .put_exp_date_map
        .as_ref()
        .and_then(|expirations| expirations.get(expiration_key))
        .ok_or_else(|| {
            insufficient_data(format!("no put strikes for expiration {expiration_key}"))
        })?;

    let mut best: Option<(&str, f64)> = None;
    for strike_key in call_strikes
        .keys()
        .filter(|key| put_strikes.contains_key(*key))
    {
        let Ok(strike) = strike_key.parse::<f64>() else {
            continue;
        };
        let diff = (strike - underlying_price).abs();
        if best.is_none_or(|(_, best_strike)| {
            let best_diff = (best_strike - underlying_price).abs();
            diff < best_diff || (diff == best_diff && strike < best_strike)
        }) {
            best = Some((strike_key.as_str(), strike));
        }
    }

    let (strike_key, strike) = best.ok_or_else(|| {
        insufficient_data(format!(
            "no common call/put strikes for expiration {expiration_key}"
        ))
    })?;
    let call = first_contract(call_strikes.get(strike_key), "call", strike_key)?;
    let put = first_contract(put_strikes.get(strike_key), "put", strike_key)?;
    let call_price = contract_price(
        call,
        "call",
        chain.symbol.as_deref().unwrap_or("symbol"),
        strike_key,
    )?;
    let put_price = contract_price(
        put,
        "put",
        chain.symbol.as_deref().unwrap_or("symbol"),
        strike_key,
    )?;

    Ok(AtmContracts {
        call,
        put,
        call_strike: strike,
        put_strike: strike,
        call_price,
        put_price,
    })
}

fn first_contract<'a>(
    contracts: Option<&'a Vec<OptionContract>>,
    put_call: &str,
    strike: &str,
) -> Result<&'a OptionContract, AppError> {
    contracts
        .and_then(|contracts| contracts.first())
        .ok_or_else(|| insufficient_data(format!("no {put_call} contracts at strike {strike}")))
}

fn contract_price(
    contract: &OptionContract,
    put_call: &str,
    symbol: &str,
    strike: &str,
) -> Result<f64, AppError> {
    if let Some(mark) = positive_number(contract.mark_price) {
        return Ok(mark);
    }

    if let (Some(bid), Some(ask)) = (
        positive_number(contract.bid_price),
        positive_number(contract.ask_price),
    ) {
        return Ok((bid + ask) / PRICE_MIDPOINT_DIVISOR);
    }

    Err(calculation_error(format!(
        "unable to determine {put_call} price for {symbol} at strike {strike}"
    )))
}

#[must_use]
fn average_implied_volatility(call: &OptionContract, put: &OptionContract) -> Option<f64> {
    let values = [
        positive_number(call.volatility),
        positive_number(put.volatility),
    ];
    let positive_values = values.into_iter().flatten().collect::<Vec<_>>();
    if positive_values.is_empty() {
        None
    } else {
        Some(positive_values.iter().sum::<f64>() / positive_values.len() as f64)
    }
}

fn parse_expiration_key(key: &str) -> Option<ExpirationSelection> {
    let (date, dte) = key.split_once(':')?;
    if date.contains(':') || date.is_empty() {
        return None;
    }

    Some(ExpirationSelection {
        key: key.to_string(),
        date: date.to_string(),
        dte: dte.parse().ok()?,
    })
}

#[must_use]
fn positive_number(value: Option<schwab::Number>) -> Option<f64> {
    let value = number_to_f64(value?)?;
    (value > 0.0).then_some(value)
}

#[must_use]
fn number_to_f64(value: schwab::Number) -> Option<f64> {
    value.to_string().parse::<f64>().ok()
}

fn insufficient_data(reason: String) -> AppError {
    AppError::TaInsufficientData {
        needed: 1,
        got: 0,
        indicator: format!("{INDICATOR}: {reason}"),
    }
}

fn calculation_error(reason: String) -> AppError {
    AppError::TaCalculationError {
        indicator: INDICATOR.to_string(),
        reason,
    }
}

fn map_chain_error(error: schwab::Error, symbol: &str) -> AppError {
    match error {
        schwab::Error::HttpStatus { status, .. } if status == 400 || status == 404 => {
            insufficient_data(format!("symbol has no listed options: {symbol}"))
        }
        error => AppError::Schwab(error),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::error::AppError;

    const EPSILON: f64 = 1e-9;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "expected {actual} to be within {EPSILON} of {expected}"
        );
    }

    fn option_chain_from_json(value: serde_json::Value) -> schwab::OptionChain {
        serde_json::from_value(value).expect("test option chain should deserialize")
    }

    fn sample_chain() -> schwab::OptionChain {
        option_chain_from_json(json!({
            "symbol": "XYZ",
            "underlyingPrice": 101.0,
            "underlying": { "mark": 101.0, "last": 100.5 },
            "callExpDateMap": {
                "2026-06-19:35": {
                    "95.0": [{ "strikePrice": 95.0, "markPrice": 8.0, "bidPrice": 7.9, "askPrice": 8.1, "volatility": 20.0 }],
                    "100.0": [{ "strikePrice": 100.0, "markPrice": 3.5, "bidPrice": 3.4, "askPrice": 3.6, "volatility": 22.0 }],
                    "105.0": [{ "strikePrice": 105.0, "markPrice": 1.5, "bidPrice": 1.4, "askPrice": 1.6, "volatility": 24.0 }]
                }
            },
            "putExpDateMap": {
                "2026-06-19:35": {
                    "95.0": [{ "strikePrice": 95.0, "markPrice": 1.2, "bidPrice": 1.1, "askPrice": 1.3, "volatility": 23.0 }],
                    "100.0": [{ "strikePrice": 100.0, "markPrice": 3.2, "bidPrice": 3.1, "askPrice": 3.3, "volatility": 25.0 }],
                    "105.0": [{ "strikePrice": 105.0, "markPrice": 6.4, "bidPrice": 6.3, "askPrice": 6.5, "volatility": 27.0 }]
                }
            }
        }))
    }

    #[test]
    fn compute_expected_move_uses_raw_straddle_for_range_math() {
        let calc = compute_expected_move(100.0, 3.50, 3.20, 30)
            .expect("valid option prices should compute");

        assert_close(calc.straddle_price, 6.70);
        assert_close(calc.expected_move, 6.70);
        assert_close(calc.expected_move_percent, 6.70);
        assert_close(calc.upper_range, 106.70);
        assert_close(calc.lower_range, 93.30);
    }

    #[test]
    fn find_atm_contracts_selects_closest_strike_to_underlying_price() {
        let chain = sample_chain();

        let selection = find_atm_contracts(&chain, "2026-06-19:35", 101.0)
            .expect("ATM contracts should be selected");

        assert_close(selection.call_strike, 100.0);
        assert_close(selection.put_strike, 100.0);
        assert_close(selection.call_price, 3.5);
        assert_close(selection.put_price, 3.2);
    }

    #[test]
    fn contract_price_uses_midpoint_when_mark_price_is_unavailable() {
        let contract = option_chain_from_json(json!({
            "callExpDateMap": { "2026-06-19:35": { "100.0": [{ "bidPrice": 2.0, "askPrice": 3.0 }] } }
        }))
        .call_exp_date_map
        .expect("call map should exist")
        .remove("2026-06-19:35")
        .expect("expiration should exist")
        .remove("100.0")
        .expect("strike should exist")
        .remove(0);

        let price = contract_price(&contract, "call", "XYZ", "100.0")
            .expect("bid/ask midpoint should price contract");

        assert_close(price, 2.5);
    }

    #[test]
    fn contract_price_rejects_zero_pricing() {
        let contract = option_chain_from_json(json!({
            "callExpDateMap": { "2026-06-19:35": { "100.0": [{ "markPrice": 0.0, "bidPrice": 0.0, "askPrice": 0.0 }] } }
        }))
        .call_exp_date_map
        .expect("call map should exist")
        .remove("2026-06-19:35")
        .expect("expiration should exist")
        .remove("100.0")
        .expect("strike should exist")
        .remove(0);

        let error = contract_price(&contract, "call", "XYZ", "100.0")
            .expect_err("zero pricing should fail");

        assert!(matches!(
            error,
            AppError::TaCalculationError { indicator, .. } if indicator == "expected-move"
        ));
    }
}
