//! Candle data extraction helpers.

use crate::error::AppError;

/// All OHLCV data extracted from Schwab candles.
#[derive(Debug, Clone)]
pub struct OhlcvData {
    /// Candle open prices.
    pub opens: Vec<f64>,
    /// Candle high prices.
    pub highs: Vec<f64>,
    /// Candle low prices.
    pub lows: Vec<f64>,
    /// Candle close prices.
    pub closes: Vec<f64>,
    /// Candle volumes represented as floating-point values for indicator math.
    pub volumes: Vec<f64>,
    /// Candle timestamps as Unix epoch millisecond values from Schwab.
    pub timestamps: Vec<i64>,
}

/// Validates that a `CandleList` has enough candles for the requested computation.
pub fn validate_candles(
    candles: &schwab::CandleList,
    min_required: usize,
) -> Result<&Vec<schwab::Candle>, AppError> {
    let candle_data = candles
        .candles
        .as_ref()
        .ok_or_else(|| insufficient_data(min_required, 0))?;
    if candle_data.is_empty() || candle_data.len() < min_required {
        return Err(insufficient_data(min_required, candle_data.len()));
    }

    Ok(candle_data)
}

/// Extracts close prices from Schwab candles.
pub fn extract_closes(candles: &[schwab::Candle]) -> Result<Vec<f64>, AppError> {
    extract_prices(candles, "close", |candle| candle.close)
}

/// Extracts high prices from Schwab candles.
pub fn extract_highs(candles: &[schwab::Candle]) -> Result<Vec<f64>, AppError> {
    extract_prices(candles, "high", |candle| candle.high)
}

/// Extracts low prices from Schwab candles.
pub fn extract_lows(candles: &[schwab::Candle]) -> Result<Vec<f64>, AppError> {
    extract_prices(candles, "low", |candle| candle.low)
}

/// Extracts open prices from Schwab candles.
pub fn extract_opens(candles: &[schwab::Candle]) -> Result<Vec<f64>, AppError> {
    extract_prices(candles, "open", |candle| candle.open)
}

/// Extracts volumes from Schwab candles as `f64` values for indicator math.
pub fn extract_volumes(candles: &[schwab::Candle]) -> Result<Vec<f64>, AppError> {
    candles
        .iter()
        .enumerate()
        .map(|(index, candle)| {
            candle
                .volume
                .map(|volume| volume as f64)
                .ok_or_else(|| missing_field("volume", index))
        })
        .collect()
}

/// Extracts timestamps from Schwab candles, defaulting missing timestamps to zero.
#[must_use]
pub fn extract_timestamps(candles: &[schwab::Candle]) -> Vec<i64> {
    candles
        .iter()
        .map(|candle| candle.datetime.unwrap_or(0))
        .collect()
}

/// Extracts all OHLCV fields from Schwab candles in one pass.
pub fn extract_ohlcv(candles: &[schwab::Candle]) -> Result<OhlcvData, AppError> {
    let mut data = OhlcvData {
        opens: Vec::with_capacity(candles.len()),
        highs: Vec::with_capacity(candles.len()),
        lows: Vec::with_capacity(candles.len()),
        closes: Vec::with_capacity(candles.len()),
        volumes: Vec::with_capacity(candles.len()),
        timestamps: Vec::with_capacity(candles.len()),
    };

    for (index, candle) in candles.iter().enumerate() {
        data.opens.push(price_value(candle.open, "open", index)?);
        data.highs.push(price_value(candle.high, "high", index)?);
        data.lows.push(price_value(candle.low, "low", index)?);
        data.closes.push(price_value(candle.close, "close", index)?);
        data.volumes.push(
            candle
                .volume
                .map(|volume| volume as f64)
                .ok_or_else(|| missing_field("volume", index))?,
        );
        data.timestamps.push(candle.datetime.unwrap_or(0));
    }

    Ok(data)
}

fn insufficient_data(needed: usize, got: usize) -> AppError {
    AppError::TaInsufficientData {
        needed: needed.max(1),
        got,
        indicator: "candles".to_string(),
    }
}

fn extract_prices(
    candles: &[schwab::Candle],
    field: &'static str,
    value: fn(&schwab::Candle) -> Option<schwab::Number>,
) -> Result<Vec<f64>, AppError> {
    candles
        .iter()
        .enumerate()
        .map(|(index, candle)| price_value(value(candle), field, index))
        .collect()
}

fn price_value(
    value: Option<schwab::Number>,
    field: &'static str,
    index: usize,
) -> Result<f64, AppError> {
    let number = value.ok_or_else(|| missing_field(field, index))?;

    number_to_f64(number).ok_or_else(|| AppError::TaCalculationError {
        indicator: "candles".to_string(),
        reason: format!("invalid {field} value at candle index {index}"),
    })
}

#[cfg(not(feature = "decimal"))]
fn number_to_f64(value: schwab::Number) -> Option<f64> {
    Some(value)
}

#[cfg(feature = "decimal")]
fn number_to_f64(value: schwab::Number) -> Option<f64> {
    value.to_string().parse::<f64>().ok()
}

fn missing_field(field: &'static str, index: usize) -> AppError {
    AppError::TaCalculationError {
        indicator: "candles".to_string(),
        reason: format!("missing {field} value at candle index {index}"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::error::AppError;

    fn candle_list_from_json(candles: serde_json::Value) -> schwab::CandleList {
        serde_json::from_value(json!({
            "candles": candles,
            "empty": false,
            "symbol": "SPY"
        }))
        .expect("test candle list should deserialize")
    }

    fn sample_candle_list() -> schwab::CandleList {
        candle_list_from_json(json!([
            {
                "open": 100.0,
                "high": 101.5,
                "low": 99.5,
                "close": 101.0,
                "volume": 12345,
                "datetime": 1_700_000_000
            },
            {
                "open": 102.0,
                "high": 103.5,
                "low": 101.5,
                "close": 103.0,
                "volume": 23456,
                "datetime": 1_700_086_400
            }
        ]))
    }

    #[test]
    fn validate_candles_accepts_sufficient_data() {
        let candle_list = sample_candle_list();

        let candles = validate_candles(&candle_list, 2).expect("candles should be valid");

        assert_eq!(candles.len(), 2);
    }

    #[test]
    fn validate_candles_rejects_none_and_empty_lists() {
        let none_list: schwab::CandleList = serde_json::from_value(json!({
            "empty": true,
            "symbol": "SPY"
        }))
        .expect("test candle list should deserialize");
        let empty_list = candle_list_from_json(json!([]));

        for candle_list in [&none_list, &empty_list] {
            let error = validate_candles(candle_list, 1).expect_err("list should be invalid");

            assert!(matches!(
                error,
                AppError::TaInsufficientData {
                    needed: 1,
                    got: 0,
                    ..
                }
            ));
        }
    }

    #[test]
    fn validate_candles_rejects_insufficient_data() {
        let candle_list = sample_candle_list();

        let error = validate_candles(&candle_list, 3).expect_err("list should be too short");

        assert!(matches!(
            error,
            AppError::TaInsufficientData {
                needed: 3,
                got: 2,
                ..
            }
        ));
    }

    #[test]
    fn extracts_typed_ohlcv_vectors() {
        let candle_list = sample_candle_list();
        let candles = validate_candles(&candle_list, 1).expect("candles should be valid");

        assert_eq!(
            extract_opens(candles).expect("opens should extract"),
            vec![100.0, 102.0]
        );
        assert_eq!(
            extract_highs(candles).expect("highs should extract"),
            vec![101.5, 103.5]
        );
        assert_eq!(
            extract_lows(candles).expect("lows should extract"),
            vec![99.5, 101.5]
        );
        assert_eq!(
            extract_closes(candles).expect("closes should extract"),
            vec![101.0, 103.0]
        );
        assert_eq!(
            extract_volumes(candles).expect("volumes should extract"),
            vec![12345.0, 23456.0]
        );
        assert_eq!(
            extract_timestamps(candles),
            vec![1_700_000_000, 1_700_086_400]
        );
    }

    #[test]
    fn extraction_reports_missing_required_fields() {
        let candle_list = candle_list_from_json(json!([
            {
                "open": 100.0,
                "high": 101.5,
                "low": 99.5,
                "volume": 12345,
                "datetime": 1_700_000_000
            }
        ]));
        let candles = validate_candles(&candle_list, 1).expect("candles should be present");

        let error = extract_closes(candles).expect_err("missing close should fail");

        match error {
            AppError::TaCalculationError { indicator, reason } => {
                assert_eq!(indicator, "candles");
                assert!(reason.contains("close"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn extract_ohlcv_returns_all_series_in_one_call() {
        let candle_list = sample_candle_list();
        let candles = validate_candles(&candle_list, 1).expect("candles should be valid");

        let data = extract_ohlcv(candles).expect("ohlcv should extract");

        assert_eq!(data.opens, vec![100.0, 102.0]);
        assert_eq!(data.highs, vec![101.5, 103.5]);
        assert_eq!(data.lows, vec![99.5, 101.5]);
        assert_eq!(data.closes, vec![101.0, 103.0]);
        assert_eq!(data.volumes, vec![12345.0, 23456.0]);
        assert_eq!(data.timestamps, vec![1_700_000_000, 1_700_086_400]);
    }

    #[test]
    fn timestamp_extraction_defaults_missing_values_to_zero() {
        let candle_list = candle_list_from_json(json!([
            {
                "open": 100.0,
                "high": 101.5,
                "low": 99.5,
                "close": 101.0,
                "volume": 12345
            }
        ]));
        let candles = validate_candles(&candle_list, 1).expect("candles should be present");

        assert_eq!(extract_timestamps(candles), vec![0]);
    }
}
