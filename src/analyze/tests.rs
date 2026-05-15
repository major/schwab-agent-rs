//! Analyze module tests.

use serde_json::{Value, json};

use super::envelope_from_results;
use crate::ta::types::{
    AdxPoint, AnalyzeSymbolResult, BbandsPoint, DashboardOutput, DerivedFields, MacdPoint,
    MomentumIndicators, MomentumSignal, SignalSummary, StochPoint, TaPoint, TrendIndicators,
    TrendSignal, VolatilityIndicators, VolatilitySignal, VolumeIndicators, VolumeSignal,
};

#[test]
fn analyze_envelope_all_succeed_has_no_warnings() {
    let envelope = envelope_from_results(vec![successful_result("AAPL")]);

    assert!(envelope.ok);
    assert_eq!(envelope.command.as_deref(), Some("analyze"));
    assert!(envelope.error.is_none());
    assert!(envelope.warnings.is_empty());

    let result = only_result(&envelope);
    assert!(result.quote.is_some());
    assert!(result.analysis.is_some());
    assert!(result.quote_error.is_none());
    assert!(result.analysis_error.is_none());
}

#[test]
fn analyze_envelope_quote_fails_dashboard_succeeds_warns_and_keeps_analysis() {
    let envelope = envelope_from_results(vec![quote_failed_result("AAPL")]);

    assert!(envelope.ok);
    assert_eq!(envelope.warnings, ["AAPL: quote failed: quote timeout"]);

    let result = only_result(&envelope);
    assert!(result.quote.is_none());
    assert!(result.analysis.is_some());
    assert_eq!(result.quote_error.as_deref(), Some("quote timeout"));
    assert!(result.analysis_error.is_none());
}

#[test]
fn analyze_envelope_dashboard_fails_quote_succeeds_warns_and_keeps_quote() {
    let envelope = envelope_from_results(vec![analysis_failed_result("AAPL")]);

    assert!(envelope.ok);
    assert_eq!(
        envelope.warnings,
        ["AAPL: analysis failed: not enough candles"]
    );

    let result = only_result(&envelope);
    assert!(result.quote.is_some());
    assert!(result.analysis.is_none());
    assert!(result.quote_error.is_none());
    assert_eq!(result.analysis_error.as_deref(), Some("not enough candles"));
}

#[test]
fn analyze_envelope_single_symbol_both_fail_is_not_ok() {
    let envelope = envelope_from_results(vec![failed_result("AAPL")]);

    assert!(!envelope.ok);
    assert_eq!(
        envelope.warnings,
        [
            "AAPL: quote failed: quote timeout",
            "AAPL: analysis failed: not enough candles",
        ]
    );

    let result = only_result(&envelope);
    assert!(result.quote.is_none());
    assert!(result.analysis.is_none());
    assert_eq!(result.quote_error.as_deref(), Some("quote timeout"));
    assert_eq!(result.analysis_error.as_deref(), Some("not enough candles"));
}

#[test]
fn analyze_envelope_multi_symbol_mixed_results_is_ok_with_all_warnings() {
    let envelope = envelope_from_results(vec![
        quote_failed_result("AAPL"),
        failed_result("MSFT"),
        successful_result("SPY"),
    ]);

    assert!(envelope.ok);
    assert_eq!(
        envelope.warnings,
        [
            "AAPL: quote failed: quote timeout",
            "MSFT: quote failed: quote timeout",
            "MSFT: analysis failed: not enough candles",
        ]
    );

    let data = envelope.data.as_ref().expect("analyze data");
    assert_eq!(data.results.len(), 3);
    assert!(data.results[0].analysis.is_some());
    assert!(data.results[1].quote.is_none());
    assert!(data.results[1].analysis.is_none());
    assert!(data.results[2].quote.is_some());
    assert!(data.results[2].analysis.is_some());
}

#[test]
fn analyze_envelope_multi_symbol_all_completely_fail_is_not_ok() {
    let envelope = envelope_from_results(vec![failed_result("AAPL"), failed_result("MSFT")]);

    assert!(!envelope.ok);
    assert_eq!(envelope.warnings.len(), 4);

    let data = envelope.data.as_ref().expect("analyze data");
    assert_eq!(data.results.len(), 2);
    assert!(
        data.results
            .iter()
            .all(|result| result.quote.is_none() && result.analysis.is_none())
    );
}

fn only_result(
    envelope: &crate::output::Envelope<crate::ta::types::AnalyzeOutput>,
) -> &AnalyzeSymbolResult {
    let data = envelope.data.as_ref().expect("analyze data");
    assert_eq!(data.results.len(), 1);
    &data.results[0]
}

fn successful_result(symbol: &str) -> AnalyzeSymbolResult {
    result(
        symbol,
        Some(quote_value(symbol)),
        Some(sample_dashboard(symbol)),
        None,
        None,
    )
}

fn quote_failed_result(symbol: &str) -> AnalyzeSymbolResult {
    result(
        symbol,
        None,
        Some(sample_dashboard(symbol)),
        Some("quote timeout"),
        None,
    )
}

fn analysis_failed_result(symbol: &str) -> AnalyzeSymbolResult {
    result(
        symbol,
        Some(quote_value(symbol)),
        None,
        None,
        Some("not enough candles"),
    )
}

fn failed_result(symbol: &str) -> AnalyzeSymbolResult {
    result(
        symbol,
        None,
        None,
        Some("quote timeout"),
        Some("not enough candles"),
    )
}

fn result(
    symbol: &str,
    quote: Option<Value>,
    analysis: Option<DashboardOutput>,
    quote_error: Option<&str>,
    analysis_error: Option<&str>,
) -> AnalyzeSymbolResult {
    AnalyzeSymbolResult {
        symbol: symbol.to_string(),
        quote,
        analysis,
        quote_error: quote_error.map(str::to_string),
        analysis_error: analysis_error.map(str::to_string),
    }
}

fn quote_value(symbol: &str) -> Value {
    json!({
        symbol: {
            "assetMainType": "EQUITY",
            "symbol": symbol,
            "quote": { "lastPrice": 123.45 }
        }
    })
}

fn sample_point() -> TaPoint {
    TaPoint {
        timestamp: 1,
        value: 2.5,
    }
}

fn sample_dashboard(symbol: &str) -> DashboardOutput {
    DashboardOutput {
        symbol: symbol.to_string(),
        interval: "daily".to_string(),
        points: 1,
        trend: TrendIndicators {
            sma_21: vec![sample_point()],
            sma_50: vec![sample_point()],
            sma_200: vec![sample_point()],
            ema_21: vec![sample_point()],
        },
        momentum: MomentumIndicators {
            rsi_14: vec![sample_point()],
            macd: vec![MacdPoint {
                timestamp: 1,
                macd: 1.0,
                signal: 0.5,
                histogram: 0.5,
            }],
            stochastic: vec![StochPoint {
                timestamp: 1,
                k: 50.0,
                d: 45.0,
            }],
            adx: vec![AdxPoint {
                timestamp: 1,
                adx: 25.0,
                plus_di: 30.0,
                minus_di: 20.0,
            }],
        },
        volatility: VolatilityIndicators {
            atr_14: vec![sample_point()],
            bollinger_bands: vec![BbandsPoint {
                timestamp: 1,
                upper: 110.0,
                middle: 100.0,
                lower: 90.0,
            }],
            historical_volatility: vec![sample_point()],
        },
        volume: VolumeIndicators {
            vwap: vec![sample_point()],
            avg_volume_20: vec![sample_point()],
            relative_volume: Some(1.2),
        },
        derived: DerivedFields {
            atr_percent: 1.0,
            range_20_high: 110.0,
            range_20_low: 90.0,
            range_252_high: 150.0,
            range_252_low: 80.0,
            distance_from_sma_21: 0.1,
            distance_from_sma_50: 0.2,
            distance_from_sma_200: 0.3,
        },
        signals: SignalSummary {
            trend: TrendSignal {
                above_sma_21: true,
                above_sma_50: true,
                above_sma_200: true,
                sma_21_above_sma_50: true,
                sma_50_above_sma_200: true,
            },
            momentum: MomentumSignal {
                rsi_overbought: false,
                rsi_oversold: false,
                macd_bullish: true,
                stoch_overbought: false,
                stoch_oversold: false,
                adx_trending: true,
            },
            volatility: VolatilitySignal {
                atr_elevated: false,
                price_near_upper_band: false,
                price_near_lower_band: false,
            },
            volume: VolumeSignal {
                high_relative_volume: true,
                price_above_vwap: true,
            },
        },
    }
}
