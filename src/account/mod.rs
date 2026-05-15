use serde::Serialize;
use serde_json::{Value, to_value};

use schwab::{
    Account, AccountNumberHash, AccountsInstrument, CashBalance, MarginBalance, SecuritiesAccount,
    UserPreferenceAccount,
};

use crate::auth;
use crate::cli::{AccountCommand, Cli};
use crate::error::AppError;

/// Dispatches an account subcommand and returns its JSON value.
pub(crate) async fn handle(cli: &Cli, command: &AccountCommand) -> Result<Value, AppError> {
    let client = auth::provider(cli)?.client().await?;
    match command {
        AccountCommand::Summary(args) => {
            let data = run_summary(&client, args.positions).await?;
            Ok(to_value(data)?)
        }
        AccountCommand::Resolve(args) => {
            let data = resolve_account(&client, &args.selector).await?;
            Ok(to_value(data)?)
        }
    }
}

/// A normalized brokerage account row for summary output.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
pub struct AccountRow {
    pub account_hash: String,
    pub nickname: Option<String>,
    pub display_account_id: Option<String>,
    pub primary_account: Option<bool>,
    pub account_type: Option<String>,
    pub balances: Option<AccountBalances>,
    pub positions: Option<Vec<Value>>,
}

/// Account balance summary, tagged by account kind.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AccountBalances {
    Margin(MarginBalanceSummary),
    Cash(CashBalanceSummary),
}

/// Margin account balance summary.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
pub struct MarginBalanceSummary {
    pub cash_available_for_trading: Option<schwab::Number>,
    pub cash_available_for_withdrawal: Option<schwab::Number>,
    pub buying_power: Option<schwab::Number>,
    pub stock_buying_power: Option<schwab::Number>,
    pub option_buying_power: Option<schwab::Number>,
    pub equity: Option<schwab::Number>,
}

/// Cash account balance summary.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
pub struct CashBalanceSummary {
    pub cash_available_for_trading: Option<schwab::Number>,
    pub cash_available_for_withdrawal: Option<schwab::Number>,
    pub total_cash: Option<schwab::Number>,
}

/// Account summary payload.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
pub struct AccountSummaryData {
    pub accounts: Vec<AccountRow>,
}

#[derive(Debug)]
struct AccountFields {
    account_number: Option<String>,
    balances: Option<AccountBalances>,
    positions: Option<Vec<Value>>,
}

/// Account resolution payload.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
pub struct AccountResolveData {
    pub account_hash: String,
    pub matched_by: String,
    pub nickname: Option<String>,
    pub display_account_id: Option<String>,
    pub primary_account: Option<bool>,
    pub account_type: Option<String>,
}

/// Builds a compact [`AccountRow`] from a hash entry and optional user preference account.
///
/// The `account_hash` field comes from `AccountNumberHash.hash_value`.
/// Raw account numbers are never included in the output.
#[must_use]
pub fn build_account_row(hash_value: String, pref: Option<&UserPreferenceAccount>) -> AccountRow {
    AccountRow {
        account_hash: hash_value,
        nickname: pref.and_then(|p| p.nick_name.clone()),
        display_account_id: pref.and_then(|p| p.display_acct_id.clone()),
        primary_account: pref.and_then(|p| p.primary_account),
        account_type: pref.and_then(|p| p.r#type.clone()),
        balances: None,
        positions: None,
    }
}

/// Fetches accounts, account hashes, and user preferences, then renders compact
/// account rows with balance summaries.
///
/// When `with_positions` is true, account data is fetched with position details
/// included. Otherwise, positions are omitted from the output.
///
/// # Errors
///
/// Returns an `AppError` when any Schwab API call fails.
pub async fn run_summary(
    client: &schwab::Client,
    with_positions: bool,
) -> Result<AccountSummaryData, AppError> {
    let hashes = client.get_account_numbers().await?;
    let preferences = client.get_user_preference().await?;
    let prefs: Vec<UserPreferenceAccount> = preferences
        .into_iter()
        .filter_map(|preference| preference.accounts)
        .flatten()
        .collect();

    let fields = with_positions.then_some("positions");
    let accounts = client.get_accounts(fields).await?;

    Ok(render_summary_from_data(
        &accounts,
        &hashes,
        &prefs,
        with_positions,
    ))
}

/// Pure helper that builds an [`AccountSummaryData`] from pre-fetched API data.
///
/// Joins accounts to hashes via `account_number`, enriches with user preferences,
/// and extracts balance summaries based on account type (margin vs cash).
#[must_use]
pub(crate) fn render_summary_from_data(
    accounts: &[Account],
    hashes: &[AccountNumberHash],
    prefs: &[UserPreferenceAccount],
    with_positions: bool,
) -> AccountSummaryData {
    let rows = accounts
        .iter()
        .filter_map(|account| {
            let fields = extract_account_fields(account, with_positions)?;
            let AccountFields {
                account_number,
                balances,
                positions,
            } = fields;
            let hash_value = find_hash_value(account_number.as_deref(), hashes)?;
            let pref = matching_preference(account_number.as_deref(), prefs);
            let mut row = build_account_row(hash_value, pref);
            row.balances = balances;
            row.positions = positions;
            Some(row)
        })
        .collect();

    AccountSummaryData { accounts: rows }
}

/// Extracts the account number, balance summary, and optional positions from an
/// [`Account`] by dispatching on the securities account variant.
///
/// Returns `None` when the account has no `securities_account` field.
#[must_use]
fn extract_account_fields(account: &Account, with_positions: bool) -> Option<AccountFields> {
    match account.securities_account.as_ref()? {
        SecuritiesAccount::Margin(margin) => {
            let balances = margin
                .current_balances
                .as_ref()
                .map(|b| AccountBalances::Margin(margin_balance_summary(b)));
            let positions = with_positions
                .then(|| compact_positions(&margin.positions))
                .flatten();
            Some(AccountFields {
                account_number: margin.account_number.clone(),
                balances,
                positions,
            })
        }
        SecuritiesAccount::Cash(cash) => {
            let balances = cash
                .current_balances
                .as_ref()
                .map(|b| AccountBalances::Cash(cash_balance_summary(b)));
            let positions = with_positions
                .then(|| compact_positions(&cash.positions))
                .flatten();
            Some(AccountFields {
                account_number: cash.account_number.clone(),
                balances,
                positions,
            })
        }
    }
}

/// Maps a [`MarginBalance`] to a compact [`MarginBalanceSummary`].
#[must_use]
fn margin_balance_summary(balance: &MarginBalance) -> MarginBalanceSummary {
    MarginBalanceSummary {
        cash_available_for_trading: balance.available_funds,
        cash_available_for_withdrawal: balance.available_funds_non_marginable_trade,
        buying_power: balance.buying_power,
        stock_buying_power: balance.stock_buying_power,
        option_buying_power: balance.option_buying_power,
        equity: balance.equity,
    }
}

/// Maps a [`CashBalance`] to a compact [`CashBalanceSummary`].
#[must_use]
fn cash_balance_summary(balance: &CashBalance) -> CashBalanceSummary {
    CashBalanceSummary {
        cash_available_for_trading: balance.cash_available_for_trading,
        cash_available_for_withdrawal: balance.cash_available_for_withdrawal,
        total_cash: balance.total_cash,
    }
}

/// Converts positions into compact JSON values with only essential fields.
///
/// Returns `None` when positions are absent, preserving the distinction between
/// "not requested" and "empty list".
#[must_use]
fn compact_positions(positions: &Option<Vec<schwab::Position>>) -> Option<Vec<Value>> {
    positions
        .as_ref()
        .map(|pos| pos.iter().map(compact_position).collect())
}

/// Builds a compact JSON value from a single position, including only fields
/// useful for an account summary.
#[must_use]
fn compact_position(position: &schwab::Position) -> Value {
    let mut map = serde_json::Map::new();

    if let Some(instrument) = position.instrument.as_ref().map(instrument_summary) {
        if let Some(symbol) = instrument.symbol {
            map.insert("symbol".to_string(), serde_json::json!(symbol));
        }
        if let Some(description) = instrument.description {
            map.insert("description".to_string(), serde_json::json!(description));
        }
        if let Some(asset_type) = instrument.asset_type {
            map.insert("asset_type".to_string(), serde_json::json!(asset_type));
        }
    }

    if let Some(qty) = position.long_quantity {
        map.insert("long_quantity".to_string(), serde_json::json!(qty));
    }
    if let Some(qty) = position.short_quantity {
        map.insert("short_quantity".to_string(), serde_json::json!(qty));
    }
    if let Some(price) = position.average_price {
        map.insert("average_price".to_string(), serde_json::json!(price));
    }
    if let Some(value) = position.market_value {
        map.insert("market_value".to_string(), serde_json::json!(value));
    }
    if let Some(pnl) = position.current_day_profit_loss {
        map.insert(
            "current_day_profit_loss".to_string(),
            serde_json::json!(pnl),
        );
    }
    if let Some(pnl_pct) = position.current_day_profit_loss_percentage {
        map.insert(
            "current_day_profit_loss_percentage".to_string(),
            serde_json::json!(pnl_pct),
        );
    }

    Value::Object(map)
}

struct InstrumentSummary {
    symbol: Option<String>,
    description: Option<String>,
    asset_type: Option<String>,
}

/// Normalizes Schwab account instrument variants into identifier fields for a
/// compact position row.
#[must_use]
fn instrument_summary(instrument: &AccountsInstrument) -> InstrumentSummary {
    macro_rules! extract {
        ($value:expr) => {
            InstrumentSummary {
                symbol: $value.symbol.clone(),
                description: $value.description.clone(),
                asset_type: $value
                    .asset_type
                    .as_ref()
                    .map(|asset_type| format!("{asset_type:?}")),
            }
        };
    }

    match instrument {
        AccountsInstrument::Option(value) => extract!(value),
        AccountsInstrument::FixedIncome(value) => extract!(value),
        AccountsInstrument::CashEquivalent(value) => extract!(value),
        AccountsInstrument::Equity(value) => extract!(value),
        AccountsInstrument::MutualFund(value) => extract!(value),
    }
}

/// Finds the hash value for an account number from the account numbers list.
#[must_use]
fn find_hash_value(account_number: Option<&str>, hashes: &[AccountNumberHash]) -> Option<String> {
    let account_number = account_number?;
    hashes
        .iter()
        .find(|h| h.account_number.as_deref() == Some(account_number))
        .and_then(|h| h.hash_value.clone())
}

/// Resolves an account selector to the canonical Schwab account hash.
///
/// Exact hash matches take precedence over exact nickname matches. Raw account
/// numbers are used only as the join key between Schwab API responses and are
/// never returned in the result or validation messages.
///
/// # Errors
///
/// Returns an `AppError` when the selector does not match any account or when a
/// nickname selector matches more than one account. Schwab API failures also
/// return an `AppError`.
pub async fn resolve_account(
    client: &schwab::Client,
    selector: &str,
) -> Result<AccountResolveData, AppError> {
    let hashes = client.get_account_numbers().await?;
    let preferences = client.get_user_preference().await?;
    let prefs = preferences
        .into_iter()
        .filter_map(|preference| preference.accounts)
        .flatten()
        .collect::<Vec<_>>();

    resolve_account_from_data(&hashes, &prefs, selector)
}

/// Resolves a selector from pre-fetched account hash and preference data.
///
/// This pure helper keeps the matching rules unit-testable without requiring a
/// live Schwab client or credentials.
pub(crate) fn resolve_account_from_data(
    hashes: &[AccountNumberHash],
    prefs: &[UserPreferenceAccount],
    selector: &str,
) -> Result<AccountResolveData, AppError> {
    let rows = joined_account_rows(hashes, prefs);

    if let Some(row) = rows.iter().find(|row| row.account_hash == selector) {
        return Ok(account_resolve_data(row, "hash"));
    }

    let nickname_matches = rows
        .iter()
        .filter(|row| row.nickname.as_deref() == Some(selector))
        .collect::<Vec<_>>();

    match nickname_matches.as_slice() {
        [row] => Ok(account_resolve_data(row, "nickname")),
        [] => Err(AppError::AccountValidation(format!(
            "no account found matching '{selector}'"
        ))),
        matches => Err(AppError::AccountValidation(format!(
            "ambiguous account nickname '{selector}' matched: {}",
            matches
                .iter()
                .map(|row| compact_account_label(row))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Joins hash records to preference records on raw account number.
///
/// Raw account numbers are borrowed only for this comparison. Rows without a
/// hash value are skipped because they cannot be used as canonical selectors.
#[must_use]
fn joined_account_rows(
    hashes: &[AccountNumberHash],
    prefs: &[UserPreferenceAccount],
) -> Vec<AccountRow> {
    hashes
        .iter()
        .filter_map(|hash| {
            let hash_value = hash.hash_value.clone()?;
            let pref = matching_preference(hash.account_number.as_deref(), prefs);
            Some(build_account_row(hash_value, pref))
        })
        .collect()
}

/// Finds the preference account that shares the hash entry account number.
#[must_use]
fn matching_preference<'a>(
    account_number: Option<&str>,
    prefs: &'a [UserPreferenceAccount],
) -> Option<&'a UserPreferenceAccount> {
    let account_number = account_number?;
    prefs
        .iter()
        .find(|pref| pref.account_number.as_deref() == Some(account_number))
}

/// Converts a joined account row into resolver output with match metadata.
#[must_use]
fn account_resolve_data(row: &AccountRow, matched_by: &str) -> AccountResolveData {
    AccountResolveData {
        account_hash: row.account_hash.clone(),
        matched_by: matched_by.to_string(),
        nickname: row.nickname.clone(),
        display_account_id: row.display_account_id.clone(),
        primary_account: row.primary_account,
        account_type: row.account_type.clone(),
    }
}

/// Formats an ambiguous account match without exposing raw account numbers.
#[must_use]
fn compact_account_label(row: &AccountRow) -> String {
    let nickname = row.nickname.as_deref().unwrap_or("<no nickname>");
    let display_account_id = row
        .display_account_id
        .as_deref()
        .unwrap_or("<no display id>");
    format!("{nickname} ({display_account_id})")
}

#[cfg(test)]
mod tests;
