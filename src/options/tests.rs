use schwab::{Expiration, ExpirationType, Number, OptionChain, OptionContract, SettlementType};
use serde_json::{Value, json};
use time::{Date, Duration, OffsetDateTime};

use crate::cli::ScreenArgs;

use super::expirations::format_expirations;
use super::screen::screen_chain;
use super::types::{
    ALL_FIELDS, CHAIN_FIELDS, FlatContract, SCREEN_FIELDS, compute_dte, filter_by_ask,
    filter_by_bid, filter_by_delta, filter_by_oi, filter_by_premium, filter_by_spread_pct,
    filter_by_strike, filter_by_volume, flatten_chain, select_fields, sort_contracts,
    validate_fields,
};

#[test]
fn flatten_chain_collects_calls_and_puts_across_expirations_and_strikes() {
    let chain = option_chain(false);

    let rows = flatten_chain(&chain);

    assert_eq!(rows.len(), 12);
    assert_eq!(
        rows.iter()
            .filter(|row| row.contract_type == "CALL")
            .count(),
        6
    );
    assert_eq!(
        rows.iter().filter(|row| row.contract_type == "PUT").count(),
        6
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.expiration == "2026-01-16")
            .count(),
        6
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.expiration == "2026-02-20")
            .count(),
        6
    );
}

#[test]
fn sort_contracts_orders_by_expiration_strike_and_call_before_put() {
    let mut contracts = vec![
        flat_contract("2026-02-20", 37, 105.0, "PUT"),
        flat_contract("2026-01-16", 2, 100.0, "PUT"),
        flat_contract("2026-01-16", 2, 100.0, "CALL"),
        flat_contract("2026-01-16", 2, 95.0, "CALL"),
    ];

    sort_contracts(&mut contracts);

    assert_eq!(contracts[0].expiration, "2026-01-16");
    assert_eq!(contracts[0].strike, number(95.0));
    assert_eq!(contracts[0].contract_type, "CALL");
    assert_eq!(contracts[1].contract_type, "CALL");
    assert_eq!(contracts[2].contract_type, "PUT");
    assert_eq!(contracts[3].expiration, "2026-02-20");
}

#[cfg(not(feature = "decimal"))]
#[test]
fn sort_contracts_places_nan_strikes_last() {
    let mut contracts = vec![
        flat_contract_with_number("2026-01-16", 2, f64::NAN, "CALL"),
        flat_contract("2026-01-16", 2, 105.0, "CALL"),
        flat_contract("2026-01-16", 2, 100.0, "CALL"),
    ];

    sort_contracts(&mut contracts);

    assert_eq!(contracts[0].strike, number(100.0));
    assert_eq!(contracts[1].strike, number(105.0));
    assert!(contracts[2].strike.is_nan());
}

#[test]
fn select_fields_returns_requested_columns_and_rows() {
    let contracts = vec![flat_contract("2026-01-16", 2, 100.0, "CALL")];

    let (columns, rows) = select_fields(&contracts, &["expiration", "strike", "type", "mark"]);

    assert_eq!(columns, vec!["expiration", "strike", "type", "mark"]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], Value::String("2026-01-16".to_string()));
    assert_eq!(rows[0][1], number_value(100.0));
    assert_eq!(rows[0][2], Value::String("CALL".to_string()));
    assert_eq!(rows[0][3], number_value(1.5));
}

#[test]
fn select_fields_keeps_legacy_aliases_available() {
    let contracts = vec![flat_contract("2026-01-16", 2, 100.0, "CALL")];

    let (columns, rows) = select_fields(&contracts, &["contract_type", "oi", "itm"]);

    assert_eq!(columns, vec!["contract_type", "oi", "itm"]);
    assert_eq!(rows[0][0], Value::String("CALL".to_string()));
    assert_eq!(rows[0][1], number_value(500.0));
    assert_eq!(rows[0][2], Value::Bool(true));
}

#[test]
fn default_chain_and_screen_fields_match_plan_schema() {
    let expected = [
        "symbol",
        "expiration",
        "dte",
        "strike",
        "type",
        "bid",
        "ask",
        "mark",
        "last",
        "volume",
        "openInterest",
        "delta",
        "gamma",
        "theta",
        "vega",
        "iv",
    ];

    assert_eq!(CHAIN_FIELDS, expected);
    assert_eq!(SCREEN_FIELDS, expected);
}

#[test]
fn all_fields_includes_plan_available_fields() {
    let plan_fields = [
        "symbol",
        "description",
        "expiration",
        "dte",
        "strike",
        "type",
        "bid",
        "ask",
        "mark",
        "last",
        "close",
        "highPrice",
        "lowPrice",
        "volume",
        "openInterest",
        "delta",
        "gamma",
        "theta",
        "vega",
        "rho",
        "iv",
        "theoreticalValue",
        "intrinsicValue",
        "extrinsicValue",
        "timeValue",
        "inTheMoney",
        "multiplier",
        "exerciseType",
        "settlementType",
        "expirationType",
        "percentChange",
        "markChange",
        "markPercentChange",
    ];

    for field in plan_fields {
        assert!(ALL_FIELDS.contains_key(field), "missing field {field}");
    }
}

#[test]
fn validate_fields_accepts_known_aliases_and_rejects_unknown_fields() {
    let valid = vec![
        "expiration".to_string(),
        "type".to_string(),
        "iv".to_string(),
        "openInterest".to_string(),
        "inTheMoney".to_string(),
    ];
    assert!(validate_fields(&valid).is_ok());

    let invalid = vec!["expiration".to_string(), "nope".to_string()];
    let error = validate_fields(&invalid).expect_err("unknown field should fail");
    let message = error.to_string();
    assert!(message.contains("nope"));
    assert!(message.contains("available fields"));
}

#[test]
fn compute_dte_returns_calendar_days_from_today_utc() {
    let today = OffsetDateTime::now_utc().date();
    let next_week = today.saturating_add(Duration::days(7));

    assert_eq!(compute_dte(&date_string(today)), Some(0));
    assert_eq!(compute_dte(&date_string(next_week)), Some(7));
    assert_eq!(compute_dte("not-a-date"), None);
}

#[test]
fn filter_predicates_match_present_values_and_reject_out_of_range_or_missing_values() {
    let contract = flat_contract("2026-01-16", 2, 100.0, "CALL");
    let missing = FlatContract {
        delta: None,
        bid: None,
        ask: None,
        oi: None,
        volume: None,
        mark: None,
        ..contract.clone()
    };

    assert!(filter_by_delta(
        &contract,
        Some(number(0.40)),
        Some(number(0.60))
    ));
    assert!(!filter_by_delta(&contract, Some(number(0.60)), None));
    assert!(!filter_by_delta(
        &missing,
        Some(number(0.40)),
        Some(number(0.60))
    ));

    assert!(filter_by_strike(
        &contract,
        Some(number(95.0)),
        Some(number(105.0)),
        None
    ));
    assert!(filter_by_strike(&contract, None, None, Some(number(100.0))));
    assert!(!filter_by_strike(
        &contract,
        None,
        None,
        Some(number(101.0))
    ));

    assert!(filter_by_bid(&contract, number(1.0)));
    assert!(!filter_by_bid(&contract, number(2.0)));
    assert!(!filter_by_bid(&missing, number(1.0)));

    assert!(filter_by_ask(&contract, number(2.0)));
    assert!(!filter_by_ask(&contract, number(1.0)));
    assert!(!filter_by_ask(&missing, number(2.0)));

    assert!(filter_by_volume(&contract, number(100.0)));
    assert!(!filter_by_volume(&contract, number(300.0)));
    assert!(!filter_by_volume(&missing, number(100.0)));

    assert!(filter_by_oi(&contract, number(400.0)));
    assert!(!filter_by_oi(&contract, number(600.0)));
    assert!(!filter_by_oi(&missing, number(400.0)));

    assert!(filter_by_premium(
        &contract,
        Some(number(1.0)),
        Some(number(2.0))
    ));
    assert!(!filter_by_premium(&contract, Some(number(2.0)), None));
    assert!(!filter_by_premium(
        &missing,
        Some(number(1.0)),
        Some(number(2.0))
    ));
}

#[test]
fn filter_by_spread_pct_uses_mark_and_skips_zero_mark() {
    let contract = flat_contract("2026-01-16", 2, 100.0, "CALL");
    let zero_mark = FlatContract {
        mark: Some(number_value(0.0)),
        ..contract.clone()
    };

    assert!(filter_by_spread_pct(&contract, number(70.0)));
    assert!(!filter_by_spread_pct(&contract, number(50.0)));
    assert!(!filter_by_spread_pct(&zero_mark, number(70.0)));
}

#[test]
fn flatten_chain_skips_malformed_expiration_keys_without_panicking() {
    let chain = option_chain(true);

    let rows = flatten_chain(&chain);

    assert_eq!(rows.len(), 12);
    assert!(rows.iter().all(|row| row.expiration != "bad-key"));
}

fn option_chain(include_malformed: bool) -> OptionChain {
    let mut calls = serde_json::Map::new();
    let mut puts = serde_json::Map::new();

    for (expiration, dte) in [("2026-01-16", 2), ("2026-02-20", 37)] {
        calls.insert(
            format!("{expiration}:{dte}"),
            expiration_map(expiration, dte, "CALL"),
        );
        puts.insert(
            format!("{expiration}:{dte}"),
            expiration_map(expiration, dte, "PUT"),
        );
    }

    if include_malformed {
        calls.insert(
            "bad-key".to_string(),
            expiration_map("2026-03-20", 65, "CALL"),
        );
    }

    serde_json::from_value(json!({
        "symbol": "AAPL",
        "status": "SUCCESS",
        "callExpDateMap": Value::Object(calls),
        "putExpDateMap": Value::Object(puts),
    }))
    .expect("test option chain should deserialize")
}

fn expiration_map(expiration: &str, dte: i32, contract_type: &str) -> Value {
    let mut strikes = serde_json::Map::new();
    for strike in [95.0, 100.0, 105.0] {
        strikes.insert(
            format!("{strike:.1}"),
            json!([contract_json(expiration, dte, strike, contract_type)]),
        );
    }
    Value::Object(strikes)
}

fn contract_json(expiration: &str, dte: i32, strike: f64, contract_type: &str) -> Value {
    json!({
        "symbol": format!("AAPL {expiration} {contract_type} {strike}"),
        "description": format!("AAPL {expiration} {strike} {contract_type}"),
        "putCall": contract_type,
        "bidPrice": 1.0,
        "askPrice": 2.0,
        "markPrice": 1.5,
        "lastPrice": 1.4,
        "closePrice": 1.3,
        "highPrice": 1.8,
        "lowPrice": 1.2,
        "strikePrice": strike,
        "daysToExpiration": dte,
        "delta": 0.5,
        "gamma": 0.1,
        "theta": -0.02,
        "vega": 0.3,
        "rho": 0.04,
        "volatility": 25.0,
        "openInterest": 500.0,
        "totalVolume": 200.0,
        "isInTheMoney": true,
        "theoreticalOptionValue": 1.45,
        "intrinsicValue": 0.5,
        "timeValue": 0.95,
        "multiplier": 100.0,
        "settlementType": "P",
        "expirationType": "S",
        "percentChange": 2.5,
        "markChange": 0.1,
        "markPercentChange": 3.0,
    })
}

fn flat_contract(expiration: &str, dte: i32, strike: f64, contract_type: &str) -> FlatContract {
    flat_contract_with_number(expiration, dte, number(strike), contract_type)
}

fn flat_contract_with_number(
    expiration: &str,
    dte: i32,
    strike: Number,
    contract_type: &str,
) -> FlatContract {
    FlatContract {
        expiration: expiration.to_string(),
        dte,
        strike,
        contract_type: contract_type.to_string(),
        symbol: Some(Value::String("AAPL  260116C00100000".to_string())),
        description: Some(Value::String("AAPL Jan 16 2026 100 Call".to_string())),
        bid: Some(number_value(1.0)),
        ask: Some(number_value(2.0)),
        mark: Some(number_value(1.5)),
        last: Some(number_value(1.4)),
        close: Some(number_value(1.3)),
        high_price: Some(number_value(1.8)),
        low_price: Some(number_value(1.2)),
        delta: Some(number_value(0.5)),
        gamma: Some(number_value(0.1)),
        theta: Some(number_value(-0.02)),
        vega: Some(number_value(0.3)),
        rho: Some(number_value(0.04)),
        iv: Some(number_value(25.0)),
        oi: Some(number_value(500.0)),
        volume: Some(number_value(200.0)),
        itm: Some(Value::Bool(true)),
        theoretical_value: Some(number_value(1.45)),
        intrinsic_value: Some(number_value(0.5)),
        extrinsic_value: Some(number_value(0.95)),
        time_value: Some(number_value(0.95)),
        multiplier: Some(number_value(100.0)),
        exercise_type: None,
        settlement_type: Some(Value::String("P".to_string())),
        expiration_type: Some(Value::String("S".to_string())),
        percent_change: Some(number_value(2.5)),
        mark_change: Some(number_value(0.1)),
        mark_percent_change: Some(number_value(3.0)),
        days_to_expiration: Some(number_value(f64::from(dte))),
    }
}

fn number(value: f64) -> Number {
    serde_json::from_value(json!(value)).expect("test number should deserialize")
}

fn number_value(value: f64) -> Value {
    serde_json::to_value(number(value)).expect("test number should serialize")
}

fn date_string(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

#[test]
fn chain_render_with_no_filters_returns_all_contracts_sorted() {
    let output = render_command_chain(chain_args("AAPL"));

    assert_eq!(output["rowCount"].as_u64(), Some(18));
    assert_eq!(
        output["columns"],
        json!([
            "symbol",
            "expiration",
            "dte",
            "strike",
            "type",
            "bid",
            "ask",
            "mark",
            "last",
            "volume",
            "openInterest",
            "delta",
            "gamma",
            "theta",
            "vega",
            "iv"
        ])
    );
    let rows = output["rows"].as_array().expect("rows should be an array");
    assert_eq!(rows[0][1], Value::String("2024-01-19".to_string()));
    assert_eq!(rows[0][3], number_value(180.0));
    assert_eq!(rows[0][4], Value::String("CALL".to_string()));
    assert_eq!(rows[1][4], Value::String("PUT".to_string()));
}

#[test]
fn chain_render_type_call_filters_to_calls_only() {
    let mut args = chain_args("AAPL");
    args.contract_type = Some("call".to_string());

    let output = render_command_chain(args);

    assert_eq!(output["rowCount"].as_u64(), Some(9));
    let rows = output["rows"].as_array().expect("rows should be an array");
    assert!(
        rows.iter()
            .all(|row| row[4] == Value::String("CALL".to_string()))
    );
}

#[test]
fn chain_render_dte_selects_nearest_expiration() {
    let mut args = chain_args("AAPL");
    args.dte = Some(30);
    let expected_expiration = command_expiration_date(28);

    let output = render_command_chain(args);

    assert_eq!(output["rowCount"].as_u64(), Some(6));
    let rows = output["rows"].as_array().expect("rows should be an array");
    assert!(
        rows.iter()
            .all(|row| row[1] == Value::String(expected_expiration.clone()))
    );
}

#[test]
fn chain_render_expiration_filters_to_exact_date() {
    let mut args = chain_args("AAPL");
    args.expiration = Some("2024-01-19".to_string());

    let output = render_command_chain(args);

    assert_eq!(output["rowCount"].as_u64(), Some(6));
    let rows = output["rows"].as_array().expect("rows should be an array");
    assert!(
        rows.iter()
            .all(|row| row[1] == Value::String("2024-01-19".to_string()))
    );
}

#[test]
fn chain_render_delta_bounds_filter_contracts() {
    let mut args = chain_args("AAPL");
    args.delta_min = Some(0.30);
    args.delta_max = Some(0.70);

    let output = render_command_chain(args);

    assert_eq!(output["rowCount"].as_u64(), Some(6));
    let rows = output["rows"].as_array().expect("rows should be an array");
    assert!(
        rows.iter()
            .all(|row| row[4] == Value::String("CALL".to_string()))
    );
}

#[test]
fn chain_render_strike_bounds_filter_contracts() {
    let mut args = chain_args("AAPL");
    args.strike_min = Some(180.0);
    args.strike_max = Some(190.0);

    let output = render_command_chain(args);

    assert_eq!(output["rowCount"].as_u64(), Some(12));
    let rows = output["rows"].as_array().expect("rows should be an array");
    assert!(
        rows.iter()
            .all(|row| row[3] == number_value(180.0) || row[3] == number_value(190.0))
    );
}

#[test]
fn chain_render_fields_returns_requested_columns() {
    let mut args = chain_args("AAPL");
    args.fields = Some("strike,bid,ask,delta".to_string());

    let output = render_command_chain(args);

    assert_eq!(output["columns"], json!(["strike", "bid", "ask", "delta"]));
    let rows = output["rows"].as_array().expect("rows should be an array");
    assert!(
        rows.iter()
            .all(|row| row.as_array().is_some_and(|values| values.len() == 4))
    );
}

#[test]
fn chain_render_empty_results_returns_zero_row_count() {
    let mut args = chain_args("AAPL");
    args.strike_min = Some(999.0);

    let output = render_command_chain(args);

    assert_eq!(output["rowCount"].as_u64(), Some(0));
    assert_eq!(output["rows"], json!([]));
}

fn render_command_chain(args: crate::cli::ChainArgs) -> Value {
    super::chain::render_chain(&command_option_chain(), &args).expect("command chain should render")
}

fn chain_args(symbol: &str) -> crate::cli::ChainArgs {
    crate::cli::ChainArgs {
        symbol: symbol.to_string(),
        contract_type: None,
        dte: None,
        expiration: None,
        delta_min: None,
        delta_max: None,
        fields: None,
        strike_count: None,
        strike: None,
        strike_min: None,
        strike_max: None,
        strike_range: None,
    }
}

fn command_option_chain() -> OptionChain {
    let mut calls = serde_json::Map::new();
    let mut puts = serde_json::Map::new();
    let expirations = [
        ("2024-01-19".to_string(), -100),
        (command_expiration_date(28), 28),
        (command_expiration_date(35), 35),
    ];

    for (expiration, dte) in expirations {
        calls.insert(
            format!("{expiration}:{dte}"),
            command_expiration_map(&expiration, dte, "CALL"),
        );
        puts.insert(
            format!("{expiration}:{dte}"),
            command_expiration_map(&expiration, dte, "PUT"),
        );
    }

    serde_json::from_value(json!({
        "symbol": "AAPL",
        "status": "SUCCESS",
        "underlyingPrice": 185.0,
        "underlying": {"mark": 184.5},
        "callExpDateMap": Value::Object(calls),
        "putExpDateMap": Value::Object(puts),
    }))
    .expect("test option chain should deserialize")
}

fn command_expiration_map(expiration: &str, dte: i32, contract_type: &str) -> Value {
    let mut strikes = serde_json::Map::new();
    for strike in [180.0, 190.0, 200.0] {
        strikes.insert(
            format!("{strike:.1}"),
            json!([command_contract_json(
                expiration,
                dte,
                strike,
                contract_type
            )]),
        );
    }
    Value::Object(strikes)
}

fn command_contract_json(expiration: &str, dte: i32, strike: f64, contract_type: &str) -> Value {
    let call_delta = match strike {
        180.0 => 0.35,
        190.0 => 0.65,
        _ => 0.80,
    };
    let delta = if contract_type == "CALL" {
        call_delta
    } else {
        -call_delta
    };

    json!({
        "symbol": format!("AAPL {expiration} {contract_type} {strike}"),
        "description": format!("AAPL {expiration} {strike} {contract_type}"),
        "putCall": contract_type,
        "bidPrice": 1.0,
        "askPrice": 2.0,
        "markPrice": 1.5,
        "lastPrice": 1.4,
        "closePrice": 1.3,
        "highPrice": 1.8,
        "lowPrice": 1.2,
        "strikePrice": strike,
        "daysToExpiration": dte,
        "delta": delta,
        "gamma": 0.1,
        "theta": -0.02,
        "vega": 0.3,
        "volatility": 25.0,
        "openInterest": 500.0,
        "totalVolume": 200.0,
        "isInTheMoney": false,
        "theoreticalOptionValue": 1.45,
        "intrinsicValue": 0.5,
        "timeValue": 0.95,
        "multiplier": 100.0,
        "settlementType": "P",
        "expirationType": "S",
        "percentChange": 2.5,
        "markChange": 0.1,
        "markPercentChange": 3.0,
    })
}

fn command_expiration_date(days_from_today: i64) -> String {
    date_string(
        OffsetDateTime::now_utc()
            .date()
            .saturating_add(Duration::days(days_from_today)),
    )
}

// ---------------------------------------------------------------------------
// Expiration tests
// ---------------------------------------------------------------------------

#[test]
fn expirations_format_returns_row_based_output_with_correct_shape() {
    let today = OffsetDateTime::now_utc().date();
    let exp_date = today.saturating_add(Duration::days(30));
    let exp_str = date_string(exp_date);

    let expirations = vec![Expiration {
        expiration: Some(exp_str.clone()),
        days_to_expiration: Some(30),
        expiration_type: Some(ExpirationType::Standard),
        settlement_type: Some(SettlementType::Pm),
        option_roots: Some("AAPL".to_string()),
        standard: Some(true),
    }];

    let result = format_expirations("AAPL", &expirations);

    assert_eq!(result["underlying"], "AAPL");
    assert_eq!(
        result["columns"],
        json!(["expiration", "dte", "expirationType", "settlementType"])
    );
    assert_eq!(result["rowCount"], 1);
    assert_eq!(result["rows"][0][0], exp_str);
    assert_eq!(result["rows"][0][1], 30);
    assert_eq!(result["rows"][0][2], "S");
    assert_eq!(result["rows"][0][3], "P");
}

#[test]
fn expirations_format_sorts_by_date_ascending() {
    let today = OffsetDateTime::now_utc().date();
    let date_far = date_string(today.saturating_add(Duration::days(60)));
    let date_near = date_string(today.saturating_add(Duration::days(10)));
    let date_mid = date_string(today.saturating_add(Duration::days(30)));

    let expirations = vec![
        make_expiration(&date_far),
        make_expiration(&date_near),
        make_expiration(&date_mid),
    ];

    let result = format_expirations("AAPL", &expirations);

    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows[0][0], date_near);
    assert_eq!(rows[1][0], date_mid);
    assert_eq!(rows[2][0], date_far);
}

#[test]
fn expirations_format_computes_dte_from_today_utc() {
    let today = OffsetDateTime::now_utc().date();
    let target = date_string(today.saturating_add(Duration::days(42)));
    let expirations = vec![make_expiration(&target)];

    let result = format_expirations("AAPL", &expirations);

    assert_eq!(result["rows"][0][1], 42);
}

#[test]
fn expirations_format_returns_empty_rows_for_empty_chain() {
    let result = format_expirations("AAPL", &[]);

    assert_eq!(result["underlying"], "AAPL");
    assert_eq!(result["rowCount"], 0);
    assert_eq!(result["rows"], json!([]));
    assert_eq!(
        result["columns"],
        json!(["expiration", "dte", "expirationType", "settlementType"])
    );
}

fn make_expiration(date: &str) -> Expiration {
    Expiration {
        expiration: Some(date.to_string()),
        days_to_expiration: None,
        expiration_type: Some(ExpirationType::Weekly),
        settlement_type: Some(SettlementType::Pm),
        option_roots: None,
        standard: Some(true),
    }
}

// ---------------------------------------------------------------------------
// Contract tests
// ---------------------------------------------------------------------------

#[test]
fn contract_build_output_includes_all_curated_fields() {
    let args = contract_args("AAPL", "2026-01-16", 100.0, true);
    let flat = flat_contract("2026-01-16", 2, 100.0, "CALL");
    let raw = test_raw_option_contract();

    let output = super::contract::build_output(&args, &flat, Some(&raw), 2, "CALL");
    let obj = output.as_object().expect("output should be an object");

    // Core
    assert_eq!(obj["underlying"], "AAPL");
    assert_eq!(obj["expiration"], "2026-01-16");
    assert_eq!(obj["dte"], 2);
    assert_eq!(obj["strike"], number_value(100.0));
    assert_eq!(obj["type"], "CALL");
    assert!(obj.contains_key("symbol"));
    assert!(obj.contains_key("description"));

    // Price
    assert_eq!(obj["bid"], number_value(1.0));
    assert_eq!(obj["ask"], number_value(2.0));
    assert_eq!(obj["mark"], number_value(1.5));
    assert_eq!(obj["last"], number_value(1.4));

    // Activity
    assert!(obj.contains_key("volume"));
    assert!(obj.contains_key("openInterest"));

    // Greeks (present, not coerced)
    assert_eq!(obj["delta"], number_value(0.5));
    assert_eq!(obj["gamma"], number_value(0.1));
    assert_eq!(obj["theta"], number_value(-0.02));
    assert_eq!(obj["vega"], number_value(0.3));
    assert_eq!(obj["rho"], number_value(0.04));

    // Analytics
    assert!(obj.contains_key("iv"));
    assert_eq!(obj["theoreticalValue"], number_value(1.45));
    assert_eq!(obj["intrinsicValue"], number_value(0.5));
    assert_eq!(obj["extrinsicValue"], number_value(0.95));

    // Status
    assert!(obj.contains_key("inTheMoney"));
    assert_eq!(obj["multiplier"], number_value(100.0));
    assert!(obj.contains_key("exerciseType"));
    assert_eq!(obj["settlementType"], "P");
    assert_eq!(obj["expirationType"], "S");
}

#[test]
fn contract_missing_greeks_coerced_to_zero() {
    let args = contract_args("AAPL", "2026-01-16", 100.0, true);
    let flat = FlatContract {
        delta: None,
        gamma: None,
        theta: None,
        vega: None,
        rho: None,
        ..flat_contract("2026-01-16", 2, 100.0, "CALL")
    };

    let output = super::contract::build_output(&args, &flat, None, 2, "CALL");
    let obj = output.as_object().expect("output should be an object");

    assert_eq!(obj["delta"], Value::from(0.0));
    assert_eq!(obj["gamma"], Value::from(0.0));
    assert_eq!(obj["theta"], Value::from(0.0));
    assert_eq!(obj["vega"], Value::from(0.0));
    assert_eq!(obj["rho"], Value::from(0.0));
}

#[test]
fn contract_no_match_error_contains_option_chain_hint() {
    let error = crate::error::AppError::OptionsValidation {
        message: format!(
            "no contract found for {} {} {} {} - use `option chain` to see available contracts",
            "AAPL", "2026-01-16", 100.0, "CALL"
        ),
    };

    let msg = error.to_string();
    assert!(
        msg.contains("option chain"),
        "error should mention option chain: {msg}"
    );
    assert!(msg.contains("AAPL"), "error should mention symbol: {msg}");
    assert!(
        msg.contains("2026-01-16"),
        "error should mention expiration: {msg}"
    );
}

#[test]
fn contract_build_output_uses_provided_dte() {
    let args = contract_args("AAPL", "2026-01-16", 100.0, true);
    let flat = flat_contract("2026-01-16", 999, 100.0, "CALL");

    let output = super::contract::build_output(&args, &flat, None, 42, "CALL");
    let obj = output.as_object().expect("output should be an object");

    assert_eq!(obj["dte"], 42);
}

#[test]
fn contract_find_raw_contract_locates_first_contract_in_chain() {
    let chain = option_chain(false);

    let call = super::contract::find_raw_contract(&chain, "CALL");
    assert!(call.is_some(), "should find a CALL contract");

    let put = super::contract::find_raw_contract(&chain, "PUT");
    assert!(put.is_some(), "should find a PUT contract");
}

#[test]
fn contract_find_raw_contract_returns_none_for_empty_chain() {
    let chain: OptionChain = serde_json::from_value(json!({
        "symbol": "AAPL",
        "status": "SUCCESS",
    }))
    .expect("empty chain should deserialize");

    assert!(super::contract::find_raw_contract(&chain, "CALL").is_none());
    assert!(super::contract::find_raw_contract(&chain, "PUT").is_none());
}

fn contract_args(
    symbol: &str,
    expiration: &str,
    strike: f64,
    call: bool,
) -> crate::cli::ContractArgs {
    crate::cli::ContractArgs {
        symbol: symbol.to_string(),
        expiration: expiration.to_string(),
        strike,
        call,
        put: !call,
    }
}

fn test_raw_option_contract() -> OptionContract {
    serde_json::from_value(json!({
        "symbol": "AAPL  260116C00100000",
        "description": "AAPL Jan 16 2026 100 Call",
        "putCall": "CALL",
        "bidPrice": 1.0,
        "askPrice": 2.0,
        "markPrice": 1.5,
        "lastPrice": 1.4,
        "strikePrice": 100.0,
        "daysToExpiration": 2,
        "delta": 0.5,
        "gamma": 0.1,
        "theta": -0.02,
        "vega": 0.3,
        "rho": 0.04,
        "volatility": 25.0,
        "openInterest": 500.0,
        "totalVolume": 200.0,
        "isInTheMoney": true,
        "theoreticalOptionValue": 1.45,
        "intrinsicValue": 0.5,
        "timeValue": 0.95,
        "multiplier": 100.0,
        "settlementType": "P",
        "expirationType": "S",
    }))
    .expect("test OptionContract should deserialize")
}

#[test]
fn screen_without_filters_returns_all_contracts_with_scan_metadata() {
    let args = screen_args();

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");

    assert_eq!(output["rowCount"], Value::from(8));
    assert_eq!(output["totalScanned"], Value::from(8));
    assert_eq!(output["underlying"], Value::String("AAPL".to_string()));
    assert_eq!(output["underlyingPrice"], number_value(123.45));
    assert_eq!(
        output["columns"].as_array().expect("columns array").len(),
        16
    );
    assert!(
        output["filtersApplied"]
            .as_array()
            .expect("filters array")
            .is_empty()
    );
}

#[test]
fn screen_min_volume_filters_low_volume_contracts() {
    let mut args = screen_args_with_symbols();
    args.min_volume = Some(100);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");
    let symbols = screen_symbols(&output);

    assert_eq!(output["rowCount"], Value::from(7));
    assert!(!symbols.contains(&"LOWVOL".to_string()));
    assert!(symbols.contains(&"GOOD".to_string()));
}

#[test]
fn screen_max_spread_pct_filters_wide_spreads_and_zero_mark() {
    let mut args = screen_args_with_symbols();
    args.max_spread_pct = Some(5.0);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");
    let symbols = screen_symbols(&output);

    assert!(symbols.contains(&"GOOD".to_string()));
    assert!(symbols.contains(&"LOWVOL".to_string()));
    assert!(!symbols.contains(&"WIDESPREAD".to_string()));
    assert!(!symbols.contains(&"ZEROMARK".to_string()));
}

#[test]
fn screen_premium_range_filters_by_mark_price() {
    let mut args = screen_args_with_symbols();
    args.min_premium = Some(2.0);
    args.max_premium = Some(10.0);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");
    let symbols = screen_symbols(&output);

    assert!(symbols.contains(&"GOOD".to_string()));
    assert!(symbols.contains(&"PUTGOOD".to_string()));
    assert!(!symbols.contains(&"CHEAP".to_string()));
    assert!(!symbols.contains(&"HIGHPREM".to_string()));
}

#[test]
fn screen_min_oi_filters_low_open_interest_contracts() {
    let mut args = screen_args_with_symbols();
    args.min_oi = Some(500);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");
    let symbols = screen_symbols(&output);

    assert_eq!(output["rowCount"], Value::from(7));
    assert!(!symbols.contains(&"LOWOI".to_string()));
}

#[test]
fn screen_min_bid_filters_cheap_contracts() {
    let mut args = screen_args_with_symbols();
    args.min_bid = Some(0.50);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");
    let symbols = screen_symbols(&output);

    assert_eq!(output["rowCount"], Value::from(6));
    assert!(!symbols.contains(&"CHEAP".to_string()));
    assert!(!symbols.contains(&"ZEROMARK".to_string()));
}

#[test]
fn screen_limit_truncates_rows_after_filtering() {
    let mut args = screen_args_with_symbols();
    args.limit = Some(5);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");

    assert_eq!(output["rowCount"], Value::from(5));
    assert_eq!(output["totalScanned"], Value::from(8));
    assert_eq!(output["rows"].as_array().expect("rows array").len(), 5);
}

#[test]
fn screen_filters_applied_contains_human_readable_descriptions() {
    let mut args = screen_args_with_symbols();
    args.min_volume = Some(100);
    args.max_spread_pct = Some(5.0);
    args.min_premium = Some(2.0);
    args.max_premium = Some(10.0);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");
    let filters = screen_filters(&output);

    assert!(filters.contains(&"volume >= 100".to_string()));
    assert!(filters.contains(&"spreadPct <= 5".to_string()));
    assert!(filters.contains(&"premium >= 2".to_string()));
    assert!(filters.contains(&"premium <= 10".to_string()));
}

#[test]
fn screen_total_scanned_reflects_pre_filter_count() {
    let mut args = screen_args_with_symbols();
    args.min_volume = Some(100);
    args.min_oi = Some(500);
    args.min_bid = Some(0.50);
    args.max_spread_pct = Some(5.0);
    args.min_premium = Some(2.0);
    args.max_premium = Some(10.0);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");

    assert_eq!(output["rowCount"], Value::from(2));
    assert_eq!(output["totalScanned"], Value::from(8));
}

#[test]
fn screen_combined_filters_require_all_conditions_to_match() {
    let mut args = screen_args_with_symbols();
    args.min_volume = Some(100);
    args.min_oi = Some(500);
    args.min_bid = Some(0.50);
    args.max_spread_pct = Some(5.0);
    args.min_premium = Some(2.0);
    args.max_premium = Some(10.0);

    let output = screen_chain(&screen_option_chain(), &args).expect("screen should succeed");
    let symbols = screen_symbols(&output);

    assert_eq!(symbols, vec!["PUTGOOD".to_string(), "GOOD".to_string()]);
}

fn screen_option_chain() -> OptionChain {
    let contracts = json!({
        "100.0": [
            screen_contract_json("GOOD", "CALL", 100.0, 2.45, 2.55, 2.50, 150.0, 600.0, 0.40),
            screen_contract_json("LOWVOL", "CALL", 100.0, 2.00, 2.10, 2.05, 50.0, 700.0, 0.35)
        ],
        "105.0": [
            screen_contract_json("WIDESPREAD", "CALL", 105.0, 2.00, 3.00, 2.50, 200.0, 700.0, 0.30),
            screen_contract_json("ZEROMARK", "CALL", 105.0, 0.10, 0.20, 0.00, 200.0, 700.0, 0.20)
        ],
        "110.0": [
            screen_contract_json("CHEAP", "CALL", 110.0, 0.25, 0.35, 0.30, 200.0, 700.0, 0.10),
            screen_contract_json("LOWOI", "CALL", 110.0, 2.00, 2.10, 2.05, 200.0, 100.0, 0.25)
        ],
        "115.0": [
            screen_contract_json("HIGHPREM", "CALL", 115.0, 11.00, 12.00, 11.50, 200.0, 700.0, 0.60)
        ]
    });
    let puts = json!({
        "95.0": [
            screen_contract_json("PUTGOOD", "PUT", 95.0, 3.00, 3.10, 3.05, 300.0, 800.0, -0.25)
        ]
    });

    serde_json::from_value(json!({
        "symbol": "AAPL",
        "status": "SUCCESS",
        "underlyingPrice": 123.45,
        "callExpDateMap": {
            "2026-01-16:30": contracts
        },
        "putExpDateMap": {
            "2026-01-16:30": puts
        }
    }))
    .expect("screen option chain should deserialize")
}

fn screen_contract_json(
    symbol: &str,
    contract_type: &str,
    strike: f64,
    bid: f64,
    ask: f64,
    mark: f64,
    volume: f64,
    oi: f64,
    delta: f64,
) -> Value {
    json!({
        "symbol": symbol,
        "description": format!("{symbol} test contract"),
        "putCall": contract_type,
        "bidPrice": bid,
        "askPrice": ask,
        "markPrice": mark,
        "lastPrice": mark,
        "closePrice": mark - 0.1,
        "highPrice": mark + 0.2,
        "lowPrice": mark - 0.2,
        "strikePrice": strike,
        "daysToExpiration": 30.0,
        "delta": delta,
        "gamma": 0.1,
        "theta": -0.02,
        "vega": 0.3,
        "rho": 0.04,
        "volatility": 25.0,
        "openInterest": oi,
        "totalVolume": volume,
        "isInTheMoney": false,
        "theoreticalOptionValue": mark,
        "intrinsicValue": 0.0,
        "timeValue": mark,
        "multiplier": 100.0,
        "settlementType": "P",
        "expirationType": "S",
        "percentChange": 2.5,
        "markChange": 0.1,
        "markPercentChange": 3.0,
    })
}

fn screen_args_with_symbols() -> ScreenArgs {
    ScreenArgs {
        fields: Some("symbol".to_string()),
        ..screen_args()
    }
}

fn screen_args() -> ScreenArgs {
    ScreenArgs {
        symbol: "AAPL".to_string(),
        contract_type: None,
        dte_min: None,
        dte_max: None,
        expiration: None,
        delta_min: None,
        delta_max: None,
        fields: None,
        strike_count: None,
        strike: None,
        strike_min: None,
        strike_max: None,
        strike_range: None,
        min_bid: None,
        max_ask: None,
        min_volume: None,
        min_oi: None,
        max_spread_pct: None,
        min_premium: None,
        max_premium: None,
        sort: None,
        limit: None,
    }
}

fn screen_symbols(output: &Value) -> Vec<String> {
    output["rows"]
        .as_array()
        .expect("rows array")
        .iter()
        .map(|row| row[0].as_str().expect("symbol string").to_string())
        .collect()
}

fn screen_filters(output: &Value) -> Vec<String> {
    output["filtersApplied"]
        .as_array()
        .expect("filters array")
        .iter()
        .map(|filter| filter.as_str().expect("filter string").to_string())
        .collect()
}
