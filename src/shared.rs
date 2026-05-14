//! Shared types used across order and equity command modules.

use clap::ValueEnum;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Session / Duration choice enums (CLI-facing)
// ---------------------------------------------------------------------------

/// Trading session for the order.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SessionChoice {
    /// Regular market hours.
    Normal,
    /// Pre-market session.
    Am,
    /// After-hours session.
    Pm,
    /// Extended hours (pre-market through after-hours).
    Seamless,
}

impl From<SessionChoice> for schwab::Session {
    fn from(choice: SessionChoice) -> Self {
        match choice {
            SessionChoice::Normal => schwab::Session::Normal,
            SessionChoice::Am => schwab::Session::Am,
            SessionChoice::Pm => schwab::Session::Pm,
            SessionChoice::Seamless => schwab::Session::Seamless,
        }
    }
}

/// Time-in-force for the order.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DurationChoice {
    /// Good for the current trading day only.
    Day,
    /// Good until cancelled (typically 60-180 days depending on broker).
    #[value(alias = "gtc")]
    GoodTillCancel,
    /// Fill the entire order immediately or cancel it.
    #[value(alias = "fok")]
    FillOrKill,
    /// Fill as much as possible immediately, cancel the rest.
    #[value(alias = "ioc")]
    ImmediateOrCancel,
}

impl From<DurationChoice> for schwab::Duration {
    fn from(choice: DurationChoice) -> Self {
        match choice {
            DurationChoice::Day => schwab::Duration::Day,
            DurationChoice::GoodTillCancel => schwab::Duration::GoodTillCancel,
            DurationChoice::FillOrKill => schwab::Duration::FillOrKill,
            DurationChoice::ImmediateOrCancel => schwab::Duration::ImmediateOrCancel,
        }
    }
}

// ---------------------------------------------------------------------------
// Number conversion helper
// ---------------------------------------------------------------------------

/// Converts an `f64` CLI argument to [`schwab::Number`].
///
/// Without the `decimal` feature this is a no-op cast. With `decimal` enabled
/// the value is converted via `Decimal::try_from`, which rejects infinities and
/// NaN.
#[cfg(not(feature = "decimal"))]
pub fn to_number(v: f64) -> Result<schwab::Number, AppError> {
    Ok(v)
}

/// Converts an `f64` CLI argument to [`schwab::Number`] (decimal variant).
///
/// Converts via string formatting to avoid a direct `rust_decimal` dependency.
/// This mirrors the `serde-with-float` round-trip path that the API uses.
#[cfg(feature = "decimal")]
pub fn to_number(v: f64) -> Result<schwab::Number, AppError> {
    use core::str::FromStr;
    let s = format!("{v}");
    schwab::Number::from_str(&s)
        .map_err(|_| AppError::OrderValidation(format!("cannot convert {v} to decimal")))
}
