/// Rolling window operations — all O(n), NaN-aware.

/// Exponential Moving Average. α = 2/(span+1).
/// NaN values are skipped (state carries forward).
/// Returns NaN until min_periods non-NaN values seen.
pub fn rolling_ema(x: &[f64], span: usize, min_periods: usize) -> Vec<f64> {
    let n = x.len();
    let alpha = 2.0 / (span as f64 + 1.0);
    let mut out = vec![f64::NAN; n];
    let mut ema = 0.0;
    let mut count = 0u64;
    let mut initialized = false;

    for i in 0..n {
        let v = x[i];
        if v.is_nan() {
            if initialized && count as usize >= min_periods {
                out[i] = ema;
            }
            continue;
        }
        if !initialized {
            ema = v;
            initialized = true;
        } else {
            ema = alpha * v + (1.0 - alpha) * ema;
        }
        count += 1;
        if count as usize >= min_periods {
            out[i] = ema;
        }
    }
    out
}

/// Simple Moving Average. Ring buffer, O(n).
/// NaN values excluded from sum and count.
pub fn rolling_sma(x: &[f64], window: usize, min_periods: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    let mut buf = vec![f64::NAN; window];
    let mut sum = 0.0;
    let mut count = 0usize;

    for i in 0..n {
        let slot = i % window;

        // Remove evicted value
        if i >= window {
            let old = buf[slot];
            if !old.is_nan() {
                sum -= old;
                count -= 1;
            }
        }

        // Add new value
        let v = x[i];
        buf[slot] = v;
        if !v.is_nan() {
            sum += v;
            count += 1;
        }

        if i >= window - 1 && count >= min_periods {
            out[i] = sum / count as f64;
        }
    }
    out
}

/// Rolling Standard Deviation (population, ddof=0). Ring buffer, O(n).
pub fn rolling_std(x: &[f64], window: usize, min_periods: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    let mut buf = vec![f64::NAN; window];
    let mut s = 0.0;   // sum
    let mut s2 = 0.0;  // sum of squares
    let mut count = 0usize;

    for i in 0..n {
        let slot = i % window;

        if i >= window {
            let old = buf[slot];
            if !old.is_nan() {
                s -= old;
                s2 -= old * old;
                count -= 1;
            }
        }

        let v = x[i];
        buf[slot] = v;
        if !v.is_nan() {
            s += v;
            s2 += v * v;
            count += 1;
        }

        if i >= window - 1 && count >= min_periods {
            let mean = s / count as f64;
            let var = (s2 / count as f64) - mean * mean;
            out[i] = var.max(0.0).sqrt();
        }
    }
    out
}

/// Rolling Z-score: (x - rolling_mean) / rolling_std. O(n).
/// Returns NaN if std <= 1e-15 or value is NaN.
pub fn rolling_zscore(x: &[f64], window: usize, min_periods: usize) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    let mut buf = vec![f64::NAN; window];
    let mut s = 0.0;
    let mut s2 = 0.0;
    let mut count = 0usize;

    for i in 0..n {
        let slot = i % window;

        if i >= window {
            let old = buf[slot];
            if !old.is_nan() {
                s -= old;
                s2 -= old * old;
                count -= 1;
            }
        }

        let v = x[i];
        buf[slot] = v;
        if !v.is_nan() {
            s += v;
            s2 += v * v;
            count += 1;
        }

        if i >= window - 1 && count >= min_periods && !v.is_nan() {
            let mean = s / count as f64;
            let var = (s2 / count as f64) - mean * mean;
            let std = var.max(0.0).sqrt();
            if std > 1e-15 {
                out[i] = (v - mean) / std;
            }
        }
    }
    out
}

/// Rolling normalize: x / rolling_std. O(n).
/// Useful for converting raw signals to σ-units.
pub fn rolling_normalize(x: &[f64], window: usize, min_periods: usize) -> Vec<f64> {
    let std = rolling_std(x, window, min_periods);
    let n = x.len();
    let mut out = vec![f64::NAN; n];
    for i in 0..n {
        if !x[i].is_nan() && !std[i].is_nan() && std[i] > 1e-15 {
            out[i] = x[i] / std[i];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ema_basic() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let out = rolling_ema(&x, 3, 1);
        assert!(!out[0].is_nan());
        assert!((out[0] - 1.0).abs() < 1e-10); // first value = x[0]
        // EMA should be increasing
        for i in 1..5 {
            assert!(out[i] > out[i - 1]);
        }
    }

    #[test]
    fn test_ema_nan_skip() {
        let x = vec![1.0, f64::NAN, 3.0, 4.0];
        let out = rolling_ema(&x, 3, 1);
        assert!(!out[0].is_nan());
        assert!(!out[1].is_nan()); // carry forward
        assert!(!out[2].is_nan());
    }

    #[test]
    fn test_sma_basic() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let out = rolling_sma(&x, 3, 1);
        assert!((out[2] - 2.0).abs() < 1e-10); // (1+2+3)/3
        assert!((out[3] - 3.0).abs() < 1e-10); // (2+3+4)/3
        assert!((out[4] - 4.0).abs() < 1e-10); // (3+4+5)/3
    }

    #[test]
    fn test_std_constant() {
        let x = vec![5.0; 10];
        let out = rolling_std(&x, 5, 2);
        for i in 4..10 {
            assert!((out[i] - 0.0).abs() < 1e-10, "std of constant should be 0");
        }
    }

    #[test]
    fn test_zscore_basic() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let out = rolling_zscore(&x, 5, 2);
        // Last value should be positive (above mean of window)
        assert!(out[9] > 0.0);
    }

    #[test]
    fn test_normalize_basic() {
        let x = vec![0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0, 5.0];
        let out = rolling_normalize(&x, 5, 2);
        // Normalized values should be in reasonable σ range
        for i in 4..10 {
            if !out[i].is_nan() {
                assert!(out[i].abs() < 10.0, "should be within 10σ");
            }
        }
    }
}
