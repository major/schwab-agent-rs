//! Custom TA implementations: VWAP and Historical Volatility.

use crate::error::AppError;

use super::interval::TRADING_DAYS_PER_YEAR;

const TYPICAL_PRICE_COMPONENT_COUNT: f64 = 3.0;

/// Computes the running volume-weighted average price from OHLCV slices.
pub fn vwap(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
) -> Result<Vec<f64>, AppError> {
    validate_vwap_inputs(highs, lows, closes, volumes)?;

    let mut values = Vec::with_capacity(highs.len());
    let mut cumulative_price_volume = 0.0;
    let mut cumulative_volume = 0.0;

    for (((high, low), close), volume) in highs.iter().zip(lows).zip(closes).zip(volumes) {
        let typical_price = (high + low + close) / TYPICAL_PRICE_COMPONENT_COUNT;
        cumulative_price_volume += typical_price * volume;
        cumulative_volume += volume;

        if cumulative_volume == 0.0 {
            return Err(ta_calculation_error(
                "vwap",
                "requires non-zero cumulative volume",
            ));
        }

        values.push(cumulative_price_volume / cumulative_volume);
    }

    Ok(values)
}

/// Computes rolling close-to-close historical volatility from log returns.
#[must_use]
pub fn historical_volatility(closes: &[f64], period: usize) -> Vec<f64> {
    if period == 0 || closes.len() <= period {
        return Vec::new();
    }

    let returns = closes
        .windows(2)
        .map(|window| (window[1] / window[0]).ln())
        .collect::<Vec<_>>();
    let annualization_factor = (TRADING_DAYS_PER_YEAR as f64).sqrt();

    returns
        .windows(period)
        .map(|window| sample_standard_deviation(window) * annualization_factor)
        .collect()
}

fn validate_vwap_inputs(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    volumes: &[f64],
) -> Result<(), AppError> {
    if highs.is_empty() || lows.is_empty() || closes.is_empty() || volumes.is_empty() {
        return Err(ta_calculation_error(
            "vwap",
            "requires non-empty price and volume slices",
        ));
    }

    if highs.len() != lows.len() || highs.len() != closes.len() || highs.len() != volumes.len() {
        return Err(ta_calculation_error(
            "vwap",
            "requires equal-length price and volume slices",
        ));
    }

    Ok(())
}

#[must_use]
fn sample_standard_deviation(values: &[f64]) -> f64 {
    if values.len() <= 1 {
        return 0.0;
    }

    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| {
            let diff = value - mean;
            diff * diff
        })
        .sum::<f64>()
        / (values.len() - 1) as f64;

    variance.sqrt()
}

fn ta_calculation_error(indicator: &str, reason: &str) -> AppError {
    AppError::TaCalculationError {
        indicator: indicator.to_string(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;

    use super::*;

    const EPSILON: f64 = 1e-6;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "expected {actual} to be within {EPSILON} of {expected}"
        );
    }

    #[test]
    fn vwap_returns_running_volume_weighted_average_price() {
        let values = vwap(
            &[10.0, 12.0, 14.0],
            &[8.0, 10.0, 12.0],
            &[9.0, 11.0, 13.0],
            &[100.0, 200.0, 300.0],
        )
        .expect("non-zero volume computes VWAP");

        assert_eq!(values.len(), 3);
        assert_close(values[0], 9.0);
        assert_close(values[1], 10.333_333_333_333_334);
        assert_close(values[2], 11.666_666_666_666_666);
    }

    #[test]
    fn vwap_rejects_zero_cumulative_volume() {
        let error = vwap(&[10.0], &[8.0], &[9.0], &[0.0]).expect_err("zero volume fails");

        assert!(matches!(
            error,
            AppError::TaCalculationError { indicator, .. } if indicator == "vwap"
        ));
    }

    #[test]
    fn historical_volatility_uses_log_returns_and_annualization() {
        let values = historical_volatility(&[100.0, 102.0, 101.0, 105.0], 2);

        assert_eq!(values.len(), 2);
        assert_close(values[0], 0.332_875_693_388_889_4);
        assert_close(values[1], 0.546_567_800_974_645_9);
    }

    #[test]
    fn historical_volatility_output_length_drops_period_from_closes() {
        let values = historical_volatility(&[100.0, 101.0, 102.0, 103.0, 104.0], 3);

        assert_eq!(values.len(), 2);
    }
}
