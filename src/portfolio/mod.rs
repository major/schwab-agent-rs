use schwab::{
    Account, AccountsInstrument, CashAccount, CashBalance, MarginAccount, MarginBalance, Position,
    SecuritiesAccount, UserPreferenceAccount,
};
use serde::Serialize;
use serde_json::{Value, to_value};

use crate::auth;
use crate::cli::{Cli, PortfolioCommand, PortfolioSnapshotArgs};
use crate::error::AppError;

/// Dispatches a portfolio subcommand and returns its JSON value.
pub(crate) async fn handle(cli: &Cli, command: &PortfolioCommand) -> Result<Value, AppError> {
    match command {
        PortfolioCommand::Snapshot(args) => snapshot(cli, args).await,
    }
}

/// Fetches all accounts from the Schwab API and returns a serialized [`PortfolioSnapshot`].
///
/// Uses `raw::fetch_accounts` to normalize Schwab API quirks (object-wrapped
/// arrays, boolean `false` in numeric fields) before deserialization. Also
/// fetches user preferences so that each account includes a human-readable
/// nickname. Passes the `positions` field query parameter only when
/// `args.positions` is true.
async fn snapshot(cli: &Cli, args: &PortfolioSnapshotArgs) -> Result<Value, AppError> {
    let provider = auth::provider(cli)?;
    let token = provider.token().await?;
    let client = provider.client().await?;

    let prefs: Vec<UserPreferenceAccount> = client
        .get_user_preference()
        .await?
        .into_iter()
        .filter_map(|p| p.accounts)
        .flatten()
        .collect();

    let fields = args.positions.then_some("positions");
    let accounts = crate::raw::fetch_accounts(&token, fields)
        .await?
        .into_iter()
        .map(|account| summarize_account(account, args.positions, &prefs))
        .collect::<Vec<_>>();
    Ok(to_value(PortfolioSnapshot { accounts })?)
}

/// Converts a raw [`Account`] into a normalized [`AccountSummary`].
///
/// Dispatches to the margin or cash variant based on the account type.
/// Looks up a human-readable nickname from user preferences, falling back
/// to the preference account type or the securities account variant name.
/// Returns an empty summary when `securities_account` is `None`.
fn summarize_account(
    account: Account,
    include_positions: bool,
    prefs: &[UserPreferenceAccount],
) -> AccountSummary {
    match account.securities_account {
        Some(SecuritiesAccount::Margin(margin_account)) => {
            let nickname =
                resolve_nickname(margin_account.account_number.as_deref(), prefs, "MARGIN");
            summarize_margin_account(margin_account, include_positions, nickname)
        }
        Some(SecuritiesAccount::Cash(cash_account)) => {
            let nickname = resolve_nickname(cash_account.account_number.as_deref(), prefs, "CASH");
            summarize_cash_account(cash_account, include_positions, nickname)
        }
        None => AccountSummary::default(),
    }
}

/// Resolves a human-readable nickname for an account.
///
/// Checks user preferences for a `nick_name` first, then falls back to the
/// preference `type` field, and finally to the securities account variant name
/// (e.g. `"MARGIN"` or `"CASH"`).
#[must_use]
fn resolve_nickname(
    account_number: Option<&str>,
    prefs: &[UserPreferenceAccount],
    variant_type: &str,
) -> String {
    let pref = account_number.and_then(|num| {
        prefs
            .iter()
            .find(|p| p.account_number.as_deref() == Some(num))
    });

    pref.and_then(|p| p.nick_name.clone())
        .filter(|n| !n.is_empty())
        .or_else(|| pref.and_then(|p| p.r#type.clone()))
        .unwrap_or_else(|| variant_type.to_string())
}

/// Builds an [`AccountSummary`] from any account type that carries the standard fields.
///
/// Both [`MarginAccount`] and [`CashAccount`] share the same field names, so this macro
/// captures the shared construction logic and avoids duplicating the field mappings.
macro_rules! build_account_summary {
    ($account:expr, $account_type:expr, $include_positions:expr, $nickname:expr) => {
        AccountSummary {
            account_number: $account.account_number,
            nickname: Some($nickname),
            account_type: Some($account_type),
            is_closing_only_restricted: $account.is_closing_only_restricted,
            is_day_trader: $account.is_day_trader,
            balances: $account.current_balances.map(BalanceSummary::from),
            positions: $include_positions
                .then(|| summarize_positions($account.positions))
                .flatten(),
        }
    };
}

/// Converts a [`MarginAccount`] into an [`AccountSummary`] with `account_type` set to `"MARGIN"`.
fn summarize_margin_account(
    account: MarginAccount,
    include_positions: bool,
    nickname: String,
) -> AccountSummary {
    build_account_summary!(account, "MARGIN", include_positions, nickname)
}

/// Converts a [`CashAccount`] into an [`AccountSummary`] with `account_type` set to `"CASH"`.
fn summarize_cash_account(
    account: CashAccount,
    include_positions: bool,
    nickname: String,
) -> AccountSummary {
    build_account_summary!(account, "CASH", include_positions, nickname)
}

/// Maps an optional list of [`Position`]s into an optional list of [`PositionSummary`]s.
///
/// Returns `None` when `positions` is `None`, preserving the absence of data.
fn summarize_positions(positions: Option<Vec<Position>>) -> Option<Vec<PositionSummary>> {
    positions.map(|values| values.into_iter().map(PositionSummary::from).collect())
}

/// Top-level snapshot of all accounts returned by the `portfolio snapshot` command.
#[derive(Debug, Serialize)]
struct PortfolioSnapshot {
    /// All accounts associated with the authenticated user.
    accounts: Vec<AccountSummary>,
}

/// Normalized summary of a single brokerage account, covering both margin and cash variants.
#[serde_with::skip_serializing_none]
#[derive(Debug, Default, Serialize)]
struct AccountSummary {
    /// The masked account number as returned by the API.
    account_number: Option<String>,
    /// Human-readable nickname for the account, sourced from user preferences.
    ///
    /// Falls back to the preference account type or the securities account
    /// variant name (e.g. `"MARGIN"`, `"CASH"`) when no nickname is configured.
    nickname: Option<String>,
    /// Account type: `"MARGIN"` or `"CASH"`.
    account_type: Option<&'static str>,
    /// Whether the account is restricted to closing orders only.
    is_closing_only_restricted: Option<bool>,
    /// Whether the account is flagged as a pattern day trader account.
    is_day_trader: Option<bool>,
    /// Current balance figures for the account.
    balances: Option<BalanceSummary>,
    /// Open positions, present only when the snapshot was requested with `--positions`.
    positions: Option<Vec<PositionSummary>>,
}

/// Flattened balance figures shared across margin and cash account types.
///
/// Fields that don't apply to a given account type are omitted from JSON output.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
struct BalanceSummary {
    /// Cash available to place new trades (margin: `available_funds`; cash: direct field).
    cash_available_for_trading: Option<schwab::Number>,
    /// Cash available to withdraw (margin: `available_funds_non_marginable_trade`; cash: direct field).
    cash_available_for_withdrawal: Option<schwab::Number>,
    /// Total cash balance; populated for cash accounts only.
    total_cash: Option<schwab::Number>,
    /// Total buying power; populated for margin accounts only.
    buying_power: Option<schwab::Number>,
    /// Buying power available for stock purchases; populated for margin accounts only.
    stock_buying_power: Option<schwab::Number>,
    /// Buying power available for options trades; populated for margin accounts only.
    option_buying_power: Option<schwab::Number>,
    /// Account equity (market value minus margin debt); populated for margin accounts only.
    equity: Option<schwab::Number>,
}

impl From<MarginBalance> for BalanceSummary {
    /// Maps margin-specific fields to the shared summary shape; `total_cash` is always `None`.
    fn from(balance: MarginBalance) -> Self {
        Self {
            cash_available_for_trading: balance.available_funds,
            cash_available_for_withdrawal: balance.available_funds_non_marginable_trade,
            total_cash: None,
            buying_power: balance.buying_power,
            stock_buying_power: balance.stock_buying_power,
            option_buying_power: balance.option_buying_power,
            equity: balance.equity,
        }
    }
}

impl From<CashBalance> for BalanceSummary {
    /// Maps cash-specific fields to the shared summary shape; margin-only fields are always `None`.
    fn from(balance: CashBalance) -> Self {
        Self {
            cash_available_for_trading: balance.cash_available_for_trading,
            cash_available_for_withdrawal: balance.cash_available_for_withdrawal,
            total_cash: balance.total_cash,
            buying_power: None,
            stock_buying_power: None,
            option_buying_power: None,
            equity: None,
        }
    }
}

/// Flattened summary of a single open position, with instrument fields inlined.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
struct PositionSummary {
    /// Ticker symbol of the held instrument.
    symbol: Option<String>,
    /// Human-readable description of the instrument.
    description: Option<String>,
    /// Asset type string derived from the instrument variant (e.g. `"Equity"`, `"Option"`).
    asset_type: Option<String>,
    /// Number of shares or contracts held long.
    long_quantity: Option<schwab::Number>,
    /// Number of shares or contracts held short.
    short_quantity: Option<schwab::Number>,
    /// Average cost basis per share or contract.
    average_price: Option<schwab::Number>,
    /// Current market value of the entire position.
    market_value: Option<schwab::Number>,
    /// Unrealized profit or loss for the current trading day.
    current_day_profit_loss: Option<schwab::Number>,
    /// Unrealized profit or loss for the current day as a percentage.
    current_day_profit_loss_percentage: Option<schwab::Number>,
}

impl From<Position> for PositionSummary {
    /// Converts a raw [`Position`] into a [`PositionSummary`].
    ///
    /// Instrument fields (symbol, description, asset type) are extracted via [`InstrumentSummary`]
    /// and inlined directly onto the summary to keep the output flat.
    fn from(position: Position) -> Self {
        let instrument = position.instrument.map(InstrumentSummary::from);
        Self {
            symbol: instrument.as_ref().and_then(|value| value.symbol.clone()),
            description: instrument
                .as_ref()
                .and_then(|value| value.description.clone()),
            asset_type: instrument.and_then(|value| value.asset_type),
            long_quantity: position.long_quantity,
            short_quantity: position.short_quantity,
            average_price: position.average_price,
            market_value: position.market_value,
            current_day_profit_loss: position.current_day_profit_loss,
            current_day_profit_loss_percentage: position.current_day_profit_loss_percentage,
        }
    }
}

/// Intermediate representation that normalizes all [`AccountsInstrument`] variants
/// into a common set of fields used when building a [`PositionSummary`].
struct InstrumentSummary {
    /// Ticker symbol of the instrument.
    symbol: Option<String>,
    /// Human-readable description of the instrument.
    description: Option<String>,
    /// Asset type as a debug-formatted string (e.g. `"Equity"`, `"Option"`).
    asset_type: Option<String>,
}

impl From<AccountsInstrument> for InstrumentSummary {
    /// Converts any [`AccountsInstrument`] variant into an [`InstrumentSummary`].
    ///
    /// Each variant carries the same core fields (symbol, description, asset_type), so the
    /// extraction logic is shared via a local macro.
    fn from(instrument: AccountsInstrument) -> Self {
        macro_rules! extract {
            ($value:expr) => {
                Self {
                    symbol: $value.symbol,
                    description: $value.description,
                    asset_type: $value.asset_type.map(|at| format!("{at:?}")),
                }
            };
        }
        match instrument {
            AccountsInstrument::Option(v) => extract!(v),
            AccountsInstrument::FixedIncome(v) => extract!(v),
            AccountsInstrument::CashEquivalent(v) => extract!(v),
            AccountsInstrument::Equity(v) => extract!(v),
            AccountsInstrument::MutualFund(v) => extract!(v),
        }
    }
}

#[cfg(test)]
mod tests;
