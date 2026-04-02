/// Cross-sectional operations — per-row across symbols.
/// Two-pass column-major for cache efficiency.

use ndarray::Array2;

/// Cross-sectional z-score. ddof=0 (population).
/// Row skipped if count < min_count or std < 1e-15.
pub fn cs_zscore(x: &Array2<f64>, min_count: usize) -> Array2<f64> {
    let (t, s) = x.dim();
    let mut result = Array2::from_elem((t, s), f64::NAN);

    // Per-row stats via column-major accumulation
    let mut row_sum = vec![0.0f64; t];
    let mut row_sum2 = vec![0.0f64; t];
    let mut row_n = vec![0usize; t];

    // Pass 1: accumulate
    for j in 0..s {
        for i in 0..t {
            let v = x[[i, j]];
            if !v.is_nan() {
                row_sum[i] += v;
                row_sum2[i] += v * v;
                row_n[i] += 1;
            }
        }
    }

    // Precompute mean and inv_std
    let mut row_mean = vec![f64::NAN; t];
    let mut row_inv_std = vec![f64::NAN; t];
    for i in 0..t {
        let n = row_n[i];
        if n >= min_count {
            let mean = row_sum[i] / n as f64;
            let var = (row_sum2[i] / n as f64) - mean * mean;
            let std = var.max(0.0).sqrt();
            if std > 1e-15 {
                row_mean[i] = mean;
                row_inv_std[i] = 1.0 / std;
            }
        }
    }

    // Pass 2: apply
    for j in 0..s {
        for i in 0..t {
            let v = x[[i, j]];
            if !v.is_nan() && !row_mean[i].is_nan() {
                result[[i, j]] = (v - row_mean[i]) * row_inv_std[i];
            }
        }
    }

    result
}

/// Cross-sectional rank. pct=true → ranks in [0,1].
pub fn cs_rank(x: &Array2<f64>, pct: bool) -> Array2<f64> {
    let (t, s) = x.dim();
    let mut result = Array2::from_elem((t, s), f64::NAN);
    let mut indices: Vec<usize> = Vec::with_capacity(s);
    let mut values: Vec<f64> = Vec::with_capacity(s);

    for i in 0..t {
        indices.clear();
        values.clear();

        for j in 0..s {
            let v = x[[i, j]];
            if !v.is_nan() {
                indices.push(j);
                values.push(v);
            }
        }

        let n = indices.len();
        if n < 2 {
            continue;
        }

        // Sort by value
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).unwrap_or(std::cmp::Ordering::Equal));

        // Assign ranks (average ties)
        let mut rank = 0usize;
        while rank < n {
            let mut end = rank + 1;
            while end < n && (values[order[end]] - values[order[rank]]).abs() < 1e-15 {
                end += 1;
            }
            let avg_rank = (rank + end - 1) as f64 / 2.0 + 1.0; // 1-based
            for k in rank..end {
                let j = indices[order[k]];
                result[[i, j]] = if pct { avg_rank / n as f64 } else { avg_rank };
            }
            rank = end;
        }
    }

    result
}

/// Factor IC: per-row Pearson correlation between signal and forward return.
/// Column-major accumulation for O(T×S).
pub fn factor_ic(signal: &Array2<f64>, fwd_return: &Array2<f64>, min_obs: usize) -> Vec<f64> {
    let (t, s) = signal.dim();
    assert_eq!(signal.dim(), fwd_return.dim());

    let mut s_sum = vec![0.0f64; t];
    let mut r_sum = vec![0.0f64; t];
    let mut sr_sum = vec![0.0f64; t];
    let mut s2_sum = vec![0.0f64; t];
    let mut r2_sum = vec![0.0f64; t];
    let mut n_vec = vec![0usize; t];

    for j in 0..s {
        for i in 0..t {
            let sv = signal[[i, j]];
            let rv = fwd_return[[i, j]];
            if !sv.is_nan() && !rv.is_nan() {
                s_sum[i] += sv;
                r_sum[i] += rv;
                sr_sum[i] += sv * rv;
                s2_sum[i] += sv * sv;
                r2_sum[i] += rv * rv;
                n_vec[i] += 1;
            }
        }
    }

    let mut ic = vec![f64::NAN; t];
    for i in 0..t {
        let n = n_vec[i];
        if n < min_obs {
            continue;
        }
        let nf = n as f64;
        let s_mean = s_sum[i] / nf;
        let r_mean = r_sum[i] / nf;
        let cov = sr_sum[i] / nf - s_mean * r_mean;
        let s_var = s2_sum[i] / nf - s_mean * s_mean;
        let r_var = r2_sum[i] / nf - r_mean * r_mean;
        let denom = (s_var * r_var).max(0.0).sqrt();
        if denom > 1e-15 {
            ic[i] = cov / denom;
        }
    }

    ic
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn test_cs_zscore() {
        let x = array![[1.0, 2.0, 3.0], [10.0, 20.0, 30.0]];
        let z = cs_zscore(&x, 2);
        // Row 0: mean=2, std=0.816... → z(1)=-1.22, z(2)=0, z(3)=1.22
        assert!(z[[0, 0]] < 0.0);
        assert!(z[[0, 1]].abs() < 1e-10);
        assert!(z[[0, 2]] > 0.0);
    }

    #[test]
    fn test_cs_rank_pct() {
        let x = array![[30.0, 10.0, 20.0]];
        let r = cs_rank(&x, true);
        assert!((r[[0, 1]] - 1.0 / 3.0).abs() < 1e-10); // lowest
        assert!((r[[0, 2]] - 2.0 / 3.0).abs() < 1e-10); // middle
        assert!((r[[0, 0]] - 3.0 / 3.0).abs() < 1e-10); // highest
    }

    #[test]
    fn test_factor_ic_perfect() {
        // Perfect positive correlation
        let sig = array![[1.0, 2.0, 3.0, 4.0, 5.0]];
        let ret = array![[10.0, 20.0, 30.0, 40.0, 50.0]];
        let ic = factor_ic(&sig, &ret, 3);
        assert!((ic[0] - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_factor_ic_negative() {
        let sig = array![[1.0, 2.0, 3.0, 4.0, 5.0]];
        let ret = array![[50.0, 40.0, 30.0, 20.0, 10.0]];
        let ic = factor_ic(&sig, &ret, 3);
        assert!((ic[0] - (-1.0)).abs() < 1e-10);
    }
}
