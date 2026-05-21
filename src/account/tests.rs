use crate::account::{
    AccountBalances, AccountResolveData, AccountRow, AccountSummaryData, CashBalanceSummary,
    MarginBalanceSummary, build_account_row, ensure_selected_account_rendered,
    render_summary_from_data, resolve_account_from_data, resolve_default_account_hash_from_data,
    retain_account_summary,
};
use crate::error::AppError;
use schwab::{
    AccountCashEquivalent, AccountEquity, AccountFixedIncome, AccountMutualFund, AccountOption,
    AccountsInstrument, InstrumentAssetType,
};

use super::{instrument_summary, preference_accounts};

fn number(value: f64) -> schwab::Number {
    serde_json::from_value(serde_json::json!(value)).unwrap()
}

#[test]
fn account_summary_serializes_correctly() {
    let summary = AccountSummaryData {
        accounts: vec![
            AccountRow {
                account_hash: "hash-1".to_string(),
                nickname: Some("margin".to_string()),
                display_account_id: Some("****1234".to_string()),
                primary_account: Some(true),
                account_type: Some("MARGIN".to_string()),
                is_closing_only_restricted: Some(false),
                is_day_trader: Some(true),
                balances: Some(AccountBalances::Margin(MarginBalanceSummary {
                    cash_available_for_trading: Some(number(10.0)),
                    cash_available_for_withdrawal: Some(number(11.0)),
                    buying_power: Some(number(12.0)),
                    stock_buying_power: Some(number(13.0)),
                    option_buying_power: Some(number(14.0)),
                    equity: Some(number(15.0)),
                })),
                positions: None,
            },
            AccountRow {
                account_hash: "hash-2".to_string(),
                nickname: Some("cash".to_string()),
                display_account_id: Some("****5678".to_string()),
                primary_account: Some(false),
                account_type: Some("CASH".to_string()),
                is_closing_only_restricted: None,
                is_day_trader: None,
                balances: Some(AccountBalances::Cash(CashBalanceSummary {
                    cash_available_for_trading: Some(number(20.0)),
                    cash_available_for_withdrawal: Some(number(21.0)),
                    total_cash: Some(number(22.0)),
                })),
                positions: None,
            },
        ],
    };

    let serialized = serde_json::to_value(summary).unwrap();
    let accounts = serialized["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0]["account_hash"], "hash-1");
    assert_eq!(accounts[1]["account_hash"], "hash-2");
    assert_eq!(accounts[0]["is_closing_only_restricted"], false);
    assert_eq!(accounts[0]["is_day_trader"], true);
    assert!(accounts[1].get("is_closing_only_restricted").is_none());
    assert!(accounts[1].get("is_day_trader").is_none());
    assert!(accounts[0].get("positions").is_none());
    assert!(accounts[1].get("positions").is_none());
}

#[test]
fn account_resolve_serializes_correctly() {
    let resolve = AccountResolveData {
        account_hash: "hash-1".to_string(),
        matched_by: "nickname".to_string(),
        nickname: Some("primary".to_string()),
        display_account_id: Some("****1234".to_string()),
        primary_account: Some(true),
        account_type: Some("MARGIN".to_string()),
    };

    let serialized = serde_json::to_value(resolve).unwrap();
    assert_eq!(serialized["account_hash"], "hash-1");
    assert_eq!(serialized["matched_by"], "nickname");
    assert_eq!(serialized["nickname"], "primary");
    assert_eq!(serialized["display_account_id"], "****1234");
    assert_eq!(serialized["primary_account"], true);
    assert_eq!(serialized["account_type"], "MARGIN");
}

#[test]
fn account_row_omits_absent_optional_fields() {
    let row = AccountRow {
        account_hash: "hash-1".to_string(),
        nickname: None,
        display_account_id: None,
        primary_account: None,
        account_type: None,
        is_closing_only_restricted: None,
        is_day_trader: None,
        balances: None,
        positions: None,
    };

    let serialized = serde_json::to_value(row).unwrap();
    assert_eq!(serialized["account_hash"], "hash-1");
    assert!(serialized.get("nickname").is_none());
    assert!(serialized.get("display_account_id").is_none());
    assert!(serialized.get("primary_account").is_none());
    assert!(serialized.get("account_type").is_none());
    assert!(serialized.get("is_closing_only_restricted").is_none());
    assert!(serialized.get("is_day_trader").is_none());
    assert!(serialized.get("balances").is_none());
    assert!(serialized.get("positions").is_none());
}

#[test]
fn account_balances_margin_has_kind_margin() {
    let balances = AccountBalances::Margin(MarginBalanceSummary {
        cash_available_for_trading: Some(number(1.0)),
        cash_available_for_withdrawal: Some(number(2.0)),
        buying_power: Some(number(3.0)),
        stock_buying_power: Some(number(4.0)),
        option_buying_power: Some(number(5.0)),
        equity: Some(number(6.0)),
    });

    let serialized = serde_json::to_string(&balances).unwrap();
    assert!(serialized.contains("\"kind\":\"margin\""));
}

#[test]
fn account_balances_cash_has_kind_cash() {
    let balances = AccountBalances::Cash(CashBalanceSummary {
        cash_available_for_trading: Some(number(1.0)),
        cash_available_for_withdrawal: Some(number(2.0)),
        total_cash: Some(number(3.0)),
    });

    let serialized = serde_json::to_string(&balances).unwrap();
    assert!(serialized.contains("\"kind\":\"cash\""));
}

#[test]
fn account_error_exit_code_is_10() {
    let err = AppError::AccountValidation("test".to_string());
    assert_eq!(err.exit_code(), 10);
}

#[test]
fn account_error_code_is_stable() {
    let err = AppError::AccountValidation("test".to_string());
    assert_eq!(err.code(), "account.validation_failed");
}

#[test]
fn account_error_category_is_account() {
    let err = AppError::AccountValidation("test".to_string());
    assert_eq!(err.category(), "account");
}

#[test]
fn account_error_hint_is_present() {
    let err = AppError::AccountValidation("test".to_string());
    assert!(err.hint().is_some());
}

#[test]
fn account_response_shape_error_is_structured() {
    let err = AppError::AccountResponseShape {
        endpoint: "accountNumbers",
        expected: "array",
        shape: "object(len=1, fields=[errors:array])".to_string(),
    };

    assert_eq!(err.exit_code(), 20);
    assert_eq!(err.code(), "account.response_shape");
    assert_eq!(err.category(), "account");
    assert!(err.hint().is_some());
    assert!(
        err.to_string()
            .contains("object(len=1, fields=[errors:array])")
    );
}

fn make_hash(account_number: &str, hash_value: &str) -> schwab::AccountNumberHash {
    schwab::AccountNumberHash {
        account_number: Some(account_number.to_string()),
        hash_value: Some(hash_value.to_string()),
    }
}

fn make_pref(
    account_number: &str,
    nick_name: Option<&str>,
    display_acct_id: Option<&str>,
    primary: bool,
    acct_type: &str,
) -> schwab::UserPreferenceAccount {
    schwab::UserPreferenceAccount {
        account_color: None,
        account_number: Some(account_number.to_string()),
        auto_position_effect: None,
        display_acct_id: display_acct_id.map(str::to_string),
        nick_name: nick_name.map(str::to_string),
        primary_account: Some(primary),
        r#type: Some(acct_type.to_string()),
    }
}

#[test]
fn build_account_row_with_preference() {
    let pref = make_pref("A1", Some("Trading"), Some("***1234"), true, "MARGIN");
    let row = build_account_row("HASH1".to_string(), Some(&pref));

    assert_eq!(row.account_hash, "HASH1");
    assert_eq!(row.nickname.as_deref(), Some("Trading"));
    assert_eq!(row.display_account_id.as_deref(), Some("***1234"));
    assert_eq!(row.primary_account, Some(true));
    assert_eq!(row.account_type.as_deref(), Some("MARGIN"));

    let serialized = serde_json::to_string(&row).unwrap();
    assert!(!serialized.contains("account_number"));
}

#[test]
fn build_account_row_without_preference() {
    let row = build_account_row("HASH2".to_string(), None);

    assert_eq!(row.account_hash, "HASH2");
    assert!(row.nickname.is_none());
    assert!(row.display_account_id.is_none());
    assert!(row.primary_account.is_none());
    assert!(row.account_type.is_none());

    let serialized = serde_json::to_value(&row).unwrap();
    let object = serialized.as_object().unwrap();
    assert_eq!(object.len(), 1);
    assert_eq!(object["account_hash"], "HASH2");
}

#[test]
fn build_account_row_empty_nickname() {
    let pref = make_pref("A1", Some(""), Some("***1234"), false, "CASH");
    let row = build_account_row("HASH3".to_string(), Some(&pref));

    assert!(row.nickname.is_none());
}

#[test]
fn build_account_row_missing_nick_name() {
    let pref = make_pref("A1", None, Some("***1234"), false, "CASH");
    let row = build_account_row("HASH4".to_string(), Some(&pref));

    assert!(row.nickname.is_none());
}

#[test]
fn join_hash_to_preference_matches_on_account_number() {
    let hashes = [make_hash("A1", "HASH1"), make_hash("A2", "HASH2")];
    let prefs = [
        make_pref("A1", Some("Nick1"), Some("***1111"), true, "MARGIN"),
        make_pref("A2", Some("Nick2"), Some("***2222"), false, "CASH"),
    ];

    let rows: Vec<_> = hashes
        .iter()
        .filter_map(|hash| {
            hash.hash_value.as_ref().map(|hash_value| {
                build_account_row(
                    hash_value.clone(),
                    prefs.iter().find(|pref| {
                        pref.account_number.as_deref() == hash.account_number.as_deref()
                    }),
                )
            })
        })
        .collect();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].account_hash, "HASH1");
    assert_eq!(rows[0].nickname.as_deref(), Some("Nick1"));
    assert_eq!(rows[1].account_hash, "HASH2");
    assert_eq!(rows[1].nickname.as_deref(), Some("Nick2"));
}

#[test]
fn join_asymmetric_hash_without_pref() {
    let hash = make_hash("A3", "HASH3");
    let row = build_account_row(hash.hash_value.clone().unwrap(), None);

    assert_eq!(row.account_hash, "HASH3");
    assert!(row.nickname.is_none());
}

#[test]
fn join_asymmetric_pref_without_hash() {
    let hash = schwab::AccountNumberHash {
        account_number: Some("A4".to_string()),
        hash_value: None,
    };
    let pref = make_pref("A4", Some("Nick4"), Some("***4444"), true, "MARGIN");

    // No hash value means this account cannot produce a row, so skip it.
    let rows: Vec<_> = [hash]
        .into_iter()
        .filter_map(|hash| {
            hash.hash_value
                .as_ref()
                .map(|hash_value| build_account_row(hash_value.clone(), Some(&pref)))
        })
        .collect();

    assert!(rows.is_empty());
}

#[test]
fn serialized_row_never_contains_account_number() {
    let rows = vec![
        build_account_row(
            "HASH1".to_string(),
            Some(&make_pref(
                "A1",
                Some("Nick1"),
                Some("***1111"),
                true,
                "MARGIN",
            )),
        ),
        build_account_row("HASH2".to_string(), None),
    ];

    let serialized = serde_json::to_string(&rows).unwrap();
    assert!(!serialized.contains("account_number"));
}

#[test]
fn account_resolve_hash_match_wins_first() {
    let hashes = [make_hash("A1", "HASH1"), make_hash("A2", "Trading")];
    let prefs = [
        make_pref("A1", Some("Trading"), Some("***1111"), true, "MARGIN"),
        make_pref("A2", Some("Other"), Some("***2222"), false, "CASH"),
    ];

    let resolved = resolve_account_from_data(&hashes, &prefs, "Trading").unwrap();

    assert_eq!(resolved.account_hash, "Trading");
    assert_eq!(resolved.matched_by, "hash");
    assert_eq!(resolved.nickname.as_deref(), Some("Other"));
    assert_eq!(resolved.display_account_id.as_deref(), Some("***2222"));
    assert_eq!(resolved.primary_account, Some(false));
    assert_eq!(resolved.account_type.as_deref(), Some("CASH"));
}

#[test]
fn account_resolve_nickname_match_returns_canonical_hash() {
    let hashes = [make_hash("A1", "HASH1"), make_hash("A2", "HASH2")];
    let prefs = [
        make_pref("A1", Some("Primary"), Some("***1111"), true, "MARGIN"),
        make_pref("A2", Some("Cash"), Some("***2222"), false, "CASH"),
    ];

    let resolved = resolve_account_from_data(&hashes, &prefs, "Cash").unwrap();

    assert_eq!(resolved.account_hash, "HASH2");
    assert_eq!(resolved.matched_by, "nickname");
    assert_eq!(resolved.nickname.as_deref(), Some("Cash"));
    assert_eq!(resolved.display_account_id.as_deref(), Some("***2222"));
    assert_eq!(resolved.primary_account, Some(false));
    assert_eq!(resolved.account_type.as_deref(), Some("CASH"));
}

#[test]
fn account_resolve_no_match_returns_validation_error() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs = [make_pref(
        "A1",
        Some("Primary"),
        Some("***1111"),
        true,
        "MARGIN",
    )];

    let err = resolve_account_from_data(&hashes, &prefs, "Missing").unwrap_err();

    assert!(matches!(err, AppError::AccountValidation(_)));
    assert_eq!(err.to_string(), "no account found matching 'Missing'");
}

#[test]
fn account_resolve_ambiguous_nickname_returns_compact_validation_error() {
    let hashes = [make_hash("A1", "HASH1"), make_hash("A2", "HASH2")];
    let prefs = [
        make_pref("A1", Some("Trading"), Some("***1111"), true, "MARGIN"),
        make_pref("A2", Some("Trading"), Some("***2222"), false, "CASH"),
    ];

    let err = resolve_account_from_data(&hashes, &prefs, "Trading").unwrap_err();
    let message = err.to_string();

    assert!(matches!(err, AppError::AccountValidation(_)));
    assert!(message.contains("ambiguous account nickname 'Trading'"));
    assert!(message.contains("Trading (***1111)"));
    assert!(message.contains("Trading (***2222)"));
    assert!(!message.contains("A1"));
    assert!(!message.contains("A2"));
}

// ---------------------------------------------------------------------------
// resolve_default_account_hash_from_data tests
// ---------------------------------------------------------------------------

#[test]
fn default_account_returns_primary_when_designated() {
    let hashes = [make_hash("A1", "HASH1"), make_hash("A2", "HASH2")];
    let prefs = [
        make_pref("A1", Some("Cash"), Some("***1111"), false, "CASH"),
        make_pref("A2", Some("Margin"), Some("***2222"), true, "MARGIN"),
    ];

    let hash = resolve_default_account_hash_from_data(&hashes, &prefs).unwrap();

    // HASH2 is primary even though HASH1 appears first in the list.
    assert_eq!(hash, "HASH2");
}

#[test]
fn default_account_falls_back_to_first_when_no_primary() {
    let hashes = [make_hash("A1", "HASH1"), make_hash("A2", "HASH2")];
    let prefs = [
        make_pref("A1", Some("Cash"), Some("***1111"), false, "CASH"),
        make_pref("A2", Some("Margin"), Some("***2222"), false, "MARGIN"),
    ];

    let hash = resolve_default_account_hash_from_data(&hashes, &prefs).unwrap();

    assert_eq!(hash, "HASH1");
}

#[test]
fn default_account_returns_only_account_when_single() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs = [make_pref(
        "A1",
        Some("Solo"),
        Some("***1111"),
        false,
        "CASH",
    )];

    let hash = resolve_default_account_hash_from_data(&hashes, &prefs).unwrap();

    assert_eq!(hash, "HASH1");
}

#[test]
fn default_account_returns_error_when_no_accounts() {
    let hashes: Vec<schwab::AccountNumberHash> = vec![];
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];

    let err = resolve_default_account_hash_from_data(&hashes, &prefs).unwrap_err();

    assert!(matches!(err, AppError::AccountValidation(_)));
    assert!(err.to_string().contains("no accounts found"));
}

#[test]
fn default_account_skips_hashes_without_hash_value() {
    // A hash entry with no hash_value should be skipped; the second entry becomes first.
    let hashes = [
        schwab::AccountNumberHash {
            account_number: Some("A1".to_string()),
            hash_value: None,
        },
        make_hash("A2", "HASH2"),
    ];
    let prefs = [
        make_pref("A1", Some("Broken"), Some("***1111"), false, "CASH"),
        make_pref("A2", Some("Good"), Some("***2222"), false, "MARGIN"),
    ];

    let hash = resolve_default_account_hash_from_data(&hashes, &prefs).unwrap();

    assert_eq!(hash, "HASH2");
}

// ---------------------------------------------------------------------------
// Fixture helpers for render_summary_from_data tests
// ---------------------------------------------------------------------------

fn make_margin_account(
    account_number: &str,
    balances: Option<schwab::MarginBalance>,
    positions: Option<Vec<schwab::Position>>,
) -> schwab::Account {
    schwab::Account {
        securities_account: Some(schwab::SecuritiesAccount::Margin(schwab::MarginAccount {
            account_number: Some(account_number.to_string()),
            is_closing_only_restricted: None,
            is_day_trader: None,
            pfcb_flag: None,
            positions,
            round_trips: None,
            r#type: None,
            current_balances: balances,
            initial_balances: None,
            projected_balances: None,
        })),
    }
}

fn make_cash_account(
    account_number: &str,
    balances: Option<schwab::CashBalance>,
    positions: Option<Vec<schwab::Position>>,
) -> schwab::Account {
    schwab::Account {
        securities_account: Some(schwab::SecuritiesAccount::Cash(schwab::CashAccount {
            account_number: Some(account_number.to_string()),
            is_closing_only_restricted: None,
            is_day_trader: None,
            pfcb_flag: None,
            positions,
            round_trips: None,
            r#type: None,
            current_balances: balances,
            initial_balances: None,
            projected_balances: None,
        })),
    }
}

fn make_margin_balance() -> schwab::MarginBalance {
    schwab::MarginBalance {
        available_funds: Some(number(10_000.0)),
        available_funds_non_marginable_trade: Some(number(8_000.0)),
        buying_power: Some(number(20_000.0)),
        buying_power_non_marginable_trade: None,
        day_trading_buying_power: None,
        day_trading_buying_power_call: None,
        equity: Some(number(50_000.0)),
        equity_percentage: None,
        is_in_call: None,
        long_margin_value: None,
        maintenance_call: None,
        maintenance_requirement: None,
        margin_balance: None,
        option_buying_power: Some(number(15_000.0)),
        reg_t_call: None,
        short_balance: None,
        short_margin_value: None,
        sma: None,
        stock_buying_power: Some(number(18_000.0)),
    }
}

fn make_cash_balance() -> schwab::CashBalance {
    schwab::CashBalance {
        cash_available_for_trading: Some(number(5_000.0)),
        cash_available_for_withdrawal: Some(number(4_500.0)),
        cash_call: None,
        cash_debit_call_value: None,
        long_non_marginable_market_value: None,
        total_cash: Some(number(5_500.0)),
        unsettled_cash: None,
    }
}

fn position_with_instrument(instrument: AccountsInstrument) -> schwab::Position {
    schwab::Position {
        aged_quantity: None,
        average_long_price: None,
        average_price: None,
        average_short_price: None,
        current_day_cost: None,
        current_day_profit_loss: None,
        current_day_profit_loss_percentage: None,
        instrument: Some(instrument),
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

#[test]
fn preference_accounts_flattens_only_present_account_lists() {
    let accounts = preference_accounts(vec![
        schwab::UserPreference {
            accounts: Some(vec![make_pref(
                "A1",
                Some("Trading"),
                Some("***1111"),
                true,
                "MARGIN",
            )]),
            streamer_info: None,
            offers: None,
        },
        schwab::UserPreference {
            accounts: None,
            streamer_info: None,
            offers: None,
        },
        schwab::UserPreference {
            accounts: Some(vec![make_pref(
                "A2",
                Some("Cash"),
                Some("***2222"),
                false,
                "CASH",
            )]),
            streamer_info: None,
            offers: None,
        },
    ]);

    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0].account_number.as_deref(), Some("A1"));
    assert_eq!(accounts[1].account_number.as_deref(), Some("A2"));
}

#[test]
fn selected_account_validation_accepts_non_empty_summary() {
    let summary = AccountSummaryData {
        accounts: vec![AccountRow {
            account_hash: "HASH1".to_string(),
            nickname: None,
            display_account_id: None,
            primary_account: None,
            account_type: None,
            is_closing_only_restricted: None,
            is_day_trader: None,
            balances: None,
            positions: None,
        }],
    };

    ensure_selected_account_rendered(&summary, "HASH1").unwrap();
}

#[test]
fn instrument_summary_handles_all_account_instrument_variants() {
    let cases = [
        AccountsInstrument::Option(AccountOption {
            asset_type: Some(InstrumentAssetType::Option),
            cusip: Some("OPT-CUSIP".to_string()),
            description: Some("Option contract".to_string()),
            instrument_id: Some(1),
            net_change: None,
            option_deliverables: None,
            option_multiplier: None,
            put_call: None,
            r#type: None,
            symbol: Some("AAPL  260117C00150000".to_string()),
            underlying_symbol: Some("AAPL".to_string()),
        }),
        AccountsInstrument::FixedIncome(AccountFixedIncome {
            asset_type: Some(InstrumentAssetType::FixedIncome),
            cusip: Some("FI-CUSIP".to_string()),
            description: Some("Bond".to_string()),
            factor: None,
            instrument_id: Some(2),
            maturity_date: None,
            net_change: None,
            symbol: Some("BOND".to_string()),
            variable_rate: None,
        }),
        AccountsInstrument::CashEquivalent(AccountCashEquivalent {
            asset_type: Some(InstrumentAssetType::CashEquivalent),
            cusip: Some("CASH-CUSIP".to_string()),
            description: Some("Sweep".to_string()),
            instrument_id: Some(3),
            net_change: None,
            symbol: Some("SWEEP".to_string()),
            r#type: None,
        }),
        AccountsInstrument::Equity(AccountEquity {
            asset_type: Some(InstrumentAssetType::Equity),
            cusip: Some("EQ-CUSIP".to_string()),
            description: Some("Stock".to_string()),
            instrument_id: Some(4),
            net_change: None,
            symbol: Some("MSFT".to_string()),
        }),
        AccountsInstrument::MutualFund(AccountMutualFund {
            asset_type: Some(InstrumentAssetType::MutualFund),
            cusip: Some("MF-CUSIP".to_string()),
            description: Some("Fund".to_string()),
            instrument_id: Some(5),
            net_change: None,
            symbol: Some("SWPPX".to_string()),
        }),
    ];

    for instrument in cases {
        let summary = instrument_summary(&instrument);
        assert!(summary.symbol.is_some());
        assert!(summary.cusip.is_some());
        assert!(summary.instrument_id.is_some());
        assert!(summary.description.is_some());
        assert!(summary.asset_type.is_some());
    }
}

// ---------------------------------------------------------------------------
// render_summary_from_data tests
// ---------------------------------------------------------------------------

#[test]
fn account_summary_without_positions() {
    let hashes = [make_hash("A1", "HASH1"), make_hash("A2", "HASH2")];
    let prefs = [
        make_pref("A1", Some("Margin Acct"), Some("***1111"), true, "MARGIN"),
        make_pref("A2", Some("Cash Acct"), Some("***2222"), false, "CASH"),
    ];
    let accounts = vec![
        make_margin_account("A1", Some(make_margin_balance()), None),
        make_cash_account("A2", Some(make_cash_balance()), None),
    ];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);

    assert_eq!(summary.accounts.len(), 2);

    // Margin account row
    let margin_row = &summary.accounts[0];
    assert_eq!(margin_row.account_hash, "HASH1");
    assert_eq!(margin_row.nickname.as_deref(), Some("Margin Acct"));
    assert!(margin_row.positions.is_none());
    assert!(margin_row.balances.is_some());

    let serialized = serde_json::to_value(margin_row).unwrap();
    assert_eq!(serialized["balances"]["kind"], "margin");
    assert!(serialized.get("positions").is_none());

    // Cash account row
    let cash_row = &summary.accounts[1];
    assert_eq!(cash_row.account_hash, "HASH2");
    assert_eq!(cash_row.nickname.as_deref(), Some("Cash Acct"));
    assert!(cash_row.positions.is_none());

    let serialized = serde_json::to_value(cash_row).unwrap();
    assert_eq!(serialized["balances"]["kind"], "cash");
    assert!(serialized.get("positions").is_none());

    // Raw account numbers must not appear in serialized output
    let full_json = serde_json::to_string(&summary).unwrap();
    assert!(!full_json.contains("account_number"));
    assert!(!full_json.contains("\"A1\""));
    assert!(!full_json.contains("\"A2\""));
}

#[test]
fn account_summary_with_positions() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs = [make_pref(
        "A1",
        Some("Trading"),
        Some("***1111"),
        true,
        "MARGIN",
    )];
    let positions = vec![schwab::Position {
        aged_quantity: None,
        average_long_price: None,
        average_price: Some(number(150.0)),
        average_short_price: None,
        current_day_cost: None,
        current_day_profit_loss: None,
        current_day_profit_loss_percentage: None,
        instrument: Some(AccountsInstrument::Equity(AccountEquity {
            asset_type: Some(InstrumentAssetType::Equity),
            cusip: Some("037833100".to_string()),
            description: Some("Apple Inc".to_string()),
            instrument_id: Some(12345),
            net_change: None,
            symbol: Some("AAPL".to_string()),
        })),
        long_open_profit_loss: None,
        long_quantity: Some(number(10.0)),
        maintenance_requirement: None,
        market_value: Some(number(1_500.0)),
        previous_session_long_quantity: None,
        previous_session_short_quantity: None,
        settled_long_quantity: None,
        settled_short_quantity: None,
        short_open_profit_loss: None,
        short_quantity: None,
        tax_lot_average_long_price: None,
        tax_lot_average_short_price: None,
    }];
    let accounts = vec![make_margin_account(
        "A1",
        Some(make_margin_balance()),
        Some(positions),
    )];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, true);

    assert_eq!(summary.accounts.len(), 1);
    let row = &summary.accounts[0];
    assert!(row.positions.is_some());
    let pos = row.positions.as_ref().unwrap().as_array().unwrap();
    assert_eq!(pos.len(), 1);
    assert_eq!(pos[0]["symbol"], "AAPL");
    assert_eq!(pos[0]["cusip"], "037833100");
    assert_eq!(pos[0]["instrument_id"], 12345);
    assert_eq!(pos[0]["description"], "Apple Inc");
    assert_eq!(pos[0]["asset_type"], "Equity");
}

#[test]
fn account_summary_positions_none_when_not_requested() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs = [make_pref(
        "A1",
        Some("Trading"),
        Some("***1111"),
        true,
        "MARGIN",
    )];
    // Even if positions data exists on the account, with_positions=false should omit them.
    let positions = vec![schwab::Position {
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
    }];
    let accounts = vec![make_margin_account(
        "A1",
        Some(make_margin_balance()),
        Some(positions),
    )];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);

    assert_eq!(summary.accounts.len(), 1);
    assert!(summary.accounts[0].positions.is_none());
}

#[test]
fn account_summary_missing_nickname_falls_back_to_variant_type() {
    let hashes = [make_hash("A1", "HASH1")];
    // No preference data at all for this account.
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];
    let accounts = vec![make_cash_account("A1", Some(make_cash_balance()), None)];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);

    assert_eq!(summary.accounts.len(), 1);
    let row = &summary.accounts[0];
    assert_eq!(row.account_hash, "HASH1");
    // Falls back to variant type when no preference data exists.
    assert_eq!(row.nickname.as_deref(), Some("CASH"));
    assert!(row.display_account_id.is_none());
    assert!(row.primary_account.is_none());
    assert!(row.account_type.is_none());

    // Balances should still be present.
    assert!(row.balances.is_some());
    let serialized = serde_json::to_value(row).unwrap();
    assert_eq!(serialized["balances"]["kind"], "cash");
}

#[test]
fn account_summary_no_nick_name_falls_back_to_pref_type() {
    let hashes = [make_hash("A1", "HASH1")];
    // Preference exists but nick_name is None; should fall back to pref type.
    let prefs = [make_pref("A1", None, Some("***1111"), true, "MARGIN")];
    let accounts = vec![make_margin_account("A1", Some(make_margin_balance()), None)];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);

    assert_eq!(summary.accounts.len(), 1);
    let row = &summary.accounts[0];
    assert_eq!(row.nickname.as_deref(), Some("MARGIN"));
    assert_eq!(row.account_type.as_deref(), Some("MARGIN"));
}

#[test]
fn account_summary_skips_accounts_without_securities_account() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs = [make_pref(
        "A1",
        Some("Trading"),
        Some("***1111"),
        true,
        "MARGIN",
    )];
    let accounts = vec![schwab::Account {
        securities_account: None,
    }];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);

    assert!(summary.accounts.is_empty());
}

#[test]
fn account_summary_skips_accounts_without_matching_hash() {
    let hashes = [make_hash("OTHER", "HASH_OTHER")];
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];
    let accounts = vec![make_margin_account("A1", Some(make_margin_balance()), None)];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);

    // A1 has no matching hash, so it should be excluded.
    assert!(summary.accounts.is_empty());
}

#[test]
fn account_summary_margin_balance_fields() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];
    let accounts = vec![make_margin_account("A1", Some(make_margin_balance()), None)];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);
    let row = &summary.accounts[0];

    match row.balances.as_ref().unwrap() {
        AccountBalances::Margin(m) => {
            assert_eq!(m.cash_available_for_trading, Some(number(10_000.0)));
            assert_eq!(m.cash_available_for_withdrawal, Some(number(8_000.0)));
            assert_eq!(m.buying_power, Some(number(20_000.0)));
            assert_eq!(m.stock_buying_power, Some(number(18_000.0)));
            assert_eq!(m.option_buying_power, Some(number(15_000.0)));
            assert_eq!(m.equity, Some(number(50_000.0)));
        }
        AccountBalances::Cash(_) => panic!("expected margin balances"),
    }
}

#[test]
fn account_summary_cash_balance_fields() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];
    let accounts = vec![make_cash_account("A1", Some(make_cash_balance()), None)];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);
    let row = &summary.accounts[0];

    match row.balances.as_ref().unwrap() {
        AccountBalances::Cash(c) => {
            assert_eq!(c.cash_available_for_trading, Some(number(5_000.0)));
            assert_eq!(c.cash_available_for_withdrawal, Some(number(4_500.0)));
            assert_eq!(c.total_cash, Some(number(5_500.0)));
        }
        AccountBalances::Margin(_) => panic!("expected cash balances"),
    }
}

#[test]
fn account_summary_no_balances_when_absent() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];
    let accounts = vec![make_margin_account("A1", None, None)];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);

    assert_eq!(summary.accounts.len(), 1);
    assert!(summary.accounts[0].balances.is_none());
}

#[test]
fn account_summary_includes_account_flags() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];

    // Build a margin account with both flags set.
    let mut account = make_margin_account("A1", None, None);
    if let Some(schwab::SecuritiesAccount::Margin(ref mut m)) = account.securities_account {
        m.is_closing_only_restricted = Some(true);
        m.is_day_trader = Some(false);
    }

    let summary = render_summary_from_data(&[account], &hashes, &prefs, false);

    assert_eq!(summary.accounts.len(), 1);
    assert_eq!(summary.accounts[0].is_closing_only_restricted, Some(true));
    assert_eq!(summary.accounts[0].is_day_trader, Some(false));
}

#[test]
fn account_summary_cash_positions_are_compacted_when_requested() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];
    let positions = vec![position_with_instrument(AccountsInstrument::MutualFund(
        AccountMutualFund {
            asset_type: Some(InstrumentAssetType::MutualFund),
            cusip: Some("808509855".to_string()),
            description: Some("Index fund".to_string()),
            instrument_id: Some(42),
            net_change: None,
            symbol: Some("SWPPX".to_string()),
        },
    ))];
    let accounts = vec![make_cash_account(
        "A1",
        Some(make_cash_balance()),
        Some(positions),
    )];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, true);

    let positions = summary.accounts[0]
        .positions
        .as_ref()
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(positions[0]["symbol"], "SWPPX");
    assert_eq!(positions[0]["asset_type"], "MutualFund");
}

#[test]
fn account_summary_omits_absent_account_flags() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs: Vec<schwab::UserPreferenceAccount> = vec![];
    let accounts = vec![make_cash_account("A1", None, None)];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, false);

    assert_eq!(summary.accounts.len(), 1);
    assert!(summary.accounts[0].is_closing_only_restricted.is_none());
    assert!(summary.accounts[0].is_day_trader.is_none());
}

#[test]
fn retain_account_summary_keeps_only_selected_hash() {
    let mut summary = AccountSummaryData {
        accounts: vec![
            AccountRow {
                account_hash: "HASH1".to_string(),
                nickname: Some("Trading".to_string()),
                display_account_id: None,
                primary_account: None,
                account_type: None,
                is_closing_only_restricted: None,
                is_day_trader: None,
                balances: None,
                positions: None,
            },
            AccountRow {
                account_hash: "HASH2".to_string(),
                nickname: Some("Savings".to_string()),
                display_account_id: None,
                primary_account: None,
                account_type: None,
                is_closing_only_restricted: None,
                is_day_trader: None,
                balances: None,
                positions: None,
            },
        ],
    };

    retain_account_summary(&mut summary, "HASH2");

    assert_eq!(summary.accounts.len(), 1);
    assert_eq!(summary.accounts[0].account_hash, "HASH2");
}

#[test]
fn selected_account_validation_fails_when_rendering_drops_account() {
    let summary = AccountSummaryData { accounts: vec![] };

    let err = ensure_selected_account_rendered(&summary, "HASH1").unwrap_err();

    match err {
        AppError::AccountValidation(message) => {
            assert_eq!(
                message,
                "account 'HASH1' resolved but no account summary data was available"
            );
        }
        other => panic!("expected account validation error, got {other:?}"),
    }
}

#[test]
fn positions_return_compact_objects_by_default() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs = [make_pref(
        "A1",
        Some("Trading"),
        Some("***1111"),
        true,
        "MARGIN",
    )];
    let positions = vec![schwab::Position {
        aged_quantity: None,
        average_long_price: None,
        average_price: Some(number(150.0)),
        average_short_price: None,
        current_day_cost: None,
        current_day_profit_loss: Some(number(25.0)),
        current_day_profit_loss_percentage: Some(number(1.5)),
        instrument: Some(AccountsInstrument::Equity(AccountEquity {
            asset_type: Some(InstrumentAssetType::Equity),
            cusip: None,
            description: Some("Apple Inc".to_string()),
            instrument_id: None,
            net_change: None,
            symbol: Some("AAPL".to_string()),
        })),
        long_open_profit_loss: None,
        long_quantity: Some(number(10.0)),
        maintenance_requirement: None,
        market_value: Some(number(1_500.0)),
        previous_session_long_quantity: None,
        previous_session_short_quantity: None,
        settled_long_quantity: None,
        settled_short_quantity: None,
        short_open_profit_loss: None,
        short_quantity: Some(number(2.0)),
        tax_lot_average_long_price: None,
        tax_lot_average_short_price: None,
    }];
    let accounts = vec![make_margin_account(
        "A1",
        Some(make_margin_balance()),
        Some(positions),
    )];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, true);

    let row = &summary.accounts[0];
    let pos = row.positions.as_ref().unwrap().as_array().unwrap();
    assert_eq!(pos.len(), 1);
    assert_eq!(pos[0]["symbol"], "AAPL");
    assert_eq!(pos[0]["description"], "Apple Inc");
    assert_eq!(pos[0]["asset_type"], "Equity");
    assert_eq!(pos[0]["long_quantity"], serde_json::json!(number(10.0)));
    assert_eq!(pos[0]["short_quantity"], serde_json::json!(number(2.0)));
    assert_eq!(pos[0]["average_price"], serde_json::json!(number(150.0)));
    assert_eq!(pos[0]["market_value"], serde_json::json!(number(1_500.0)));
    assert_eq!(
        pos[0]["current_day_profit_loss"],
        serde_json::json!(number(25.0))
    );
    assert_eq!(
        pos[0]["current_day_profit_loss_percentage"],
        serde_json::json!(number(1.5))
    );
}

#[test]
fn positions_include_fallback_instrument_identifiers() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs = [make_pref(
        "A1",
        Some("Trading"),
        Some("***1111"),
        true,
        "MARGIN",
    )];
    let positions = vec![schwab::Position {
        aged_quantity: None,
        average_long_price: None,
        average_price: None,
        average_short_price: None,
        current_day_cost: None,
        current_day_profit_loss: None,
        current_day_profit_loss_percentage: None,
        instrument: Some(AccountsInstrument::Equity(AccountEquity {
            asset_type: None,
            cusip: Some("9128285M8".to_string()),
            description: Some("Treasury holding".to_string()),
            instrument_id: Some(98765),
            net_change: None,
            symbol: None,
        })),
        long_open_profit_loss: None,
        long_quantity: Some(number(5.0)),
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
    }];
    let accounts = vec![make_margin_account(
        "A1",
        Some(make_margin_balance()),
        Some(positions),
    )];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, true);

    let row = &summary.accounts[0];
    let pos = row.positions.as_ref().unwrap().as_array().unwrap();
    assert_eq!(pos.len(), 1);
    assert!(pos[0].get("symbol").is_none());
    assert!(pos[0].get("asset_type").is_none());
    assert_eq!(pos[0]["cusip"], "9128285M8");
    assert_eq!(pos[0]["instrument_id"], 98765);
    assert_eq!(pos[0]["long_quantity"], serde_json::json!(number(5.0)));
}

#[test]
fn positions_omit_missing_fields() {
    let hashes = [make_hash("A1", "HASH1")];
    let prefs = [make_pref(
        "A1",
        Some("Trading"),
        Some("***1111"),
        true,
        "MARGIN",
    )];
    let positions = vec![schwab::Position {
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
    }];
    let accounts = vec![make_margin_account(
        "A1",
        Some(make_margin_balance()),
        Some(positions),
    )];

    let summary = render_summary_from_data(&accounts, &hashes, &prefs, true);

    let row = &summary.accounts[0];
    let pos = row.positions.as_ref().unwrap().as_array().unwrap();
    assert_eq!(pos.len(), 1);
    assert!(pos[0].as_object().unwrap().is_empty());
}
