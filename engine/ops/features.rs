/// Feature construction primitives — composable building blocks, all O(n).

use super::rolling::rolling_ema;

/// EMA spread: (fast_ema - slow_ema) / |slow_ema| * 10000 (basis points).
/// Clamped to [-5000, 5000].
pub fn ema_spread(x: &[f64], fast_span: usize, slow_span: usize) -> Vec<f64> {
    let fast = rolling_ema(x, fast_span, 1);
    let slow = rolling_ema(x, slow_span, 1);
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    for i in 0..n {
        let f = fast[i];
        let s = slow[i];
        if !f.is_nan() && !s.is_nan() && s.abs() > 1e-10 {
            let v = (f - s) / s.abs() * 10000.0;
            out[i] = v.clamp(-5000.0, 5000.0);
        }
    }
    out
}

/// EMA slope: (ema[t] - ema[t-lookback]) / |ema[t-lookback]| * 10000 (bp).
/// Clamped to [-1000, 1000].
pub fn ema_slope(x: &[f64], span: usize, lookback: usize) -> Vec<f64> {
    let ema = rolling_ema(x, span, 1);
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    for i in lookback..n {
        let now = ema[i];
        let prev = ema[i - lookback];
        if !now.is_nan() && !prev.is_nan() && prev.abs() > 1e-10 {
            let v = (now - prev) / prev.abs() * 10000.0;
            out[i] = v.clamp(-1000.0, 1000.0);
        }
    }
    out
}

/// Spread-adjusted slope: sign(spread) * sqrt(|spread| * |slope|).
/// Combines trend direction (spread) with rate of change (slope).
pub fn spread_adjusted_slope(x: &[f64], fast_span: usize, slow_span: usize) -> Vec<f64> {
    let spread = ema_spread(x, fast_span, slow_span);
    let slope = ema_slope(x, fast_span, fast_span);
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    for i in 0..n {
        let sp = spread[i];
        let sl = slope[i];
        if !sp.is_nan() && !sl.is_nan() {
            out[i] = sp.signum() * (sp.abs() * sl.abs()).sqrt();
        }
    }
    out
}

/// Rolling delta: x[t] - x[t-window]. O(n).
pub fn rolling_delta(x: &[f64], window: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    for i in window..n {
        let now = x[i];
        let prev = x[i - window];
        if !now.is_nan() && !prev.is_nan() {
            out[i] = now - prev;
        }
    }
    out
}

/// Rolling delta normalized by rolling std. O(n).
pub fn rolling_delta_norm(
    x: &[f64],
    delta_window: usize,
    norm_window: usize,
    min_periods: usize,
) -> Vec<f64> {
    let delta = rolling_delta(x, delta_window);
    let std = super::rolling::rolling_std(&delta, norm_window, min_periods);
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    for i in 0..n {
        if !delta[i].is_nan() && !std[i].is_nan() && std[i] > 1e-15 {
            out[i] = delta[i] / std[i];
        }
    }
    out
}

/// Log return over N periods: log(x[t] / x[t-period]) * 10000 (bp). O(n).
pub fn log_return(x: &[f64], period: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    for i in period..n {
        let now = x[i];
        let prev = x[i - period];
        if !now.is_nan() && !prev.is_nan() && prev > 0.0 {
            out[i] = (now / prev).ln() * 10000.0;
        }
    }
    out
}

/// Absolute move: |log(x[t] / x[t-window])| * 10000 (bp). O(n).
pub fn abs_move(x: &[f64], window: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    for i in window..n {
        let now = x[i];
        let prev = x[i - window];
        if !now.is_nan() && !prev.is_nan() && prev > 0.0 {
            out[i] = ((now / prev).ln() * 10000.0).abs();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_price_series() -> Vec<f64> {
        // Simulated uptrend: 100 → ~110 over 200 bars
        (0..200)
            .map(|i| 100.0 + (i as f64) * 0.05 + (i as f64 * 0.1).sin() * 0.5)
            .collect()
    }

    #[test]
    fn test_ema_spread_sign() {
        let x = make_price_series();
        let spread = ema_spread(&x, 24, 120);
        // In uptrend, spread should be positive after warmup
        let valid: Vec<f64> = spread[130..].iter().filter(|v| !v.is_nan()).copied().collect();
        assert!(!valid.is_empty());
        let mean: f64 = valid.iter().sum::<f64>() / valid.len() as f64;
        assert!(mean > 0.0, "Uptrend should have positive spread");
    }

    #[test]
    fn test_spread_adjusted_slope() {
        let x = make_price_series();
        let sas = spread_adjusted_slope(&x, 24, 120);
        let valid: Vec<f64> = sas[130..].iter().filter(|v| !v.is_nan()).copied().collect();
        assert!(!valid.is_empty());
        let mean: f64 = valid.iter().sum::<f64>() / valid.len() as f64;
        assert!(mean > 0.0, "Uptrend should have positive SAS");
    }

    #[test]
    fn test_abs_move() {
        let x = vec![100.0, 101.0, 99.0, 102.0, 98.0];
        let out = abs_move(&x, 1);
        assert!(out[0].is_nan());
        assert!(!out[1].is_nan());
        assert!(out[1] > 0.0); // always positive
        assert!(out[2] > 0.0);
    }

    #[test]
    fn test_log_return() {
        let x = vec![100.0, 110.0, 100.0];
        let out = log_return(&x, 1);
        assert!(out[1] > 0.0); // 100→110 positive
        assert!(out[2] < 0.0); // 110→100 negative
    }

    #[test]
    fn test_clamp_spread() {
        // Very large spread should be clamped
        let x: Vec<f64> = (0..200).map(|i| if i < 100 { 1.0 } else { 1000.0 }).collect();
        let spread = ema_spread(&x, 5, 50);
        for v in &spread {
            if !v.is_nan() {
                assert!(*v <= 5000.0 && *v >= -5000.0, "Should be clamped");
            }
        }
    }
}
