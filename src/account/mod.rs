use serde::Serialize;
use serde_json::{Value, json, to_value};

use schwab::{
    Account, AccountNumberHash, AccountsInstrument, CashBalance, MarginBalance, SecuritiesAccount,
    UserPreferenceAccount,
};

use crate::auth;
use crate::cli::{AccountArgs, Cli};
use crate::error::AppError;

/// Default position fields for row-based output.
const DEFAULT_POSITION_FIELDS: [&str; 6] = ["sym", "long_qty", "avg", "mktval", "pnl", "pnlpct"];

/// Dispatches the account command and returns its JSON value.
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) async fn handle(_cli: &Cli, args: &AccountArgs) -> Result<Value, AppError> {
    if let Some(selector) = &args.selector {
        let provider = auth::provider()?;
        let token = provider.token().await?;
        let data = resolve_account(&token, selector).await?;
        return Ok(to_value(data)?);
    }

    // Validate fields before fetching account data to fail fast on bad input.
    let position_fields = if args.all_fields {
        None
    } else {
        Some(selected_position_fields(args.fields.as_deref())?)
    };
    let provider = auth::provider()?;
    let token = provider.token().await?;
    let data = run_summary(
        &token,
        args.include_positions(),
        args.with_positions_only,
        position_fields.as_deref(),
    )
    .await?;
    Ok(to_value(data)?)
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
    pub is_closing_only_restricted: Option<bool>,
    pub is_day_trader: Option<bool>,
    pub balances: Option<AccountBalances>,
    pub positions: Option<Value>,
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
    variant_type: &'static str,
    is_closing_only_restricted: Option<bool>,
    is_day_trader: Option<bool>,
    balances: Option<AccountBalances>,
    positions: Option<Value>,
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
        nickname: pref
            .and_then(|p| p.nick_name.clone())
            .filter(|n| !n.is_empty()),
        display_account_id: pref.and_then(|p| p.display_acct_id.clone()),
        primary_account: pref.and_then(|p| p.primary_account),
        account_type: pref.and_then(|p| p.r#type.clone()),
        is_closing_only_restricted: None,
        is_day_trader: None,
        balances: None,
        positions: None,
    }
}

/// Fetches accounts, account hashes, and user preferences, then renders compact
/// account rows with balance summaries.
///
/// Uses a raw HTTP request to normalize Schwab API quirks (object-wrapped
/// arrays, boolean `false` in numeric fields) before deserialization.
///
/// When `with_positions` is true, account data is fetched with position details
/// included. Otherwise, positions are omitted from the output.
///
/// When `with_positions_only` is true, accounts that have no positions are
/// excluded from the result entirely. This implicitly enables position
/// retrieval regardless of the `with_positions` value.
///
/// # Errors
///
/// Returns an `AppError` when any Schwab API call fails.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn run_summary(
    bearer_token: &str,
    with_positions: bool,
    with_positions_only: bool,
    position_fields: Option<&[&str]>,
) -> Result<AccountSummaryData, AppError> {
    // Filtering by positions requires fetching them.
    let effective_positions = with_positions || with_positions_only;

    let hashes = crate::raw::fetch_account_numbers(bearer_token).await?;
    let preferences = crate::raw::fetch_user_preference(bearer_token).await?;
    let prefs: Vec<UserPreferenceAccount> = preferences
        .into_iter()
        .filter_map(|preference| preference.accounts)
        .flatten()
        .collect();

    let fields = effective_positions.then_some("positions");
    let accounts = crate::raw::fetch_accounts(bearer_token, fields).await?;

    Ok(render_summary_from_data(
        &accounts,
        &hashes,
        &prefs,
        effective_positions,
        with_positions_only,
        position_fields,
    ))
}

/// Pure helper that builds an [`AccountSummaryData`] from pre-fetched API data.
///
/// Joins accounts to hashes via `account_number`, enriches with user preferences,
/// and extracts balance summaries based on account type (margin vs cash).
///
/// When `with_positions_only` is true, accounts whose `positions` field is
/// `None`, an empty array, or a row-based object with zero rows are excluded
/// from the output.
///
/// When `position_fields` is `Some`, positions use row-based output with the
/// given field selection. When `None`, positions use full compact objects.
#[must_use]
pub(crate) fn render_summary_from_data(
    accounts: &[Account],
    hashes: &[AccountNumberHash],
    prefs: &[UserPreferenceAccount],
    with_positions: bool,
    with_positions_only: bool,
    position_fields: Option<&[&str]>,
) -> AccountSummaryData {
    let rows = accounts
        .iter()
        .filter_map(|account| {
            let fields = extract_account_fields(account, with_positions, position_fields)?;
            let AccountFields {
                account_number,
                variant_type,
                is_closing_only_restricted,
                is_day_trader,
                balances,
                positions,
            } = fields;
            if with_positions_only && !has_positions(&positions) {
                return None;
            }
            let hash_value = find_hash_value(account_number.as_deref(), hashes)?;
            let pref = matching_preference(account_number.as_deref(), prefs);
            let mut row = build_account_row(hash_value, pref);
            if row.nickname.is_none() {
                row.nickname = row
                    .account_type
                    .clone()
                    .or_else(|| Some(variant_type.to_string()));
            }
            row.is_closing_only_restricted = is_closing_only_restricted;
            row.is_day_trader = is_day_trader;
            row.balances = balances;
            row.positions = positions;
            Some(row)
        })
        .collect();

    AccountSummaryData { accounts: rows }
}

/// Returns `true` when positions contains at least one entry.
///
/// Handles both row-based (`{rows: [...]}`) and array-based (`[...]`) formats.
#[must_use]
fn has_positions(positions: &Option<Value>) -> bool {
    positions.as_ref().is_some_and(|v| match v {
        Value::Array(arr) => !arr.is_empty(),
        Value::Object(obj) => obj
            .get("rows")
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty()),
        _ => false,
    })
}

/// Extracts the account number, balance summary, and optional positions from an
/// [`Account`] by dispatching on the securities account variant.
///
/// When `position_fields` is `Some`, positions are formatted as row-based output
/// using [`select_position_fields`]. When `None`, positions use full compact
/// objects via [`compact_position`].
///
/// Returns `None` when the account has no `securities_account` field.
#[must_use]
fn extract_account_fields(
    account: &Account,
    with_positions: bool,
    position_fields: Option<&[&str]>,
) -> Option<AccountFields> {
    match account.securities_account.as_ref()? {
        SecuritiesAccount::Margin(margin) => {
            let balances = margin
                .current_balances
                .as_ref()
                .map(|b| AccountBalances::Margin(margin_balance_summary(b)));
            let positions = with_positions
                .then(|| format_positions(&margin.positions, position_fields))
                .flatten();
            Some(AccountFields {
                account_number: margin.account_number.clone(),
                variant_type: "MARGIN",
                is_closing_only_restricted: margin.is_closing_only_restricted,
                is_day_trader: margin.is_day_trader,
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
                .then(|| format_positions(&cash.positions, position_fields))
                .flatten();
            Some(AccountFields {
                account_number: cash.account_number.clone(),
                variant_type: "CASH",
                is_closing_only_restricted: cash.is_closing_only_restricted,
                is_day_trader: cash.is_day_trader,
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

/// Formats positions using either row-based or compact object output.
///
/// When `position_fields` is `Some`, returns row-based output with `columns`,
/// `rows`, and `rowCount`. When `None`, returns an array of compact objects.
///
/// Returns `None` when positions are absent, preserving the distinction between
/// "not requested" and "empty list".
#[must_use]
fn format_positions(
    positions: &Option<Vec<schwab::Position>>,
    position_fields: Option<&[&str]>,
) -> Option<Value> {
    let pos = positions.as_ref()?;
    match position_fields {
        Some(fields) => Some(select_position_fields(pos, fields)),
        None => Some(Value::Array(pos.iter().map(compact_position).collect())),
    }
}

/// Builds row-based position output with `columns`, `rows`, and `rowCount`.
#[must_use]
fn select_position_fields(positions: &[schwab::Position], fields: &[&str]) -> Value {
    let columns: Vec<Value> = fields.iter().map(|f| json!(*f)).collect();
    let rows: Vec<Value> = positions
        .iter()
        .map(|pos| {
            let instrument = pos.instrument.as_ref().map(instrument_summary);
            let row: Vec<Value> = fields
                .iter()
                .map(|f| position_field_value(pos, instrument.as_ref(), f))
                .collect();
            Value::Array(row)
        })
        .collect();
    json!({
        "columns": columns,
        "rows": rows,
        "rowCount": rows.len(),
    })
}

/// Extracts a single field value from a position by canonical field name.
///
/// The caller precomputes `instrument` once per position so it is reused
/// across all fields in the row.
#[must_use]
fn position_field_value(
    position: &schwab::Position,
    instrument: Option<&InstrumentSummary>,
    field: &str,
) -> Value {
    match field {
        "sym" => instrument
            .and_then(|i| i.symbol.as_deref())
            .map_or(Value::Null, |s| json!(s)),
        "desc" => instrument
            .and_then(|i| i.description.as_deref())
            .map_or(Value::Null, |s| json!(s)),
        "type" => instrument
            .and_then(|i| i.asset_type.as_deref())
            .map_or(Value::Null, |s| json!(s)),
        "long_qty" => position.long_quantity.map_or(Value::Null, |v| json!(v)),
        "short_qty" => position.short_quantity.map_or(Value::Null, |v| json!(v)),
        "avg" => position.average_price.map_or(Value::Null, |v| json!(v)),
        "mktval" => position.market_value.map_or(Value::Null, |v| json!(v)),
        "pnl" => position
            .current_day_profit_loss
            .map_or(Value::Null, |v| json!(v)),
        "pnlpct" => position
            .current_day_profit_loss_percentage
            .map_or(Value::Null, |v| json!(v)),
        _ => Value::Null,
    }
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

/// Parses and validates a comma-separated field list for position output.
///
/// Returns canonical field names. When `requested` is `None`, returns the
/// default field set.
///
/// # Errors
///
/// Returns `AppError::AccountValidation` when any field name is unrecognized
/// or the list is empty.
pub(crate) fn selected_position_fields(
    requested: Option<&str>,
) -> Result<Vec<&'static str>, AppError> {
    let Some(input) = requested else {
        return Ok(DEFAULT_POSITION_FIELDS.to_vec());
    };

    let fields: Vec<&str> = input.split(',').map(str::trim).collect();
    if fields.is_empty() || fields.iter().all(|f| f.is_empty()) {
        return Err(AppError::AccountValidation(
            "empty field list; omit --fields to use defaults".to_string(),
        ));
    }

    validate_position_fields(&fields)?;

    Ok(fields
        .into_iter()
        .filter(|f| !f.is_empty())
        .filter_map(|f| canonical_position_field(f))
        .collect())
}

/// Validates that all requested fields are recognized position field names.
fn validate_position_fields(fields: &[&str]) -> Result<(), AppError> {
    let unknown: Vec<&str> = fields
        .iter()
        .filter(|f| !f.is_empty())
        .filter(|f| canonical_position_field(f).is_none())
        .copied()
        .collect();

    if unknown.is_empty() {
        Ok(())
    } else {
        let available = available_position_fields();
        Err(AppError::AccountValidation(format!(
            "unknown position field(s): {}; available: {}",
            unknown.join(", "),
            available.join(", ")
        )))
    }
}

/// Maps a field name or alias to its canonical short name.
#[must_use]
pub(crate) fn canonical_position_field(name: &str) -> Option<&'static str> {
    match name {
        "sym" | "symbol" => Some("sym"),
        "desc" | "description" => Some("desc"),
        "type" | "asset_type" => Some("type"),
        "long_qty" | "long_quantity" => Some("long_qty"),
        "short_qty" | "short_quantity" => Some("short_qty"),
        "avg" | "average_price" => Some("avg"),
        "mktval" | "market_value" => Some("mktval"),
        "pnl" | "current_day_profit_loss" => Some("pnl"),
        "pnlpct" | "current_day_profit_loss_percentage" => Some("pnlpct"),
        _ => None,
    }
}

/// Returns a sorted list of all accepted position field aliases.
#[must_use]
pub(crate) fn available_position_fields() -> Vec<&'static str> {
    let mut fields = vec![
        "sym",
        "symbol",
        "desc",
        "description",
        "type",
        "asset_type",
        "long_qty",
        "long_quantity",
        "short_qty",
        "short_quantity",
        "avg",
        "average_price",
        "mktval",
        "market_value",
        "pnl",
        "current_day_profit_loss",
        "pnlpct",
        "current_day_profit_loss_percentage",
    ];
    fields.sort_unstable();
    fields
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

/// Resolves the default account hash from pre-fetched data.
///
/// Pure helper: prefers the account marked as `primary_account == true`,
/// falls back to the first account in the hash list.
///
/// # Errors
///
/// Returns `AppError::AccountValidation` when no accounts are available.
#[cfg(test)]
pub(crate) fn resolve_default_account_hash_from_data(
    hashes: &[AccountNumberHash],
    prefs: &[UserPreferenceAccount],
) -> Result<String, AppError> {
    let rows = joined_account_rows(hashes, prefs);

    // Primary account wins; first account is the fallback.
    if let Some(row) = rows.iter().find(|r| r.primary_account == Some(true)) {
        return Ok(row.account_hash.clone());
    }

    rows.into_iter()
        .next()
        .map(|r| r.account_hash)
        .ok_or_else(|| AppError::AccountValidation("no accounts found".to_string()))
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
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn resolve_account(
    bearer_token: &str,
    selector: &str,
) -> Result<AccountResolveData, AppError> {
    let hashes = crate::raw::fetch_account_numbers(bearer_token).await?;
    let preferences = crate::raw::fetch_user_preference(bearer_token).await?;
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
