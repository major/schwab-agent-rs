//! Interval mapping: human-friendly intervals to Schwab API parameters.

use core::{fmt, str::FromStr};

use crate::error::AppError;

const MINUTES_PER_1_MIN_INTERVAL: u32 = 1;
const MINUTES_PER_5_MIN_INTERVAL: u32 = 5;
const MINUTES_PER_15_MIN_INTERVAL: u32 = 15;
const MINUTES_PER_30_MIN_INTERVAL: u32 = 30;

const HISTORY_PERIOD_TYPE_DAY: &str = "day";
const HISTORY_PERIOD_TYPE_YEAR: &str = "year";

const HISTORY_FREQUENCY_TYPE_DAILY: &str = "daily";
const HISTORY_FREQUENCY_TYPE_MINUTE: &str = "minute";
const HISTORY_FREQUENCY_TYPE_WEEKLY: &str = "weekly";

const WEEKS_PER_YEAR: usize = 52;
const DAILY_LOOKBACK_SAFETY_CANDLES: usize = 10;
const REGULAR_SESSION_MINUTES_PER_DAY: usize = 390;
const MAX_YEAR_PERIOD: usize = 20;
const MAX_DAY_PERIOD: usize = 10;

/// Number of regular trading days used for annualized TA lookbacks.
pub(crate) const TRADING_DAYS_PER_YEAR: usize = 252;

/// Human-friendly candle interval choices supported by TA commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interval {
    /// Daily candles.
    Daily,
    /// Weekly candles.
    Weekly,
    /// One-minute candles.
    OneMinute,
    /// Five-minute candles.
    FiveMinute,
    /// Fifteen-minute candles.
    FifteenMinute,
    /// Thirty-minute candles.
    ThirtyMinute,
}

impl fmt::Display for Interval {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::OneMinute => "1min",
            Self::FiveMinute => "5min",
            Self::FifteenMinute => "15min",
            Self::ThirtyMinute => "30min",
        })
    }
}

impl FromStr for Interval {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            "1min" => Ok(Self::OneMinute),
            "5min" => Ok(Self::FiveMinute),
            "15min" => Ok(Self::FifteenMinute),
            "30min" => Ok(Self::ThirtyMinute),
            _ => Err(AppError::TaInvalidInterval {
                interval: value.to_string(),
            }),
        }
    }
}

/// Schwab price history parameters derived from a TA interval and candle count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryParams {
    /// Schwab period type, such as `day` or `year`.
    pub period_type: String,
    /// Schwab period value.
    pub period: u32,
    /// Schwab frequency type, such as `minute`, `daily`, or `weekly`.
    pub frequency_type: String,
    /// Schwab frequency value.
    pub frequency: u32,
}

/// Returns the ceiling of integer division for non-negative values.
#[must_use]
pub(crate) const fn ceil_div(a: usize, b: usize) -> usize {
    a.div_ceil(b)
}

/// Returns the first valid period greater than or equal to `n`.
#[must_use]
pub(crate) fn next_valid_period(n: usize, valid: &[usize]) -> usize {
    for &period in valid {
        if period >= n {
            return period;
        }
    }

    match valid.last() {
        Some(&period) => period,
        None => n,
    }
}

/// Returns the maximum number of candles supported for an interval by Schwab limits.
#[must_use]
pub(crate) const fn max_candles_for_interval(interval: Interval) -> usize {
    match interval {
        Interval::Daily => MAX_YEAR_PERIOD * TRADING_DAYS_PER_YEAR,
        Interval::Weekly => MAX_YEAR_PERIOD * WEEKS_PER_YEAR,
        Interval::OneMinute => MAX_DAY_PERIOD * REGULAR_SESSION_MINUTES_PER_DAY,
        Interval::FiveMinute => {
            MAX_DAY_PERIOD * (REGULAR_SESSION_MINUTES_PER_DAY / MINUTES_PER_5_MIN_INTERVAL as usize)
        }
        Interval::FifteenMinute => {
            MAX_DAY_PERIOD
                * (REGULAR_SESSION_MINUTES_PER_DAY / MINUTES_PER_15_MIN_INTERVAL as usize)
        }
        Interval::ThirtyMinute => {
            MAX_DAY_PERIOD
                * (REGULAR_SESSION_MINUTES_PER_DAY / MINUTES_PER_30_MIN_INTERVAL as usize)
        }
    }
}

/// Converts a TA interval and required candle count into Schwab history parameters.
pub fn interval_to_history_params(
    interval: Interval,
    required_candles: usize,
) -> Result<HistoryParams, AppError> {
    let max_candles = max_candles_for_interval(interval);
    if max_candles > 0 && required_candles > max_candles {
        return Err(AppError::TaInsufficientData {
            needed: required_candles,
            got: max_candles,
            indicator: interval.to_string(),
        });
    }

    Ok(match interval {
        Interval::Daily => {
            let years = ceil_div(
                required_candles + DAILY_LOOKBACK_SAFETY_CANDLES,
                TRADING_DAYS_PER_YEAR,
            )
            .max(1);
            history_params(
                HISTORY_PERIOD_TYPE_YEAR,
                next_valid_period(years, valid_year_periods()) as u32,
                HISTORY_FREQUENCY_TYPE_DAILY,
                1,
            )
        }
        Interval::Weekly => {
            let years = ceil_div(required_candles, WEEKS_PER_YEAR).max(1);
            history_params(
                HISTORY_PERIOD_TYPE_YEAR,
                next_valid_period(years, valid_year_periods()) as u32,
                HISTORY_FREQUENCY_TYPE_WEEKLY,
                1,
            )
        }
        Interval::OneMinute => intraday_history_params(
            required_candles,
            MINUTES_PER_1_MIN_INTERVAL,
            REGULAR_SESSION_MINUTES_PER_DAY,
        ),
        Interval::FiveMinute => intraday_history_params(
            required_candles,
            MINUTES_PER_5_MIN_INTERVAL,
            REGULAR_SESSION_MINUTES_PER_DAY / MINUTES_PER_5_MIN_INTERVAL as usize,
        ),
        Interval::FifteenMinute => intraday_history_params(
            required_candles,
            MINUTES_PER_15_MIN_INTERVAL,
            REGULAR_SESSION_MINUTES_PER_DAY / MINUTES_PER_15_MIN_INTERVAL as usize,
        ),
        Interval::ThirtyMinute => intraday_history_params(
            required_candles,
            MINUTES_PER_30_MIN_INTERVAL,
            REGULAR_SESSION_MINUTES_PER_DAY / MINUTES_PER_30_MIN_INTERVAL as usize,
        ),
    })
}

/// Returns Schwab's supported year periods.
#[must_use]
const fn valid_year_periods() -> &'static [usize] {
    &[1, 2, 3, 5, 10, 15, 20]
}

/// Returns Schwab's supported day periods.
#[must_use]
const fn valid_day_periods() -> &'static [usize] {
    &[1, 2, 3, 4, 5, 10]
}

/// Builds history parameters for intraday minute intervals.
#[must_use]
fn intraday_history_params(
    required_candles: usize,
    frequency: u32,
    candles_per_day: usize,
) -> HistoryParams {
    let days = ceil_div(required_candles, candles_per_day).max(1);
    history_params(
        HISTORY_PERIOD_TYPE_DAY,
        next_valid_period(days, valid_day_periods()) as u32,
        HISTORY_FREQUENCY_TYPE_MINUTE,
        frequency,
    )
}

/// Builds a history parameter struct.
#[must_use]
fn history_params(
    period_type: &str,
    period: u32,
    frequency_type: &str,
    frequency: u32,
) -> HistoryParams {
    HistoryParams {
        period_type: period_type.to_string(),
        period,
        frequency_type: frequency_type.to_string(),
        frequency,
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use crate::error::AppError;

    use super::*;

    fn assert_params(
        params: HistoryParams,
        period_type: &str,
        period: u32,
        frequency_type: &str,
        frequency: u32,
    ) {
        assert_eq!(params.period_type, period_type);
        assert_eq!(params.period, period);
        assert_eq!(params.frequency_type, frequency_type);
        assert_eq!(params.frequency, frequency);
    }

    #[test]
    fn from_str_and_display_round_trip_all_supported_intervals() {
        let cases = [
            ("daily", Interval::Daily),
            ("weekly", Interval::Weekly),
            ("1min", Interval::OneMinute),
            ("5min", Interval::FiveMinute),
            ("15min", Interval::FifteenMinute),
            ("30min", Interval::ThirtyMinute),
        ];

        for (input, expected) in cases {
            let interval = Interval::from_str(input).expect("supported interval parses");
            assert_eq!(interval, expected);
            assert_eq!(interval.to_string(), input);
        }
    }

    #[test]
    fn from_str_rejects_unsupported_interval() {
        let error = Interval::from_str("2min").expect_err("unsupported interval fails");

        assert!(matches!(
            error,
            AppError::TaInvalidInterval { interval } if interval == "2min"
        ));
    }

    #[test]
    fn ceil_div_rounds_up_and_handles_exact_division() {
        assert_eq!(ceil_div(0, 390), 0);
        assert_eq!(ceil_div(1, 390), 1);
        assert_eq!(ceil_div(390, 390), 1);
        assert_eq!(ceil_div(391, 390), 2);
    }

    #[test]
    fn next_valid_period_rounds_to_allowed_periods() {
        assert_eq!(next_valid_period(1, &[1, 2, 3, 5, 10, 15, 20]), 1);
        assert_eq!(next_valid_period(4, &[1, 2, 3, 5, 10, 15, 20]), 5);
        assert_eq!(next_valid_period(6, &[1, 2, 3, 4, 5, 10]), 10);
        assert_eq!(next_valid_period(99, &[1, 2, 3, 5, 10, 15, 20]), 20);
    }

    #[test]
    fn max_candles_matches_schwab_period_limits() {
        assert_eq!(max_candles_for_interval(Interval::Daily), 20 * 252);
        assert_eq!(max_candles_for_interval(Interval::Weekly), 20 * 52);
        assert_eq!(max_candles_for_interval(Interval::OneMinute), 10 * 390);
        assert_eq!(
            max_candles_for_interval(Interval::FiveMinute),
            10 * (390 / 5)
        );
        assert_eq!(
            max_candles_for_interval(Interval::FifteenMinute),
            10 * (390 / 15)
        );
        assert_eq!(
            max_candles_for_interval(Interval::ThirtyMinute),
            10 * (390 / 30)
        );
    }

    #[test]
    fn daily_uses_year_period_daily_frequency_and_safety_margin() {
        assert_params(
            interval_to_history_params(Interval::Daily, 0).expect("zero candles maps"),
            "year",
            1,
            "daily",
            1,
        );
        assert_params(
            interval_to_history_params(Interval::Daily, 242).expect("safety margin fits one year"),
            "year",
            1,
            "daily",
            1,
        );
        assert_params(
            interval_to_history_params(Interval::Daily, 243).expect("safety margin rounds up"),
            "year",
            2,
            "daily",
            1,
        );
        assert_params(
            interval_to_history_params(Interval::Daily, 747).expect("daily rounds to valid period"),
            "year",
            5,
            "daily",
            1,
        );
    }

    #[test]
    fn weekly_uses_year_period_weekly_frequency() {
        assert_params(
            interval_to_history_params(Interval::Weekly, 1).expect("one candle maps"),
            "year",
            1,
            "weekly",
            1,
        );
        assert_params(
            interval_to_history_params(Interval::Weekly, 53).expect("weekly rounds to two years"),
            "year",
            2,
            "weekly",
            1,
        );
        assert_params(
            interval_to_history_params(Interval::Weekly, 157).expect("weekly rounds to five years"),
            "year",
            5,
            "weekly",
            1,
        );
    }

    #[test]
    fn intraday_intervals_use_day_period_minute_frequency() {
        let cases = [
            (Interval::OneMinute, 1, 1, 1),
            (Interval::OneMinute, 391, 2, 1),
            (Interval::FiveMinute, 79, 2, 5),
            (Interval::FiveMinute, 235, 4, 5),
            (Interval::FifteenMinute, 27, 2, 15),
            (Interval::FifteenMinute, 131, 10, 15),
            (Interval::ThirtyMinute, 14, 2, 30),
            (Interval::ThirtyMinute, 66, 10, 30),
        ];

        for (interval, required_candles, expected_days, expected_frequency) in cases {
            assert_params(
                interval_to_history_params(interval, required_candles).expect("intraday maps"),
                "day",
                expected_days,
                "minute",
                expected_frequency,
            );
        }
    }

    #[test]
    fn exactly_at_boundary_candle_counts_are_allowed() {
        assert_params(
            interval_to_history_params(Interval::Daily, 20 * 252).expect("daily max allowed"),
            "year",
            20,
            "daily",
            1,
        );
        assert_params(
            interval_to_history_params(Interval::Weekly, 20 * 52).expect("weekly max allowed"),
            "year",
            20,
            "weekly",
            1,
        );
        assert_params(
            interval_to_history_params(Interval::OneMinute, 10 * 390)
                .expect("one minute max allowed"),
            "day",
            10,
            "minute",
            1,
        );
        assert_params(
            interval_to_history_params(Interval::ThirtyMinute, 10 * (390 / 30))
                .expect("thirty minute max allowed"),
            "day",
            10,
            "minute",
            30,
        );
    }

    #[test]
    fn exceeding_max_candles_returns_ta_insufficient_data() {
        let error = interval_to_history_params(Interval::FiveMinute, 10 * (390 / 5) + 1)
            .expect_err("too many candles fails");

        assert!(matches!(
            error,
            AppError::TaInsufficientData {
                needed,
                got,
                indicator,
            } if needed == 781 && got == 780 && indicator == "5min"
        ));
    }
}
