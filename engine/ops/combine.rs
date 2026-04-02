/// Signal combination utilities — element-wise ops on Vec<f64>.

/// Element-wise multiply. NaN if either is NaN.
pub fn element_mul(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| if x.is_nan() || y.is_nan() { f64::NAN } else { x * y })
        .collect()
}

/// Check if two signals have the same sign. NaN → false.
pub fn element_sign_match(a: &[f64], b: &[f64]) -> Vec<bool> {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| {
            !x.is_nan() && !y.is_nan() && x.signum() == y.signum() && x != 0.0 && y != 0.0
        })
        .collect()
}

/// Clamp each element to [lo, hi]. NaN preserved.
pub fn clamp_vec(x: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    x.iter()
        .map(|&v| if v.is_nan() { f64::NAN } else { v.clamp(lo, hi) })
        .collect()
}

/// Apply boolean mask: set to NaN where mask is false.
pub fn nan_mask(x: &[f64], mask: &[bool]) -> Vec<f64> {
    x.iter()
        .zip(mask.iter())
        .map(|(&v, &m)| if m { v } else { f64::NAN })
        .collect()
}

/// Replace NaN with a default value.
pub fn fill_nan(x: &[f64], default: f64) -> Vec<f64> {
    x.iter()
        .map(|&v| if v.is_nan() { default } else { v })
        .collect()
}

/// Count non-NaN values.
pub fn count_valid(x: &[f64]) -> usize {
    x.iter().filter(|v| !v.is_nan()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_mul() {
        let a = vec![1.0, 2.0, f64::NAN, 4.0];
        let b = vec![10.0, f64::NAN, 30.0, 40.0];
        let out = element_mul(&a, &b);
        assert!((out[0] - 10.0).abs() < 1e-10);
        assert!(out[1].is_nan());
        assert!(out[2].is_nan());
        assert!((out[3] - 160.0).abs() < 1e-10);
    }

    #[test]
    fn test_sign_match() {
        let a = vec![1.0, -1.0, 1.0, -1.0, 0.0];
        let b = vec![2.0, -2.0, -2.0, 2.0, 1.0];
        let out = element_sign_match(&a, &b);
        assert!(out[0]);   // both positive
        assert!(out[1]);   // both negative
        assert!(!out[2]);  // opposite
        assert!(!out[3]);  // opposite
        assert!(!out[4]);  // zero
    }

    #[test]
    fn test_nan_mask() {
        let x = vec![1.0, 2.0, 3.0, 4.0];
        let mask = vec![true, false, true, false];
        let out = nan_mask(&x, &mask);
        assert!((out[0] - 1.0).abs() < 1e-10);
        assert!(out[1].is_nan());
        assert!((out[2] - 3.0).abs() < 1e-10);
        assert!(out[3].is_nan());
    }
}
