use schwab::{
    Account, AccountCashEquivalent, AccountEquity, AccountFixedIncome, AccountMutualFund,
    AccountOption, AccountsInstrument, CashAccount, CashBalance, InstrumentAssetType,
    MarginAccount, MarginBalance, Position, SecuritiesAccount,
};

use super::*;

/// Convert a float literal to [`schwab::Number`] for test assertions.
///
/// Under the default feature set `Number` is `f64` so this is a no-op.
/// With `--features decimal` the value is parsed through its string
/// representation, matching the serde round-trip path.
#[cfg(not(feature = "decimal"))]
fn num(v: f64) -> schwab::Number {
    v
}

/// Convert a float literal to [`schwab::Number`] for test assertions.
///
/// Under the default feature set `Number` is `f64` so this is a no-op.
/// With `--features decimal` the value is parsed through its string
/// representation, matching the serde round-trip path.
#[cfg(feature = "decimal")]
fn num(v: f64) -> schwab::Number {
    use core::str::FromStr;
    schwab::Number::from_str(&format!("{v}")).expect("test float must be a valid Decimal")
}

// -- helpers --------------------------------------------------------------

/// Build a Position with all None fields except the ones we set.
fn bare_position() -> Position {
    Position {
        aged_quantity: None,
        average_long_price: None,
        average_price: None,
        average_short_price: None,
        current_day_cost: None,
        current_day_profit_loss: None,
        current_day_profit_loss_percentage: None,
        instrument: None,
        long_open_profit_loss: None,
        long_quantity: None,
        maintenance_requirement: None,
        market_value: None,
        previous_session_long_quantity: None,
        previous_session_short_quantity: None,
        settled_long_quantity: None,
        settled_short_quantity: None,
        short_open_profit_loss: None,
        short_quantity: None,
        tax_lot_average_long_price: None,
        tax_lot_average_short_price: None,
    }
}

fn bare_margin_account() -> MarginAccount {
    MarginAccount {
        account_number: None,
        is_closing_only_restricted: None,
        is_day_trader: None,
        pfcb_flag: None,
        positions: None,
        round_trips: None,
        r#type: None,
        current_balances: None,
        initial_balances: None,
        projected_balances: None,
    }
}

fn bare_cash_account() -> CashAccount {
    CashAccount {
        account_number: None,
        is_closing_only_restricted: None,
        is_day_trader: None,
        pfcb_flag: None,
        positions: None,
        round_trips: None,
        r#type: None,
        current_balances: None,
        initial_balances: None,
        projected_balances: None,
    }
}

fn bare_margin_balance() -> MarginBalance {
    MarginBalance {
        available_funds: None,
        available_funds_non_marginable_trade: None,
        buying_power: None,
        buying_power_non_marginable_trade: None,
        day_trading_buying_power: None,
        day_trading_buying_power_call: None,
        equity: None,
        equity_percentage: None,
        is_in_call: None,
        long_margin_value: None,
        maintenance_call: None,
        maintenance_requirement: None,
        margin_balance: None,
        option_buying_power: None,
        reg_t_call: None,
        short_balance: None,
        short_margin_value: None,
        sma: None,
        stock_buying_power: None,
    }
}

fn bare_cash_balance() -> CashBalance {
    CashBalance {
        cash_available_for_trading: None,
        cash_available_for_withdrawal: None,
        cash_call: None,
        cash_debit_call_value: None,
        long_non_marginable_market_value: None,
        total_cash: None,
        unsettled_cash: None,
    }
}

fn equity_instrument() -> AccountsInstrument {
    AccountsInstrument::Equity(AccountEquity {
        asset_type: Some(InstrumentAssetType::Equity),
        cusip: Some("037833100".into()),
        description: Some("Apple Inc".into()),
        instrument_id: Some(12345),
        net_change: Some(num(1.5)),
        symbol: Some("AAPL".into()),
    })
}

// -- AccountSummary::default -----------------------------------------------

#[test]
fn account_summary_default_has_all_none() {
    let summary = AccountSummary::default();
    assert!(summary.account_number.is_none());
    assert!(summary.account_type.is_none());
    assert!(summary.is_closing_only_restricted.is_none());
    assert!(summary.is_day_trader.is_none());
    assert!(summary.balances.is_none());
    assert!(summary.positions.is_none());
}

// -- summarize_account ----------------------------------------------------

#[test]
fn summarize_account_none_returns_empty() {
    let account = Account {
        securities_account: None,
    };
    let summary = summarize_account(account, true);
    assert!(summary.account_type.is_none());
    assert!(summary.balances.is_none());
}

#[test]
fn summarize_account_margin_variant() {
    let mut ma = bare_margin_account();
    ma.account_number = Some("11111111".into());
    let account = Account {
        securities_account: Some(SecuritiesAccount::Margin(ma)),
    };
    let summary = summarize_account(account, false);
    assert_eq!(summary.account_type, Some("MARGIN"));
    assert_eq!(summary.account_number.as_deref(), Some("11111111"));
}

#[test]
fn summarize_account_cash_variant() {
    let mut ca = bare_cash_account();
    ca.account_number = Some("22222222".into());
    let account = Account {
        securities_account: Some(SecuritiesAccount::Cash(ca)),
    };
    let summary = summarize_account(account, false);
    assert_eq!(summary.account_type, Some("CASH"));
    assert_eq!(summary.account_number.as_deref(), Some("22222222"));
}

// -- summarize_margin_account ---------------------------------------------

#[test]
fn margin_account_without_positions_flag() {
    let mut ma = bare_margin_account();
    ma.account_number = Some("33333333".into());
    ma.is_day_trader = Some(true);
    ma.is_closing_only_restricted = Some(false);
    ma.positions = Some(vec![bare_position()]);
    let summary = summarize_margin_account(ma, false);
    assert_eq!(summary.account_type, Some("MARGIN"));
    assert_eq!(summary.is_day_trader, Some(true));
    assert_eq!(summary.is_closing_only_restricted, Some(false));
    // positions not included because include_positions is false
    assert!(summary.positions.is_none());
}

#[test]
fn margin_account_with_positions_flag() {
    let mut ma = bare_margin_account();
    ma.positions = Some(vec![bare_position()]);
    let summary = summarize_margin_account(ma, true);
    assert!(summary.positions.is_some());
    assert_eq!(summary.positions.unwrap().len(), 1);
}

#[test]
fn margin_account_with_balances() {
    let mut bal = bare_margin_balance();
    bal.available_funds = Some(num(10_000.0));
    bal.buying_power = Some(num(20_000.0));
    bal.equity = Some(num(50_000.0));
    bal.stock_buying_power = Some(num(15_000.0));
    bal.option_buying_power = Some(num(12_000.0));
    bal.available_funds_non_marginable_trade = Some(num(5_000.0));

    let mut ma = bare_margin_account();
    ma.current_balances = Some(bal);
    let summary = summarize_margin_account(ma, false);

    let b = summary.balances.unwrap();
    assert_eq!(b.cash_available_for_trading, Some(num(10_000.0)));
    assert_eq!(b.cash_available_for_withdrawal, Some(num(5_000.0)));
    assert!(b.total_cash.is_none());
    assert_eq!(b.buying_power, Some(num(20_000.0)));
    assert_eq!(b.stock_buying_power, Some(num(15_000.0)));
    assert_eq!(b.option_buying_power, Some(num(12_000.0)));
    assert_eq!(b.equity, Some(num(50_000.0)));
}

#[test]
fn margin_account_no_positions_field() {
    let ma = bare_margin_account();
    let summary = summarize_margin_account(ma, true);
    // positions is None in the source, so even with flag true we get None
    assert!(summary.positions.is_none());
}

// -- summarize_cash_account -----------------------------------------------

#[test]
fn cash_account_without_positions_flag() {
    let mut ca = bare_cash_account();
    ca.account_number = Some("44444444".into());
    ca.is_day_trader = Some(false);
    ca.is_closing_only_restricted = Some(true);
    ca.positions = Some(vec![bare_position()]);
    let summary = summarize_cash_account(ca, false);
    assert_eq!(summary.account_type, Some("CASH"));
    assert_eq!(summary.is_day_trader, Some(false));
    assert_eq!(summary.is_closing_only_restricted, Some(true));
    assert!(summary.positions.is_none());
}

#[test]
fn cash_account_with_positions_flag() {
    let mut ca = bare_cash_account();
    ca.positions = Some(vec![bare_position(), bare_position()]);
    let summary = summarize_cash_account(ca, true);
    assert!(summary.positions.is_some());
    assert_eq!(summary.positions.unwrap().len(), 2);
}

#[test]
fn cash_account_with_balances() {
    let mut bal = bare_cash_balance();
    bal.cash_available_for_trading = Some(num(8_000.0));
    bal.cash_available_for_withdrawal = Some(num(6_000.0));
    bal.total_cash = Some(num(10_000.0));

    let mut ca = bare_cash_account();
    ca.current_balances = Some(bal);
    let summary = summarize_cash_account(ca, false);

    let b = summary.balances.unwrap();
    assert_eq!(b.cash_available_for_trading, Some(num(8_000.0)));
    assert_eq!(b.cash_available_for_withdrawal, Some(num(6_000.0)));
    assert_eq!(b.total_cash, Some(num(10_000.0)));
    assert!(b.buying_power.is_none());
    assert!(b.stock_buying_power.is_none());
    assert!(b.option_buying_power.is_none());
    assert!(b.equity.is_none());
}

#[test]
fn cash_account_no_positions_field() {
    let ca = bare_cash_account();
    let summary = summarize_cash_account(ca, true);
    assert!(summary.positions.is_none());
}

// -- BalanceSummary -------------------------------------------------------

#[test]
fn balance_summary_from_margin_all_none() {
    let bal = bare_margin_balance();
    let summary = BalanceSummary::from(bal);
    assert!(summary.cash_available_for_trading.is_none());
    assert!(summary.cash_available_for_withdrawal.is_none());
    assert!(summary.total_cash.is_none());
    assert!(summary.buying_power.is_none());
    assert!(summary.stock_buying_power.is_none());
    assert!(summary.option_buying_power.is_none());
    assert!(summary.equity.is_none());
}

#[test]
fn balance_summary_from_cash_all_none() {
    let bal = bare_cash_balance();
    let summary = BalanceSummary::from(bal);
    assert!(summary.cash_available_for_trading.is_none());
    assert!(summary.cash_available_for_withdrawal.is_none());
    assert!(summary.total_cash.is_none());
    assert!(summary.buying_power.is_none());
    assert!(summary.stock_buying_power.is_none());
    assert!(summary.option_buying_power.is_none());
    assert!(summary.equity.is_none());
}

// -- summarize_positions --------------------------------------------------

#[test]
fn summarize_positions_none() {
    assert!(summarize_positions(None).is_none());
}

#[test]
fn summarize_positions_empty_vec() {
    let result = summarize_positions(Some(vec![]));
    assert!(result.is_some());
    assert!(result.unwrap().is_empty());
}

#[test]
fn summarize_positions_with_entries() {
    let mut pos = bare_position();
    pos.long_quantity = Some(num(100.0));
    pos.instrument = Some(equity_instrument());
    let result = summarize_positions(Some(vec![pos])).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].symbol.as_deref(), Some("AAPL"));
    assert_eq!(result[0].long_quantity, Some(num(100.0)));
}

// -- PositionSummary::from ------------------------------------------------

#[test]
fn position_summary_from_no_instrument() {
    let mut pos = bare_position();
    pos.long_quantity = Some(num(50.0));
    pos.short_quantity = Some(num(10.0));
    pos.average_price = Some(num(150.0));
    pos.market_value = Some(num(7500.0));
    pos.current_day_profit_loss = Some(num(200.0));
    pos.current_day_profit_loss_percentage = Some(num(2.5));

    let summary = PositionSummary::from(pos);
    assert!(summary.symbol.is_none());
    assert!(summary.description.is_none());
    assert!(summary.asset_type.is_none());
    assert_eq!(summary.long_quantity, Some(num(50.0)));
    assert_eq!(summary.short_quantity, Some(num(10.0)));
    assert_eq!(summary.average_price, Some(num(150.0)));
    assert_eq!(summary.market_value, Some(num(7500.0)));
    assert_eq!(summary.current_day_profit_loss, Some(num(200.0)));
    assert_eq!(summary.current_day_profit_loss_percentage, Some(num(2.5)));
}

#[test]
fn position_summary_from_with_equity_instrument() {
    let mut pos = bare_position();
    pos.instrument = Some(equity_instrument());
    pos.market_value = Some(num(15_000.0));

    let summary = PositionSummary::from(pos);
    assert_eq!(summary.symbol.as_deref(), Some("AAPL"));
    assert_eq!(summary.description.as_deref(), Some("Apple Inc"));
    assert_eq!(summary.asset_type.as_deref(), Some("Equity"));
    assert_eq!(summary.market_value, Some(num(15_000.0)));
}

// -- InstrumentSummary::from for each variant -----------------------------

#[test]
fn instrument_summary_from_equity() {
    let instrument = AccountsInstrument::Equity(AccountEquity {
        asset_type: Some(InstrumentAssetType::Equity),
        cusip: None,
        description: Some("Microsoft Corp".into()),
        instrument_id: None,
        net_change: None,
        symbol: Some("MSFT".into()),
    });
    let summary = InstrumentSummary::from(instrument);
    assert_eq!(summary.symbol.as_deref(), Some("MSFT"));
    assert_eq!(summary.description.as_deref(), Some("Microsoft Corp"));
    assert_eq!(summary.asset_type.as_deref(), Some("Equity"));
}

#[test]
fn instrument_summary_from_option() {
    let instrument = AccountsInstrument::Option(AccountOption {
        asset_type: Some(InstrumentAssetType::Option),
        cusip: None,
        description: Some("AAPL Jan 170 Call".into()),
        instrument_id: None,
        net_change: None,
        symbol: Some("AAPL  240119C00170000".into()),
        option_deliverables: None,
        option_multiplier: None,
        put_call: None,
        r#type: None,
        underlying_symbol: None,
    });
    let summary = InstrumentSummary::from(instrument);
    assert_eq!(summary.symbol.as_deref(), Some("AAPL  240119C00170000"));
    assert_eq!(summary.description.as_deref(), Some("AAPL Jan 170 Call"));
    assert_eq!(summary.asset_type.as_deref(), Some("Option"));
}

#[test]
fn instrument_summary_from_fixed_income() {
    let instrument = AccountsInstrument::FixedIncome(AccountFixedIncome {
        asset_type: Some(InstrumentAssetType::FixedIncome),
        cusip: Some("912828YK0".into()),
        description: Some("US Treasury Bond".into()),
        instrument_id: None,
        net_change: None,
        symbol: Some("USTB".into()),
        factor: None,
        maturity_date: None,
        variable_rate: None,
    });
    let summary = InstrumentSummary::from(instrument);
    assert_eq!(summary.symbol.as_deref(), Some("USTB"));
    assert_eq!(summary.description.as_deref(), Some("US Treasury Bond"));
    assert_eq!(summary.asset_type.as_deref(), Some("FixedIncome"));
}

#[test]
fn instrument_summary_from_cash_equivalent() {
    let instrument = AccountsInstrument::CashEquivalent(AccountCashEquivalent {
        asset_type: Some(InstrumentAssetType::CashEquivalent),
        cusip: None,
        description: Some("Sweep Vehicle".into()),
        instrument_id: None,
        net_change: None,
        symbol: Some("MMDA1".into()),
        r#type: None,
    });
    let summary = InstrumentSummary::from(instrument);
    assert_eq!(summary.symbol.as_deref(), Some("MMDA1"));
    assert_eq!(summary.description.as_deref(), Some("Sweep Vehicle"));
    assert_eq!(summary.asset_type.as_deref(), Some("CashEquivalent"));
}

#[test]
fn instrument_summary_from_mutual_fund() {
    let instrument = AccountsInstrument::MutualFund(AccountMutualFund {
        asset_type: Some(InstrumentAssetType::MutualFund),
        cusip: Some("922908769".into()),
        description: Some("Vanguard Total Stock".into()),
        instrument_id: None,
        net_change: None,
        symbol: Some("VTSAX".into()),
    });
    let summary = InstrumentSummary::from(instrument);
    assert_eq!(summary.symbol.as_deref(), Some("VTSAX"));
    assert_eq!(summary.description.as_deref(), Some("Vanguard Total Stock"));
    assert_eq!(summary.asset_type.as_deref(), Some("MutualFund"));
}

#[test]
fn instrument_summary_with_no_asset_type() {
    let instrument = AccountsInstrument::Equity(AccountEquity {
        asset_type: None,
        cusip: None,
        description: None,
        instrument_id: None,
        net_change: None,
        symbol: Some("XYZ".into()),
    });
    let summary = InstrumentSummary::from(instrument);
    assert_eq!(summary.symbol.as_deref(), Some("XYZ"));
    assert!(summary.description.is_none());
    assert!(summary.asset_type.is_none());
}

// -- serialization round-trip ---------------------------------------------

#[test]
fn account_summary_serializes_to_json() {
    let mut bal = bare_margin_balance();
    bal.available_funds = Some(num(1_000.0));
    bal.buying_power = Some(num(2_000.0));

    let mut ma = bare_margin_account();
    ma.account_number = Some("99999999".into());
    ma.current_balances = Some(bal);

    let summary = summarize_margin_account(ma, false);
    let json = serde_json::to_value(&summary).expect("serialize");
    assert_eq!(json["account_number"], "99999999");
    assert_eq!(json["account_type"], "MARGIN");
    #[cfg(not(feature = "decimal"))]
    {
        assert_eq!(json["balances"]["cash_available_for_trading"], 1_000.0);
        assert_eq!(json["balances"]["buying_power"], 2_000.0);
    }
    #[cfg(feature = "decimal")]
    {
        assert_eq!(json["balances"]["cash_available_for_trading"], "1000");
        assert_eq!(json["balances"]["buying_power"], "2000");
    }
    // None fields are skipped
    assert!(json.get("positions").is_none());
}

#[test]
fn portfolio_snapshot_serializes_accounts() {
    let snapshot = PortfolioSnapshot {
        accounts: vec![AccountSummary::default()],
    };
    let json = serde_json::to_value(&snapshot).expect("serialize");
    assert_eq!(json["accounts"].as_array().unwrap().len(), 1);
}
