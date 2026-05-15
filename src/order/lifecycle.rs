//! Order lifecycle commands: list, get, cancel.
//!
//! These commands let agents inspect and manage existing orders rather than
//! only creating new ones. The cancel command includes post-action
//! verification (follow-up GET) so the agent gets immediate confirmation
//! instead of an empty 200 response.

use clap::Args;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{Date, Month, OffsetDateTime, Time};

use crate::auth;
use crate::cli::Cli;
use crate::error::AppError;

use crate::verify;

// ---------------------------------------------------------------------------
// CLI argument structs
// ---------------------------------------------------------------------------

/// List orders for an account, or all linked accounts if `--account` is
/// omitted.
///
/// By default, lists orders entered in the last 60 days. Use `--recent` for
/// the last 24 hours, or `--from`/`--to` for a custom range. Pass `--status`
/// to filter by a specific order status (e.g., WORKING, FILLED, CANCELED).
#[derive(Debug, Args)]
pub struct OrderListArgs {
    /// Account hash. If omitted, returns orders for all linked accounts.
    #[arg(long)]
    pub account: Option<String>,

    /// Filter by order status (e.g. WORKING, FILLED, CANCELED).
    #[arg(long)]
    pub status: Option<String>,

    /// Start of time range (YYYY-MM-DD or RFC3339, e.g. 2025-01-15).
    /// Defaults to 60 days ago.
    #[arg(long)]
    pub from: Option<String>,

    /// End of time range (YYYY-MM-DD or RFC3339, e.g. 2025-06-15).
    /// Defaults to now.
    #[arg(long)]
    pub to: Option<String>,

    /// List orders from the last 24 hours. Overrides `--from`.
    #[arg(long)]
    pub recent: bool,

    /// Maximum number of orders to return.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_results: Option<u32>,
}

/// Retrieve a single order by ID.
#[derive(Debug, Args)]
pub struct OrderGetArgs {
    /// Account hash (required).
    #[arg(long)]
    pub account: String,

    /// Order ID to retrieve.
    #[arg(value_parser = clap::value_parser!(i64).range(1..))]
    pub order_id: i64,
}

/// Cancel an order by ID, with post-cancel verification.
///
/// After cancellation, a follow-up GET retrieves the order so the agent
/// can confirm the order reached CANCELED.
#[derive(Debug, Args)]
pub struct OrderCancelArgs {
    /// Account hash (required).
    #[arg(long)]
    pub account: String,

    /// Order ID to cancel.
    #[arg(value_parser = clap::value_parser!(i64).range(1..))]
    pub order_id: i64,
}

/// Which side of a date range is being normalized.
#[derive(Clone, Copy, Debug)]
enum RangeBoundary {
    /// Inclusive start of a calendar day.
    Start,
    /// Inclusive end of a calendar day.
    End,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Lists orders, optionally filtered by account, status, and time range.
pub(crate) async fn handle_list(cli: &Cli, args: &OrderListArgs) -> Result<Value, AppError> {
    let client = auth::provider(cli)?.client().await?;

    let (from_time, to_time) = normalize_list_range(args, OffsetDateTime::now_utc())?;

    let mut options = schwab::OrderListOptions::new(&from_time, &to_time);
    if let Some(n) = args.max_results {
        options = options.max_results(i64::from(n));
    }
    if let Some(status) = &args.status {
        options = options.status(status);
    }

    let orders: Vec<schwab::Order> = if let Some(account) = &args.account {
        client.get_orders(account, options).await?
    } else {
        client.get_all_orders(options).await?
    };

    let count = orders.len();
    let data: Value = serde_json::to_value(&orders)?;

    Ok(serde_json::json!({
        "orders": data,
        "count": count,
    }))
}

/// Retrieves a single order by account and order ID.
pub(crate) async fn handle_get(cli: &Cli, args: &OrderGetArgs) -> Result<Value, AppError> {
    let client = auth::provider(cli)?.client().await?;
    let order = client.get_order(&args.account, args.order_id).await?;
    Ok(serde_json::to_value(&order)?)
}

/// Cancels an order and verifies the cancellation via a follow-up GET.
pub(crate) async fn handle_cancel(cli: &Cli, args: &OrderCancelArgs) -> Result<Value, AppError> {
    crate::config::require_mutable_enabled()?;
    let client = auth::provider(cli)?.client().await?;
    client.cancel_order(&args.account, args.order_id).await?;

    let result = verify::verify_order(
        &client,
        &args.account,
        Some(args.order_id),
        "cancel",
        None,
        None,
    )
    .await;

    verify::action_value(result)
}

/// Normalizes list date arguments to RFC3339 instants.
fn normalize_list_range(
    args: &OrderListArgs,
    now: OffsetDateTime,
) -> Result<(String, String), AppError> {
    let to_time = match &args.to {
        Some(value) => parse_range_instant(value, RangeBoundary::End)?,
        None => now,
    };

    let from_time = if args.recent {
        now - time::Duration::hours(24)
    } else {
        match &args.from {
            Some(value) => parse_range_instant(value, RangeBoundary::Start)?,
            None => now - time::Duration::days(60),
        }
    };

    if from_time > to_time {
        return Err(AppError::OrderValidation(
            "order list --from must be before or equal to --to".to_string(),
        ));
    }

    Ok((format_rfc3339(from_time), format_rfc3339(to_time)))
}

/// Parses either a date-only value or a full RFC3339 instant.
fn parse_range_instant(value: &str, boundary: RangeBoundary) -> Result<OffsetDateTime, AppError> {
    if is_date_only(value) {
        return parse_date_only(value).and_then(|date| date_boundary(date, boundary));
    }

    OffsetDateTime::parse(value, &Rfc3339).map_err(|e| {
        AppError::OrderValidation(format!(
            "invalid order list date/time '{value}': expected YYYY-MM-DD or RFC3339 ({e})"
        ))
    })
}

/// Returns true when a value matches the supported YYYY-MM-DD shape.
fn is_date_only(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..].iter().all(u8::is_ascii_digit)
}

/// Parses a YYYY-MM-DD date without enabling free-form local timezone inference.
fn parse_date_only(value: &str) -> Result<Date, AppError> {
    let year = value[0..4]
        .parse::<i32>()
        .map_err(|e| invalid_date(value, e))?;
    let month_number = value[5..7]
        .parse::<u8>()
        .map_err(|e| invalid_date(value, e))?;
    let day = value[8..10]
        .parse::<u8>()
        .map_err(|e| invalid_date(value, e))?;
    let month = Month::try_from(month_number).map_err(|e| invalid_date(value, e))?;

    Date::from_calendar_date(year, month, day).map_err(|e| invalid_date(value, e))
}

/// Converts a calendar date to the requested inclusive UTC boundary.
fn date_boundary(date: Date, boundary: RangeBoundary) -> Result<OffsetDateTime, AppError> {
    let time = match boundary {
        RangeBoundary::Start => Time::MIDNIGHT,
        RangeBoundary::End => Time::from_hms_nano(23, 59, 59, 999_999_999).map_err(|e| {
            AppError::OrderValidation(format!("failed to build end-of-day timestamp: {e}"))
        })?,
    };

    Ok(date.with_time(time).assume_utc())
}

/// Formats an instant as RFC3339, which the Schwab API expects.
fn format_rfc3339(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).expect("RFC3339 format")
}

/// Builds a consistent validation error for invalid YYYY-MM-DD dates.
fn invalid_date<E: std::fmt::Display>(value: &str, error: E) -> AppError {
    AppError::OrderValidation(format!("invalid order list date '{value}': {error}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use clap::Parser;
    use time::{Duration, OffsetDateTime};

    use super::{OrderListArgs, normalize_list_range};
    use crate::cli::{Cli, Command};
    use crate::order::OrderCommand;

    #[test]
    fn parse_order_list_no_args() {
        let cli = Cli::parse_from(["schwab-agent", "order", "list"]);
        let Command::Order(OrderCommand::List(args)) = cli.command else {
            panic!("expected order list command");
        };
        assert!(args.account.is_none());
        assert!(args.status.is_none());
        assert!(args.from.is_none());
        assert!(args.to.is_none());
        assert!(!args.recent);
        assert!(args.max_results.is_none());
    }

    #[test]
    fn parse_order_list_with_account_and_status() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "list",
            "--account",
            "HASH123",
            "--status",
            "WORKING",
        ]);
        let Command::Order(OrderCommand::List(args)) = cli.command else {
            panic!("expected order list command");
        };
        assert_eq!(args.account.as_deref(), Some("HASH123"));
        assert_eq!(args.status.as_deref(), Some("WORKING"));
    }

    #[test]
    fn parse_order_list_recent() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "list",
            "--account",
            "HASH123",
            "--recent",
        ]);
        let Command::Order(OrderCommand::List(args)) = cli.command else {
            panic!("expected order list command");
        };
        assert_eq!(args.account.as_deref(), Some("HASH123"));
        assert!(args.recent);
    }

    #[test]
    fn parse_order_list_with_time_range() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "list",
            "--from",
            "2025-01-01",
            "--to",
            "2025-06-01T12:00:00Z",
            "--max-results",
            "50",
        ]);
        let Command::Order(OrderCommand::List(args)) = cli.command else {
            panic!("expected order list command");
        };
        assert_eq!(args.from.as_deref(), Some("2025-01-01"));
        assert_eq!(args.to.as_deref(), Some("2025-06-01T12:00:00Z"));
        assert_eq!(args.max_results, Some(50));
    }

    #[test]
    fn parse_order_get() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "get",
            "--account",
            "HASH123",
            "12345",
        ]);
        let Command::Order(OrderCommand::Get(args)) = cli.command else {
            panic!("expected order get command");
        };
        assert_eq!(args.account, "HASH123");
        assert_eq!(args.order_id, 12345);
    }

    #[test]
    fn parse_order_cancel() {
        let cli = Cli::parse_from([
            "schwab-agent",
            "order",
            "cancel",
            "--account",
            "HASH123",
            "67890",
        ]);
        let Command::Order(OrderCommand::Cancel(args)) = cli.command else {
            panic!("expected order cancel command");
        };
        assert_eq!(args.account, "HASH123");
        assert_eq!(args.order_id, 67890);
    }

    #[test]
    fn parse_order_list_rejects_non_positive_max_results() {
        assert!(
            Cli::try_parse_from(["schwab-agent", "order", "list", "--max-results", "0"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["schwab-agent", "order", "list", "--max-results", "-1"]).is_err()
        );
    }

    #[test]
    fn parse_order_get_and_cancel_reject_non_positive_order_ids() {
        assert!(
            Cli::try_parse_from(["schwab-agent", "order", "get", "--account", "HASH123", "0"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "schwab-agent",
                "order",
                "cancel",
                "--account",
                "HASH123",
                "-1"
            ])
            .is_err()
        );
    }

    #[test]
    fn normalize_list_range_expands_date_only_boundaries() {
        let now = OffsetDateTime::parse(
            "2026-06-15T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let args = OrderListArgs {
            account: None,
            status: None,
            from: Some("2026-05-28".to_string()),
            to: Some("2026-05-31".to_string()),
            recent: false,
            max_results: None,
        };

        let (from, to) = normalize_list_range(&args, now).unwrap();

        assert_eq!(from, "2026-05-28T00:00:00Z");
        assert_eq!(to, "2026-05-31T23:59:59.999999999Z");
    }

    #[test]
    fn normalize_list_range_allows_mixed_date_and_rfc3339() {
        let now = OffsetDateTime::parse(
            "2026-06-15T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let args = OrderListArgs {
            account: None,
            status: None,
            from: Some("2026-05-28".to_string()),
            to: Some("2026-05-31T12:30:00Z".to_string()),
            recent: false,
            max_results: None,
        };

        let (from, to) = normalize_list_range(&args, now).unwrap();

        assert_eq!(from, "2026-05-28T00:00:00Z");
        assert_eq!(to, "2026-05-31T12:30:00Z");
    }

    #[test]
    fn normalize_list_range_recent_overrides_from() {
        let now = OffsetDateTime::parse(
            "2026-06-15T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let args = OrderListArgs {
            account: None,
            status: None,
            from: Some("2026-05-28".to_string()),
            to: None,
            recent: true,
            max_results: None,
        };

        let (from, to) = normalize_list_range(&args, now).unwrap();

        assert_eq!(
            from,
            (now - Duration::hours(24))
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap()
        );
        assert_eq!(
            to,
            now.format(&time::format_description::well_known::Rfc3339)
                .unwrap()
        );
    }

    #[test]
    fn normalize_list_range_rejects_reversed_ranges() {
        let now = OffsetDateTime::parse(
            "2026-06-15T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let args = OrderListArgs {
            account: None,
            status: None,
            from: Some("2026-06-01".to_string()),
            to: Some("2026-05-31".to_string()),
            recent: false,
            max_results: None,
        };

        let error = normalize_list_range(&args, now).unwrap_err();
        assert!(error.to_string().contains("--from must be before"));
    }
}
