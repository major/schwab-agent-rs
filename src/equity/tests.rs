use clap::Parser;

use crate::cli::Cli;
use crate::shared::{DurationChoice, SessionChoice};

// ---------------------------------------------------------------------------
// CLI parsing tests
// ---------------------------------------------------------------------------

#[test]
fn parse_build_buy_market() {
    let cli = Cli::parse_from(["schwab-agent", "stock", "build", "buy", "AAPL"]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_build_buy_limit() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "build",
        "buy",
        "AAPL",
        "--order-type",
        "limit",
        "--price",
        "150.00",
        "--quantity",
        "10",
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_build_sell() {
    let cli = Cli::parse_from(["schwab-agent", "stock", "build", "sell", "AAPL"]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_build_sell_short() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "build",
        "sell-short",
        "TSLA",
        "--order-type",
        "limit",
        "--price",
        "250.00",
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_build_buy_to_cover() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "build",
        "buy-to-cover",
        "TSLA",
        "--order-type",
        "stop",
        "--stop-price",
        "275.00",
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_build_stop_limit() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "build",
        "buy",
        "SPY",
        "--order-type",
        "stop-limit",
        "--price",
        "500.00",
        "--stop-price",
        "495.00",
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_preview_with_save() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "preview",
        "--account",
        "ABC123",
        "--save-preview",
        "buy",
        "AAPL",
        "--order-type",
        "limit",
        "--price",
        "150.00",
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_place() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "place",
        "--account",
        "ABC123",
        "sell",
        "AAPL",
        "--quantity",
        "50",
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_place_from_preview() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "place-from-preview",
        "--account",
        "ABC123",
        "--digest",
        "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_preview_raw() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "preview-raw",
        "--account",
        "ABC123",
        "--json",
        r#"{"orderType":"MARKET"}"#,
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_place_raw() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "place-raw",
        "--account",
        "ABC123",
        "--json",
        r#"{"orderType":"MARKET"}"#,
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

#[test]
fn parse_with_session_and_duration() {
    let cli = Cli::parse_from([
        "schwab-agent",
        "stock",
        "build",
        "buy",
        "AAPL",
        "--session",
        "am",
        "--duration",
        "gtc",
    ]);
    assert!(matches!(cli.command, crate::cli::Command::Stock(_)));
}

// ---------------------------------------------------------------------------
// Instruction hardcoding tests
// ---------------------------------------------------------------------------

#[test]
fn buy_hardcodes_buy_instruction() {
    let action = super::EquityAction::Buy(market_args());
    let order = super::build_equity_order(&action).unwrap();
    let json = serde_json::to_value(&order).unwrap();
    assert_eq!(json["orderLegCollection"][0]["instruction"], "BUY");
}

#[test]
fn sell_hardcodes_sell_instruction() {
    let action = super::EquityAction::Sell(market_args());
    let order = super::build_equity_order(&action).unwrap();
    let json = serde_json::to_value(&order).unwrap();
    assert_eq!(json["orderLegCollection"][0]["instruction"], "SELL");
}

#[test]
fn sell_short_hardcodes_sell_short_instruction() {
    let action = super::EquityAction::SellShort(market_args());
    let order = super::build_equity_order(&action).unwrap();
    let json = serde_json::to_value(&order).unwrap();
    assert_eq!(json["orderLegCollection"][0]["instruction"], "SELL_SHORT");
}

#[test]
fn buy_to_cover_hardcodes_buy_to_cover_instruction() {
    let action = super::EquityAction::BuyToCover(market_args());
    let order = super::build_equity_order(&action).unwrap();
    let json = serde_json::to_value(&order).unwrap();
    assert_eq!(json["orderLegCollection"][0]["instruction"], "BUY_TO_COVER");
}

// ---------------------------------------------------------------------------
// Build output validation
// ---------------------------------------------------------------------------

#[test]
fn do_build_market_order() {
    let action = super::EquityAction::Buy(market_args());
    let json = super::do_build(&action).unwrap();

    assert_eq!(json["orderType"], "MARKET");
    assert_eq!(json["session"], "NORMAL");
    assert_eq!(json["duration"], "DAY");
    assert_eq!(json["orderStrategyType"], "SINGLE");
    assert!(json.get("price").is_none());
    assert!(json.get("stopPrice").is_none());

    let leg = &json["orderLegCollection"][0];
    assert_eq!(leg["instrument"]["assetType"], "EQUITY");
    assert_eq!(leg["instrument"]["symbol"], "AAPL");
}

#[test]
fn do_build_limit_order() {
    let action = super::EquityAction::Buy(limit_args());
    let json = super::do_build(&action).unwrap();

    assert_eq!(json["orderType"], "LIMIT");
    assert!(json.get("price").is_some());
    assert!(json.get("stopPrice").is_none());
}

#[test]
fn do_build_stop_order() {
    let args = super::EquityArgs {
        symbol: "AAPL".to_string(),
        quantity: 1,
        order_type: super::OrderTypeChoice::Stop,
        price: None,
        stop_price: Some(140.0),
        session: SessionChoice::Normal,
        duration: DurationChoice::Day,
    };
    let action = super::EquityAction::Sell(args);
    let json = super::do_build(&action).unwrap();

    assert_eq!(json["orderType"], "STOP");
    assert!(json.get("price").is_none());
    assert!(json.get("stopPrice").is_some());
}

#[test]
fn do_build_stop_limit_order() {
    let args = super::EquityArgs {
        symbol: "AAPL".to_string(),
        quantity: 5,
        order_type: super::OrderTypeChoice::StopLimit,
        price: Some(150.0),
        stop_price: Some(145.0),
        session: SessionChoice::Normal,
        duration: DurationChoice::Day,
    };
    let action = super::EquityAction::Buy(args);
    let json = super::do_build(&action).unwrap();

    assert_eq!(json["orderType"], "STOP_LIMIT");
    assert!(json.get("price").is_some());
    assert!(json.get("stopPrice").is_some());
}

#[test]
fn do_build_with_custom_session_and_duration() {
    let args = super::EquityArgs {
        symbol: "SPY".to_string(),
        quantity: 10,
        order_type: super::OrderTypeChoice::Market,
        price: None,
        stop_price: None,
        session: SessionChoice::Am,
        duration: DurationChoice::GoodTillCancel,
    };
    let action = super::EquityAction::Buy(args);
    let json = super::do_build(&action).unwrap();

    assert_eq!(json["session"], "AM");
    assert_eq!(json["duration"], "GOOD_TILL_CANCEL");
}

// ---------------------------------------------------------------------------
// Validation tests
// ---------------------------------------------------------------------------

#[test]
fn limit_order_requires_price() {
    let args = super::EquityArgs {
        symbol: "AAPL".to_string(),
        quantity: 1,
        order_type: super::OrderTypeChoice::Limit,
        price: None,
        stop_price: None,
        session: SessionChoice::Normal,
        duration: DurationChoice::Day,
    };
    let action = super::EquityAction::Buy(args);
    let result = super::build_equity_order(&action);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--price"));
}

#[test]
fn stop_order_requires_stop_price() {
    let args = super::EquityArgs {
        symbol: "AAPL".to_string(),
        quantity: 1,
        order_type: super::OrderTypeChoice::Stop,
        price: None,
        stop_price: None,
        session: SessionChoice::Normal,
        duration: DurationChoice::Day,
    };
    let action = super::EquityAction::Buy(args);
    let result = super::build_equity_order(&action);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--stop-price"));
}

#[test]
fn stop_limit_requires_price() {
    let args = super::EquityArgs {
        symbol: "AAPL".to_string(),
        quantity: 1,
        order_type: super::OrderTypeChoice::StopLimit,
        price: None,
        stop_price: Some(145.0),
        session: SessionChoice::Normal,
        duration: DurationChoice::Day,
    };
    let action = super::EquityAction::Buy(args);
    let result = super::build_equity_order(&action);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--price"));
}

#[test]
fn stop_limit_requires_stop_price() {
    let args = super::EquityArgs {
        symbol: "AAPL".to_string(),
        quantity: 1,
        order_type: super::OrderTypeChoice::StopLimit,
        price: Some(150.0),
        stop_price: None,
        session: SessionChoice::Normal,
        duration: DurationChoice::Day,
    };
    let action = super::EquityAction::Buy(args);
    let result = super::build_equity_order(&action);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--stop-price"));
}

// ---------------------------------------------------------------------------
// Action name tests
// ---------------------------------------------------------------------------

#[test]
fn action_name_coverage() {
    assert_eq!(
        super::action_name(&super::EquityAction::Buy(market_args())),
        "buy"
    );
    assert_eq!(
        super::action_name(&super::EquityAction::Sell(market_args())),
        "sell"
    );
    assert_eq!(
        super::action_name(&super::EquityAction::SellShort(market_args())),
        "sell-short"
    );
    assert_eq!(
        super::action_name(&super::EquityAction::BuyToCover(market_args())),
        "buy-to-cover"
    );
}

// ---------------------------------------------------------------------------
// Raw JSON parsing tests
// ---------------------------------------------------------------------------

#[test]
fn parse_raw_json_valid() {
    let result = super::parse_raw_json(r#"{"orderType":"MARKET"}"#);
    assert!(result.is_ok());
}

#[test]
fn parse_raw_json_invalid() {
    let result = super::parse_raw_json("not json");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid JSON"));
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Builds a simple market order args for instruction hardcoding tests.
fn market_args() -> super::EquityArgs {
    super::EquityArgs {
        symbol: "AAPL".to_string(),
        quantity: 1,
        order_type: super::OrderTypeChoice::Market,
        price: None,
        stop_price: None,
        session: SessionChoice::Normal,
        duration: DurationChoice::Day,
    }
}

/// Builds limit order args for build output tests.
fn limit_args() -> super::EquityArgs {
    super::EquityArgs {
        symbol: "AAPL".to_string(),
        quantity: 10,
        order_type: super::OrderTypeChoice::Limit,
        price: Some(150.0),
        stop_price: None,
        session: SessionChoice::Normal,
        duration: DurationChoice::Day,
    }
}
