use schwab::{PriceHistoryOptions, QuoteOptions, QuoteResponseObject};
use serde::Serialize;
use serde_json::{Value, to_value};

use crate::auth;
use crate::cli::{Cli, HistoryArgs, MarketCommand, QuoteArgs};
use crate::error::AppError;

/// Routes market subcommands to their handlers and returns a JSON value.
pub(crate) async fn handle(cli: &Cli, command: &MarketCommand) -> Result<Value, AppError> {
    match command {
        MarketCommand::History(args) => history(cli, args).await,
        MarketCommand::Quote(args) => quote(cli, args).await,
    }
}

/// Fetches price history candles for a single symbol and returns them as JSON.
async fn history(cli: &Cli, args: &HistoryArgs) -> Result<Value, AppError> {
    let client = auth::provider(cli)?.client().await?;
    let mut options = PriceHistoryOptions::new();
    if let Some(period_type) = &args.period_type {
        options = options.parameter("periodType", period_type);
    }
    if let Some(period) = args.period {
        options = options.integer_parameter("period", period);
    }
    if let Some(frequency_type) = &args.frequency_type {
        options = options.parameter("frequencyType", frequency_type);
    }
    if let Some(frequency) = args.frequency {
        options = options.integer_parameter("frequency", frequency);
    }
    if let Some(from) = args.from {
        options = options.integer_parameter("startDate", from);
    }
    if let Some(to) = args.to {
        options = options.integer_parameter("endDate", to);
    }
    if args.extended_hours {
        options = options.bool_parameter("needExtendedHoursData", true);
    }
    let candle_list = client.get_price_history(&args.symbol, options).await?;
    Ok(to_value(candle_list)?)
}

/// Fetches quotes for the requested symbols from the Schwab API and returns a sorted,
/// flattened JSON array of [`QuoteSummary`] values wrapped in a [`QuoteOutput`].
async fn quote(cli: &Cli, args: &QuoteArgs) -> Result<Value, AppError> {
    let client = auth::provider(cli)?.client().await?;
    let quotes = if let Some(fields) = &args.fields {
        client
            .get_quotes_with_options(&args.symbols, QuoteOptions::new().fields(fields))
            .await?
    } else {
        client.get_quotes(&args.symbols).await?
    };
    let mut summaries = quotes
        .into_iter()
        .map(|(requested_symbol, quote)| summarize_quote(requested_symbol, quote))
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.requested_symbol.cmp(&right.requested_symbol));
    Ok(to_value(QuoteOutput {
        symbols: args.symbols.clone(),
        quotes: summaries,
    })?)
}

/// Extracts a field from an `Option<T>` by reference, returning `None` if the
/// outer option is `None`. Use `clone` for non-`Copy` fields like `String`.
macro_rules! opt_field {
    ($opt:expr, $field:ident) => {
        $opt.as_ref().and_then(|v| v.$field)
    };
    ($opt:expr, clone $field:ident) => {
        $opt.as_ref().and_then(|v| v.$field.clone())
    };
}

/// Normalizes all eight [`QuoteResponseObject`] variants (Equity, Option, MutualFund,
/// Forex, Future, FutureOption, Index, Error) into a single flat [`QuoteSummary`].
/// Fields that don't apply to a given asset type are left at their `Default` value (`None`).
fn summarize_quote(requested_symbol: String, quote: QuoteResponseObject) -> QuoteSummary {
    match quote {
        QuoteResponseObject::Equity(response) => {
            let quote = response.quote;
            let reference = response.reference;
            QuoteSummary {
                requested_symbol,
                symbol: response.symbol,
                asset_type: response.asset_main_type.map(|v| format!("{v:?}")),
                description: opt_field!(reference, clone description),
                exchange: opt_field!(reference, clone exchange_name),
                bid: opt_field!(quote, bid_price),
                ask: opt_field!(quote, ask_price),
                last: opt_field!(quote, last_price),
                mark: opt_field!(quote, mark),
                net_change: opt_field!(quote, net_change),
                net_percent_change: opt_field!(quote, net_percent_change),
                volume: opt_field!(quote, total_volume),
                quote_time: opt_field!(quote, quote_time),
                trade_time: opt_field!(quote, trade_time),
                security_status: opt_field!(quote, clone security_status),
                realtime: response.realtime,
                ..QuoteSummary::default()
            }
        }
        QuoteResponseObject::Option(response) => {
            let quote = response.quote;
            let reference = response.reference;
            QuoteSummary {
                requested_symbol,
                symbol: response.symbol,
                asset_type: response.asset_main_type.map(|v| format!("{v:?}")),
                description: opt_field!(reference, clone description),
                exchange: opt_field!(reference, clone exchange_name),
                bid: opt_field!(quote, bid_price),
                ask: opt_field!(quote, ask_price),
                last: opt_field!(quote, last_price),
                mark: opt_field!(quote, mark),
                net_change: opt_field!(quote, net_change),
                net_percent_change: opt_field!(quote, net_percent_change),
                volume: opt_field!(quote, total_volume),
                quote_time: opt_field!(quote, quote_time),
                trade_time: opt_field!(quote, trade_time),
                security_status: opt_field!(quote, clone security_status),
                realtime: response.realtime,
                underlying: opt_field!(reference, clone underlying),
                put_call: reference
                    .as_ref()
                    .and_then(|v| v.contract_type.as_ref())
                    .map(|v| format!("{v:?}")),
                strike_price: opt_field!(reference, strike_price),
                days_to_expiration: opt_field!(reference, days_to_expiration),
                ..QuoteSummary::default()
            }
        }
        QuoteResponseObject::MutualFund(response) => {
            let quote = response.quote;
            let reference = response.reference;
            QuoteSummary {
                requested_symbol,
                symbol: response.symbol,
                asset_type: response.asset_main_type.map(|v| format!("{v:?}")),
                description: opt_field!(reference, clone description),
                exchange: opt_field!(reference, clone exchange_name),
                last: opt_field!(quote, nav),
                mark: opt_field!(quote, nav),
                net_change: opt_field!(quote, net_change),
                net_percent_change: opt_field!(quote, net_percent_change),
                volume: opt_field!(quote, total_volume),
                trade_time: opt_field!(quote, trade_time),
                security_status: opt_field!(quote, clone security_status),
                realtime: response.realtime,
                ..QuoteSummary::default()
            }
        }
        QuoteResponseObject::Forex(response) => {
            let quote = response.quote;
            QuoteSummary {
                requested_symbol,
                symbol: response.symbol,
                asset_type: response.asset_main_type.map(|v| format!("{v:?}")),
                description: response.reference.and_then(|v| v.description),
                bid: opt_field!(quote, bid_price),
                ask: opt_field!(quote, ask_price),
                last: opt_field!(quote, last_price),
                mark: opt_field!(quote, mark),
                net_change: opt_field!(quote, net_change),
                net_percent_change: opt_field!(quote, net_percent_change),
                volume: opt_field!(quote, total_volume),
                quote_time: opt_field!(quote, quote_time),
                trade_time: opt_field!(quote, trade_time),
                security_status: opt_field!(quote, clone security_status),
                realtime: response.realtime,
                ..QuoteSummary::default()
            }
        }
        QuoteResponseObject::Future(response) => {
            let quote = response.quote;
            QuoteSummary {
                requested_symbol,
                symbol: response.symbol,
                asset_type: response.asset_main_type.map(|v| format!("{v:?}")),
                description: response.reference.and_then(|v| v.description),
                bid: opt_field!(quote, bid_price),
                ask: opt_field!(quote, ask_price),
                last: opt_field!(quote, last_price),
                mark: opt_field!(quote, mark),
                net_change: opt_field!(quote, net_change),
                net_percent_change: opt_field!(quote, future_percent_change),
                volume: opt_field!(quote, total_volume),
                quote_time: opt_field!(quote, quote_time),
                trade_time: opt_field!(quote, trade_time),
                security_status: opt_field!(quote, clone security_status),
                realtime: response.realtime,
                ..QuoteSummary::default()
            }
        }
        QuoteResponseObject::FutureOption(response) => {
            let quote = response.quote;
            QuoteSummary {
                requested_symbol,
                symbol: response.symbol,
                asset_type: response.asset_main_type.map(|v| format!("{v:?}")),
                description: response.reference.and_then(|v| v.description),
                bid: opt_field!(quote, bid_price),
                ask: opt_field!(quote, ask_price),
                last: opt_field!(quote, last_price),
                mark: opt_field!(quote, mark),
                net_change: opt_field!(quote, net_change),
                net_percent_change: opt_field!(quote, net_percent_change),
                volume: opt_field!(quote, total_volume),
                quote_time: opt_field!(quote, quote_time),
                trade_time: opt_field!(quote, trade_time),
                security_status: opt_field!(quote, clone security_status),
                realtime: response.realtime,
                ..QuoteSummary::default()
            }
        }
        QuoteResponseObject::Index(response) => {
            let quote = response.quote;
            QuoteSummary {
                requested_symbol,
                symbol: response.symbol,
                asset_type: response.asset_main_type.map(|v| format!("{v:?}")),
                description: response.reference.and_then(|v| v.description),
                last: opt_field!(quote, last_price),
                mark: opt_field!(quote, last_price),
                net_change: opt_field!(quote, net_change),
                net_percent_change: opt_field!(quote, net_percent_change),
                volume: opt_field!(quote, total_volume),
                trade_time: opt_field!(quote, trade_time),
                security_status: opt_field!(quote, clone security_status),
                realtime: response.realtime,
                ..QuoteSummary::default()
            }
        }
        QuoteResponseObject::Error(error) => QuoteSummary {
            requested_symbol,
            error: Some(QuoteErrorSummary {
                invalid_symbols: error.invalid_symbols,
                invalid_cusips: error.invalid_cusips,
                invalid_ssids: error.invalid_ssids,
            }),
            ..QuoteSummary::default()
        },
    }
}

/// Top-level JSON envelope returned by the `market quote` command.
#[derive(Debug, Serialize)]
struct QuoteOutput {
    /// The symbols that were requested, in the order the user supplied them.
    symbols: Vec<String>,
    /// Normalized quote data for each symbol, sorted alphabetically by `requested_symbol`.
    quotes: Vec<QuoteSummary>,
}

/// Flattened, agent-friendly view of any Schwab quote type.
///
/// All eight `QuoteResponseObject` variants collapse into this single struct.
/// Fields that don't apply to a given asset type are omitted from JSON output.
#[serde_with::skip_serializing_none]
#[derive(Debug, Default, Serialize)]
struct QuoteSummary {
    /// The symbol string the caller originally requested.
    requested_symbol: String,
    /// The canonical symbol returned by the API, which may differ from the requested symbol.
    symbol: Option<String>,
    /// Asset class as a debug-formatted string (e.g. `"Equity"`, `"Option"`, `"Future"`).
    asset_type: Option<String>,
    /// Human-readable name or description of the instrument.
    description: Option<String>,
    /// Exchange name (e.g. `"NASDAQ"`, `"CBOE"`). Not set for Forex, Future, FutureOption, or Index.
    exchange: Option<String>,
    /// Current best bid price. `None` for MutualFund and Index.
    bid: Option<schwab::Number>,
    /// Current best ask price. `None` for MutualFund and Index.
    ask: Option<schwab::Number>,
    /// Last traded price. For MutualFund this is the NAV.
    last: Option<schwab::Number>,
    /// Mark price. For Index this equals `last`; for MutualFund this equals NAV.
    mark: Option<schwab::Number>,
    /// Dollar change from the previous close.
    net_change: Option<schwab::Number>,
    /// Percent change from the previous close. For Future, sourced from `future_percent_change`.
    net_percent_change: Option<schwab::Number>,
    /// Total trading volume for the session.
    volume: Option<i64>,
    /// Timestamp of the most recent quote, in milliseconds since epoch. `None` for MutualFund and Index.
    quote_time: Option<i64>,
    /// Timestamp of the most recent trade, in milliseconds since epoch.
    trade_time: Option<i64>,
    /// Market session status string (e.g. `"Normal"`, `"Unknown"`).
    security_status: Option<String>,
    /// Whether the quote is real-time (`true`) or delayed/end-of-day (`false`).
    realtime: Option<bool>,
    /// Underlying symbol for options (e.g. `"AAPL"` for an AAPL option). Option variant only.
    underlying: Option<String>,
    /// Contract type as a debug-formatted string (`"Call"` or `"Put"`). Option variant only.
    put_call: Option<String>,
    /// Strike price of the option contract. Option variant only.
    strike_price: Option<schwab::Number>,
    /// Calendar days remaining until option expiration. Option variant only.
    days_to_expiration: Option<i32>,
    /// Populated when the API returns an error for this symbol instead of a valid quote.
    error: Option<QuoteErrorSummary>,
}

/// Error detail returned by the API when one or more requested symbols are unrecognized.
#[serde_with::skip_serializing_none]
#[derive(Debug, Serialize)]
struct QuoteErrorSummary {
    /// Ticker symbols the API did not recognize.
    invalid_symbols: Option<Vec<String>>,
    /// CUSIP identifiers the API did not recognize.
    invalid_cusips: Option<Vec<String>>,
    /// SSID identifiers the API did not recognize.
    invalid_ssids: Option<Vec<i64>>,
}

#[cfg(test)]
mod tests;
