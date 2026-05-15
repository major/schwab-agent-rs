//! Hand-rolled technical indicator functions.

/// Moving Average Convergence/Divergence result series.
#[derive(Debug, Clone, PartialEq)]
pub struct MacdResult {
    /// MACD line values aligned to the signal and histogram series.
    pub macd: Vec<f64>,
    /// Signal line values computed as an EMA of the MACD line.
    pub signal: Vec<f64>,
    /// Histogram values computed as MACD minus signal.
    pub histogram: Vec<f64>,
}

/// Bollinger Bands result series.
#[derive(Debug, Clone, PartialEq)]
pub struct BbandsResult {
    /// Upper band values.
    pub upper: Vec<f64>,
    /// Middle SMA values.
    pub middle: Vec<f64>,
    /// Lower band values.
    pub lower: Vec<f64>,
}

/// Stochastic oscillator result series.
#[derive(Debug, Clone, PartialEq)]
pub struct StochResult {
    /// Smoothed percent-K values.
    pub k: Vec<f64>,
    /// Percent-D values computed from smoothed percent-K.
    pub d: Vec<f64>,
}

/// Average Directional Index result series with companion directional indexes.
#[derive(Debug, Clone, PartialEq)]
pub struct AdxResult {
    /// Average Directional Index values.
    pub adx: Vec<f64>,
    /// Positive directional index values aligned to ADX.
    pub plus_di: Vec<f64>,
    /// Negative directional index values aligned to ADX.
    pub minus_di: Vec<f64>,
}

/// Remove only the leading NaN values from a series.
#[must_use]
pub fn strip_leading_nans(data: &[f64]) -> Vec<f64> {
    data.iter()
        .position(|value| !value.is_nan())
        .map_or_else(Vec::new, |index| data[index..].to_vec())
}

/// Compute a simple moving average over fixed-size windows.
#[must_use]
pub fn sma(data: &[f64], period: usize) -> Vec<f64> {
    if period == 0 || data.len() < period {
        return Vec::new();
    }

    let mut sum: f64 = data[..period].iter().sum();
    let mut output = Vec::with_capacity(data.len() - period + 1);
    output.push(sum / period as f64);

    for index in period..data.len() {
        sum += data[index] - data[index - period];
        output.push(sum / period as f64);
    }

    output
}

/// Compute an exponential moving average seeded by the first window SMA.
#[must_use]
pub fn ema(data: &[f64], period: usize) -> Vec<f64> {
    if period == 0 || data.len() < period {
        return Vec::new();
    }

    let multiplier = 2.0 / (period as f64 + 1.0);
    let mut previous = data[..period].iter().sum::<f64>() / period as f64;
    let mut output = Vec::with_capacity(data.len() - period + 1);
    output.push(previous);

    for value in &data[period..] {
        previous = (*value - previous).mul_add(multiplier, previous);
        output.push(previous);
    }

    output
}

/// Compute the Relative Strength Index using Wilder's smoothing.
#[must_use]
pub fn rsi(data: &[f64], period: usize) -> Vec<f64> {
    if period <= 1 || data.len() < period + 1 {
        return Vec::new();
    }

    let mut average_gain = 0.0;
    let mut average_loss = 0.0;
    for index in 1..=period {
        let change = data[index] - data[index - 1];
        if change >= 0.0 {
            average_gain += change;
        } else {
            average_loss -= change;
        }
    }
    average_gain /= period as f64;
    average_loss /= period as f64;

    let mut output = Vec::with_capacity(data.len() - period);
    output.push(rsi_from_averages(average_gain, average_loss));

    for index in (period + 1)..data.len() {
        let change = data[index] - data[index - 1];
        let gain = change.max(0.0);
        let loss = (-change).max(0.0);
        average_gain = ((average_gain * (period - 1) as f64) + gain) / period as f64;
        average_loss = ((average_loss * (period - 1) as f64) + loss) / period as f64;
        output.push(rsi_from_averages(average_gain, average_loss));
    }

    output
}

/// Compute Moving Average Convergence/Divergence, signal, and histogram series.
#[must_use]
pub fn macd(data: &[f64], fast: usize, slow: usize, signal: usize) -> MacdResult {
    if fast == 0 || slow == 0 || signal == 0 || fast >= slow || data.len() < slow {
        return empty_macd();
    }

    let fast_ema = ema(data, fast);
    let slow_ema = ema(data, slow);
    if slow_ema.is_empty() || fast_ema.len() < slow_ema.len() {
        return empty_macd();
    }

    let fast_offset = slow - fast;
    let macd_line: Vec<f64> = slow_ema
        .iter()
        .enumerate()
        .map(|(index, slow_value)| fast_ema[index + fast_offset] - slow_value)
        .collect();
    let signal_line = ema(&macd_line, signal);
    if signal_line.is_empty() {
        return empty_macd();
    }

    let aligned_macd = macd_line[macd_line.len() - signal_line.len()..].to_vec();
    let histogram = aligned_macd
        .iter()
        .zip(&signal_line)
        .map(|(macd_value, signal_value)| macd_value - signal_value)
        .collect();

    MacdResult {
        macd: aligned_macd,
        signal: signal_line,
        histogram,
    }
}

/// Compute Average True Range using Wilder's smoothing.
#[must_use]
pub fn atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Vec<f64> {
    if period == 0 || !same_lengths(highs, lows, closes) || closes.len() < period + 1 {
        return Vec::new();
    }

    let true_ranges = true_ranges(highs, lows, closes);
    if true_ranges.len() < period {
        return Vec::new();
    }

    wilder_average(&true_ranges, period)
}

/// Compute Bollinger Bands with an SMA middle line and population standard deviation.
#[must_use]
pub fn bbands(data: &[f64], period: usize, std_dev: f64) -> BbandsResult {
    if period == 0 || data.len() < period || std_dev <= 0.0 {
        return empty_bbands();
    }

    let mut upper = Vec::with_capacity(data.len() - period + 1);
    let mut middle = Vec::with_capacity(data.len() - period + 1);
    let mut lower = Vec::with_capacity(data.len() - period + 1);

    for window in data.windows(period) {
        let mean = window.iter().sum::<f64>() / period as f64;
        let variance = window
            .iter()
            .map(|value| {
                let diff = value - mean;
                diff * diff
            })
            .sum::<f64>()
            / period as f64;
        let band_width = std_dev * variance.sqrt();
        upper.push(mean + band_width);
        middle.push(mean);
        lower.push(mean - band_width);
    }

    BbandsResult {
        upper,
        middle,
        lower,
    }
}

/// Compute the stochastic oscillator with SMA smoothing for percent-K and percent-D.
#[must_use]
pub fn stochastic(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    k_period: usize,
    smooth_k: usize,
    d_period: usize,
) -> StochResult {
    if k_period == 0
        || smooth_k == 0
        || d_period == 0
        || !same_lengths(highs, lows, closes)
        || closes.len() < k_period
    {
        return empty_stoch();
    }

    let fast_k: Vec<f64> = (k_period - 1..closes.len())
        .map(|index| {
            let window_start = index + 1 - k_period;
            let highest_high = highs[window_start..=index]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let lowest_low = lows[window_start..=index]
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min);
            let range = highest_high - lowest_low;
            if range == 0.0 {
                50.0
            } else {
                (closes[index] - lowest_low) / range * 100.0
            }
        })
        .collect();

    let mut k = sma(&fast_k, smooth_k);
    let d = sma(&k, d_period);
    if d.is_empty() {
        return empty_stoch();
    }
    trim_front_to_len(&mut k, d.len());

    StochResult { k, d }
}

/// Compute Average Directional Index plus aligned positive and negative DI lines.
#[must_use]
pub fn adx(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> AdxResult {
    if period == 0 || !same_lengths(highs, lows, closes) || closes.len() < period + 1 {
        return empty_adx();
    }

    let directional = directional_movement(highs, lows, closes);
    if directional.true_ranges.len() < period {
        return empty_adx();
    }

    let mut smoothed_tr = directional.true_ranges[..period].iter().sum::<f64>();
    let mut smoothed_plus_dm = directional.plus_dm[..period].iter().sum::<f64>();
    let mut smoothed_minus_dm = directional.minus_dm[..period].iter().sum::<f64>();

    let mut plus_di = Vec::with_capacity(directional.true_ranges.len() - period + 1);
    let mut minus_di = Vec::with_capacity(directional.true_ranges.len() - period + 1);
    let mut dx = Vec::with_capacity(directional.true_ranges.len() - period + 1);
    push_directional_indexes(
        smoothed_tr,
        smoothed_plus_dm,
        smoothed_minus_dm,
        &mut plus_di,
        &mut minus_di,
        &mut dx,
    );

    for index in period..directional.true_ranges.len() {
        smoothed_tr = smoothed_tr - (smoothed_tr / period as f64) + directional.true_ranges[index];
        smoothed_plus_dm =
            smoothed_plus_dm - (smoothed_plus_dm / period as f64) + directional.plus_dm[index];
        smoothed_minus_dm =
            smoothed_minus_dm - (smoothed_minus_dm / period as f64) + directional.minus_dm[index];
        push_directional_indexes(
            smoothed_tr,
            smoothed_plus_dm,
            smoothed_minus_dm,
            &mut plus_di,
            &mut minus_di,
            &mut dx,
        );
    }

    let adx = wilder_average(&dx, period);
    if adx.is_empty() {
        return empty_adx();
    }
    trim_front_to_len(&mut plus_di, adx.len());
    trim_front_to_len(&mut minus_di, adx.len());

    AdxResult {
        adx,
        plus_di,
        minus_di,
    }
}

#[derive(Debug)]
struct DirectionalMovement {
    true_ranges: Vec<f64>,
    plus_dm: Vec<f64>,
    minus_dm: Vec<f64>,
}

fn rsi_from_averages(average_gain: f64, average_loss: f64) -> f64 {
    match (average_gain == 0.0, average_loss == 0.0) {
        (true, true) => 50.0,
        (true, false) => 0.0,
        (false, true) => 100.0,
        (false, false) => 100.0 - (100.0 / (1.0 + average_gain / average_loss)),
    }
}

fn true_ranges(highs: &[f64], lows: &[f64], closes: &[f64]) -> Vec<f64> {
    (1..closes.len())
        .map(|index| {
            let high_low = highs[index] - lows[index];
            let high_close = (highs[index] - closes[index - 1]).abs();
            let low_close = (lows[index] - closes[index - 1]).abs();
            high_low.max(high_close).max(low_close)
        })
        .collect()
}

fn wilder_average(data: &[f64], period: usize) -> Vec<f64> {
    if period == 0 || data.len() < period {
        return Vec::new();
    }

    let mut previous = data[..period].iter().sum::<f64>() / period as f64;
    let mut output = Vec::with_capacity(data.len() - period + 1);
    output.push(previous);

    for value in &data[period..] {
        previous = ((previous * (period - 1) as f64) + value) / period as f64;
        output.push(previous);
    }

    output
}

fn directional_movement(highs: &[f64], lows: &[f64], closes: &[f64]) -> DirectionalMovement {
    let mut true_ranges = Vec::with_capacity(closes.len().saturating_sub(1));
    let mut plus_dm = Vec::with_capacity(closes.len().saturating_sub(1));
    let mut minus_dm = Vec::with_capacity(closes.len().saturating_sub(1));

    for index in 1..closes.len() {
        let up_move = highs[index] - highs[index - 1];
        let down_move = lows[index - 1] - lows[index];

        plus_dm.push(if up_move > down_move && up_move > 0.0 {
            up_move
        } else {
            0.0
        });
        minus_dm.push(if down_move > up_move && down_move > 0.0 {
            down_move
        } else {
            0.0
        });

        let high_low = highs[index] - lows[index];
        let high_close = (highs[index] - closes[index - 1]).abs();
        let low_close = (lows[index] - closes[index - 1]).abs();
        true_ranges.push(high_low.max(high_close).max(low_close));
    }

    DirectionalMovement {
        true_ranges,
        plus_dm,
        minus_dm,
    }
}

fn push_directional_indexes(
    smoothed_tr: f64,
    smoothed_plus_dm: f64,
    smoothed_minus_dm: f64,
    plus_di: &mut Vec<f64>,
    minus_di: &mut Vec<f64>,
    dx: &mut Vec<f64>,
) {
    let plus_value = if smoothed_tr == 0.0 {
        0.0
    } else {
        100.0 * smoothed_plus_dm / smoothed_tr
    };
    let minus_value = if smoothed_tr == 0.0 {
        0.0
    } else {
        100.0 * smoothed_minus_dm / smoothed_tr
    };
    let directional_sum = plus_value + minus_value;
    let dx_value = if directional_sum == 0.0 {
        0.0
    } else {
        100.0 * (plus_value - minus_value).abs() / directional_sum
    };

    plus_di.push(plus_value);
    minus_di.push(minus_value);
    dx.push(dx_value);
}

fn trim_front_to_len(values: &mut Vec<f64>, len: usize) {
    if values.len() > len {
        values.drain(..values.len() - len);
    }
}

fn same_lengths(first: &[f64], second: &[f64], third: &[f64]) -> bool {
    first.len() == second.len() && second.len() == third.len()
}

fn empty_macd() -> MacdResult {
    MacdResult {
        macd: Vec::new(),
        signal: Vec::new(),
        histogram: Vec::new(),
    }
}

fn empty_bbands() -> BbandsResult {
    BbandsResult {
        upper: Vec::new(),
        middle: Vec::new(),
        lower: Vec::new(),
    }
}

fn empty_stoch() -> StochResult {
    StochResult {
        k: Vec::new(),
        d: Vec::new(),
    }
}

fn empty_adx() -> AdxResult {
    AdxResult {
        adx: Vec::new(),
        plus_di: Vec::new(),
        minus_di: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-6;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= EPSILON,
            "actual {actual} differs from expected {expected}",
        );
    }

    fn assert_series_close(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "series length mismatch");
        for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (actual - expected).abs() <= EPSILON,
                "index {index}: actual {actual} differs from expected {expected}",
            );
        }
    }

    #[test]
    fn strips_leading_nans_without_touching_later_values() {
        let data = [f64::NAN, f64::NAN, 1.0, f64::NAN, 2.0];

        let stripped = strip_leading_nans(&data);

        assert_eq!(stripped.len(), 3);
        assert_close(stripped[0], 1.0);
        assert!(stripped[1].is_nan());
        assert_close(stripped[2], 2.0);
    }

    #[test]
    fn sma_returns_window_averages() {
        let actual = sma(&[1.0, 2.0, 3.0, 4.0, 5.0], 3);

        assert_series_close(&actual, &[2.0, 3.0, 4.0]);
    }

    #[test]
    fn ema_uses_sma_seed_then_exponential_updates() {
        let actual = ema(&[1.0, 2.0, 3.0, 4.0, 5.0], 3);

        assert_series_close(&actual, &[2.0, 3.0, 4.0]);
    }

    #[test]
    fn rsi_uses_wilder_smoothing() {
        let actual = rsi(&[44.0, 44.15, 43.9, 44.35, 44.8, 45.0, 44.7, 45.2, 45.6], 3);

        assert_series_close(
            &actual,
            &[
                70.588_235_294_117_71,
                83.606_557_377_049_13,
                87.341_772_151_898_75,
                57.740_585_774_058_786,
                77.123_442_808_607_17,
                85.244_704_163_623_17,
            ],
        );
    }

    #[test]
    fn macd_returns_aligned_macd_signal_and_histogram() {
        let data = [
            1.0, 2.0, 3.0, 4.0, 5.0, 7.0, 8.0, 7.0, 9.0, 10.0, 12.0, 11.0, 13.0, 15.0, 14.0,
        ];

        let actual = macd(&data, 3, 6, 3);

        assert_series_close(
            &actual.macd,
            &[
                1.371_598_639_455_782_2,
                1.435_070_456_754_130_1,
                1.467_014_611_967_235_8,
                1.697_421_151_405_167_8,
                1.322_934_751_003_691_3,
                1.428_770_357_859_779_2,
                1.691_030_166_328_413_7,
                1.328_832_931_306_010_3,
            ],
        );
        assert_series_close(
            &actual.signal,
            &[
                1.683_390_022_675_736_8,
                1.559_230_239_714_933_6,
                1.513_122_425_841_084_7,
                1.605_271_788_623_126_3,
                1.464_103_269_813_408_7,
                1.446_436_813_836_594,
                1.568_733_490_082_503_8,
                1.448_783_210_694_257,
            ],
        );
        assert_series_close(
            &actual.histogram,
            &[
                -0.311_791_383_219_954_6,
                -0.124_159_782_960_803_43,
                -0.046_107_813_873_848_88,
                0.092_149_362_782_041_56,
                -0.141_168_518_809_717_37,
                -0.017_666_455_976_814_71,
                0.122_296_676_245_909_9,
                -0.119_950_279_388_246_76,
            ],
        );
    }

    #[test]
    fn atr_uses_true_range_with_wilder_smoothing() {
        let highs = [10.0, 12.0, 13.0, 14.0, 13.0, 15.0, 16.0];
        let lows = [9.0, 10.0, 11.0, 12.0, 11.0, 13.0, 14.0];
        let closes = [9.5, 11.0, 12.0, 13.0, 12.0, 14.0, 15.0];

        let actual = atr(&highs, &lows, &closes, 3);

        assert_series_close(
            &actual,
            &[
                2.166_666_666_666_666_5,
                2.111_111_111_111_111,
                2.407_407_407_407_407_4,
                2.271_604_938_271_605,
            ],
        );
    }

    #[test]
    fn bbands_returns_sma_middle_and_population_std_dev_bands() {
        let actual = bbands(&[1.0, 2.0, 3.0, 4.0, 5.0], 3, 2.0);

        assert_series_close(
            &actual.upper,
            &[
                3.632_993_161_855_452,
                4.632_993_161_855_452_5,
                5.632_993_161_855_452_5,
            ],
        );
        assert_series_close(&actual.middle, &[2.0, 3.0, 4.0]);
        assert_series_close(
            &actual.lower,
            &[
                0.367_006_838_144_547_93,
                1.367_006_838_144_548,
                2.367_006_838_144_548,
            ],
        );
    }

    #[test]
    fn stochastic_returns_smoothed_k_and_d() {
        let highs = [10.0, 12.0, 14.0, 16.0, 18.0, 20.0];
        let lows = [8.0, 9.0, 10.0, 12.0, 14.0, 16.0];
        let closes = [9.0, 11.0, 13.0, 15.0, 17.0, 19.0];

        let actual = stochastic(&highs, &lows, &closes, 3, 2, 2);

        assert_series_close(&actual.k, &[86.607_142_857_142_86, 87.5]);
        assert_series_close(&actual.d, &[85.565_476_190_476_19, 87.053_571_428_571_43]);
    }

    #[test]
    fn stochastic_flat_price_uses_neutral_fast_k() {
        let highs = [10.0, 10.0, 10.0, 10.0, 10.0];
        let lows = [10.0, 10.0, 10.0, 10.0, 10.0];
        let closes = [10.0, 10.0, 10.0, 10.0, 10.0];

        let actual = stochastic(&highs, &lows, &closes, 3, 2, 2);

        assert_series_close(&actual.k, &[50.0]);
        assert_series_close(&actual.d, &[50.0]);
    }

    #[test]
    fn adx_returns_aligned_adx_and_di_lines() {
        let highs = [30.0, 32.0, 31.0, 35.0, 36.0, 38.0, 37.0, 39.0, 41.0, 43.0];
        let lows = [28.0, 29.0, 27.0, 30.0, 31.0, 33.0, 32.0, 34.0, 36.0, 37.0];
        let closes = [29.0, 31.0, 28.0, 34.0, 35.0, 37.0, 33.0, 38.0, 40.0, 42.0];

        let actual = adx(&highs, &lows, &closes, 3);

        assert_series_close(
            &actual.adx,
            &[
                59.774_436_090_225_57,
                52.559_456_194_442_95,
                55.342_667_765_992_27,
                61.834_106_475_522_42,
                69.082_721_739_449_31,
            ],
        );
        assert_series_close(
            &actual.plus_di,
            &[
                36.641_221_374_045_8,
                24.181_360_201_511_335,
                27.656_25,
                31.629_139_072_847_682,
                32.254_277_088_225_43,
            ],
        );
        assert_series_close(
            &actual.minus_di,
            &[
                6.106_870_229_007_635,
                10.831_234_256_926_953,
                6.718_750_000_000_002,
                4.556_291_390_728_478,
                2.884_937_940_288_495,
            ],
        );
    }
}
