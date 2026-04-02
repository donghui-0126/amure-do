/// Trade extraction from signal vector.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub enter: usize,
    pub exit: usize,
    pub direction: i32,   // +1 long, -1 short
    pub ret_bp: f64,      // basis points: dir * ln(p_exit/p_enter) * 10000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeConfig {
    /// Bars to delay entry (execution lag). 0 = same bar.
    pub entry_delay: usize,
    /// Minimum bars to hold before exit takes effect. 0 = no minimum.
    pub min_hold: usize,
}

impl Default for TradeConfig {
    fn default() -> Self {
        Self {
            entry_delay: 1,
            min_hold: 0,
        }
    }
}

/// Extract trades from a signal vector.
///
/// Contiguous same-sign non-zero runs → one trade.
/// Applies entry_delay and min_hold.
/// O(n) single pass.
pub fn extract_trades(
    signal: &[f64],
    price: &[f64],
    config: &TradeConfig,
) -> Vec<Trade> {
    let n = signal.len();
    assert_eq!(n, price.len());
    let mut trades = Vec::new();
    let mut i = 0;

    while i < n {
        let s = signal[i];
        if s != 0.0 && !s.is_nan() {
            let dir = if s > 0.0 { 1i32 } else { -1i32 };
            let run_start = i;
            let mut j = i + 1;
            while j < n && signal[j] != 0.0 && !signal[j].is_nan() && signal[j].signum() as i32 == dir {
                j += 1;
            }
            let run_end = j - 1;

            // Apply entry delay
            let ae = (run_start + config.entry_delay).min(n - 1);
            // Apply exit delay + min hold
            let signal_exit = (run_end + config.entry_delay).min(n - 1);
            let ax = signal_exit.max(ae + config.min_hold).min(n - 1);

            let p_enter = price[ae];
            let p_exit = price[ax];

            if !p_enter.is_nan() && p_enter > 0.0 && ae < ax && !p_exit.is_nan() {
                let ret_bp = dir as f64 * (p_exit / p_enter).ln() * 10000.0;
                trades.push(Trade {
                    enter: ae,
                    exit: ax,
                    direction: dir,
                    ret_bp,
                });
            }

            i = j;
        } else {
            i += 1;
        }
    }

    trades
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_trade() {
        let signal = vec![0.0, 0.0, 1.5, 1.2, 0.8, 0.0, 0.0];
        let price =  vec![100.0, 100.0, 101.0, 102.0, 103.0, 104.0, 104.0];
        let config = TradeConfig { entry_delay: 1, min_hold: 0 };
        let trades = extract_trades(&signal, &price, &config);

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].direction, 1);
        assert_eq!(trades[0].enter, 3);  // run starts at 2, +1 delay = 3
        assert_eq!(trades[0].exit, 5);   // run ends at 4, +1 delay = 5
        assert!(trades[0].ret_bp > 0.0); // price went up
    }

    #[test]
    fn test_short_trade() {
        let signal = vec![0.0, -1.5, -1.2, 0.0, 0.0];
        let price =  vec![100.0, 100.0, 98.0, 97.0, 97.0];
        let config = TradeConfig { entry_delay: 1, min_hold: 0 };
        let trades = extract_trades(&signal, &price, &config);

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].direction, -1);
        assert!(trades[0].ret_bp > 0.0); // short + price down = positive
    }

    #[test]
    fn test_min_hold() {
        // Signal only active for 2 bars, but min_hold=5
        let signal = vec![0.0, 1.5, 1.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let price =  vec![100.0, 100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0];
        let config = TradeConfig { entry_delay: 1, min_hold: 5 };
        let trades = extract_trades(&signal, &price, &config);

        assert_eq!(trades.len(), 1);
        // enter = 1+1=2, signal_exit = 2+1=3, but min_hold forces exit to 2+5=7
        assert_eq!(trades[0].enter, 2);
        assert!(trades[0].exit >= 7);
    }

    #[test]
    fn test_multiple_trades() {
        let signal = vec![0.0, 1.0, 1.0, 0.0, -1.0, -1.0, 0.0];
        let price =  vec![100.0, 101.0, 102.0, 103.0, 102.0, 101.0, 100.0];
        let config = TradeConfig { entry_delay: 0, min_hold: 0 };
        let trades = extract_trades(&signal, &price, &config);

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].direction, 1);
        assert_eq!(trades[1].direction, -1);
    }

    #[test]
    fn test_nan_price_skip() {
        let signal = vec![1.5, 1.2, 0.0];
        let price =  vec![f64::NAN, 100.0, 101.0];
        let config = TradeConfig { entry_delay: 0, min_hold: 0 };
        let trades = extract_trades(&signal, &price, &config);
        // enter price is NaN → skip
        assert!(trades.is_empty() || !trades[0].ret_bp.is_nan());
    }
}
