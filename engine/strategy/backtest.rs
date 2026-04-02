/// Backtest evaluation — trade stats, monthly breakdown, period stability.

use chrono::{DateTime, Utc, Datelike};
use serde::{Deserialize, Serialize};

use super::trades::Trade;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResult {
    pub n_trades: usize,
    pub n_long: usize,
    pub n_short: usize,
    pub mean_ret_bp: f64,
    pub mean_ret_net_bp: f64,
    pub win_rate: f64,
    pub gross_cum_bp: f64,
    pub net_cum_bp: f64,
    pub max_drawdown_bp: f64,
    pub avg_duration_bars: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyPnL {
    pub month: String,      // "2025-01"
    pub n_trades: usize,
    pub gross_bp: f64,
    pub net_bp: f64,
    pub mean_bp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodResult {
    pub label: String,
    pub n_trades: usize,
    pub mean_bp: f64,
    pub win_rate: f64,
    pub net_cum_bp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectionStats {
    pub n: usize,
    pub mean_bp: f64,
    pub win_rate: f64,
    pub net_cum_bp: f64,
}

/// Compute backtest statistics for a set of trades.
pub fn trade_stats(trades: &[Trade], fee_bp: f64) -> BacktestResult {
    let n = trades.len();
    if n == 0 {
        return BacktestResult {
            n_trades: 0, n_long: 0, n_short: 0,
            mean_ret_bp: 0.0, mean_ret_net_bp: 0.0, win_rate: 0.0,
            gross_cum_bp: 0.0, net_cum_bp: 0.0, max_drawdown_bp: 0.0,
            avg_duration_bars: 0.0,
        };
    }

    let rets: Vec<f64> = trades.iter().map(|t| t.ret_bp).collect();
    let rets_net: Vec<f64> = rets.iter().map(|r| r - fee_bp).collect();
    let n_long = trades.iter().filter(|t| t.direction > 0).count();
    let n_short = trades.iter().filter(|t| t.direction < 0).count();
    let wins = rets.iter().filter(|r| **r > 0.0).count();
    let durations: Vec<f64> = trades.iter().map(|t| (t.exit - t.enter + 1) as f64).collect();

    let gross_cum: f64 = rets.iter().sum();
    let net_cum: f64 = rets_net.iter().sum();
    let mean_ret = gross_cum / n as f64;
    let mean_net = net_cum / n as f64;

    // Max drawdown on net cumulative
    let mdd = max_drawdown(&rets_net);

    BacktestResult {
        n_trades: n,
        n_long,
        n_short,
        mean_ret_bp: mean_ret,
        mean_ret_net_bp: mean_net,
        win_rate: wins as f64 / n as f64,
        gross_cum_bp: gross_cum,
        net_cum_bp: net_cum,
        max_drawdown_bp: mdd,
        avg_duration_bars: durations.iter().sum::<f64>() / n as f64,
    }
}

/// Compute direction-specific stats.
pub fn direction_stats(trades: &[Trade], fee_bp: f64, dir: i32) -> DirectionStats {
    let filtered: Vec<&Trade> = trades.iter().filter(|t| t.direction == dir).collect();
    let n = filtered.len();
    if n == 0 {
        return DirectionStats { n: 0, mean_bp: 0.0, win_rate: 0.0, net_cum_bp: 0.0 };
    }
    let rets: Vec<f64> = filtered.iter().map(|t| t.ret_bp).collect();
    let net: Vec<f64> = rets.iter().map(|r| r - fee_bp).collect();
    let wins = rets.iter().filter(|r| **r > 0.0).count();
    DirectionStats {
        n,
        mean_bp: rets.iter().sum::<f64>() / n as f64,
        win_rate: wins as f64 / n as f64,
        net_cum_bp: net.iter().sum(),
    }
}

/// Monthly PnL breakdown.
pub fn monthly_breakdown(
    trades: &[Trade],
    timestamps: &[DateTime<Utc>],
    fee_bp: f64,
) -> Vec<MonthlyPnL> {
    use std::collections::BTreeMap;

    let mut months: BTreeMap<String, Vec<f64>> = BTreeMap::new();

    for t in trades {
        if t.enter >= timestamps.len() {
            continue;
        }
        let dt = timestamps[t.enter];
        let key = format!("{:04}-{:02}", dt.year(), dt.month());
        months.entry(key).or_default().push(t.ret_bp);
    }

    months
        .into_iter()
        .map(|(month, rets)| {
            let n = rets.len();
            let gross: f64 = rets.iter().sum();
            let net = gross - fee_bp * n as f64;
            MonthlyPnL {
                month,
                n_trades: n,
                gross_bp: gross,
                net_bp: net,
                mean_bp: gross / n as f64,
            }
        })
        .collect()
}

/// Period stability analysis.
pub fn period_stability(
    trades: &[Trade],
    timestamps: &[DateTime<Utc>],
    fee_bp: f64,
    periods: &[(String, DateTime<Utc>, DateTime<Utc>)],
) -> Vec<PeriodResult> {
    periods
        .iter()
        .map(|(label, start, end)| {
            let filtered: Vec<&Trade> = trades
                .iter()
                .filter(|t| {
                    t.enter < timestamps.len() && {
                        let dt = timestamps[t.enter];
                        dt >= *start && dt < *end
                    }
                })
                .collect();

            let n = filtered.len();
            if n == 0 {
                return PeriodResult {
                    label: label.clone(), n_trades: 0,
                    mean_bp: 0.0, win_rate: 0.0, net_cum_bp: 0.0,
                };
            }

            let rets: Vec<f64> = filtered.iter().map(|t| t.ret_bp).collect();
            let net: Vec<f64> = rets.iter().map(|r| r - fee_bp).collect();
            let wins = rets.iter().filter(|r| **r > 0.0).count();

            PeriodResult {
                label: label.clone(),
                n_trades: n,
                mean_bp: rets.iter().sum::<f64>() / n as f64,
                win_rate: wins as f64 / n as f64,
                net_cum_bp: net.iter().sum(),
            }
        })
        .collect()
}

/// Max drawdown from a return series (not cumulative — will compute internally).
fn max_drawdown(rets: &[f64]) -> f64 {
    let mut cum = 0.0f64;
    let mut peak = 0.0f64;
    let mut mdd = 0.0f64;

    for &r in rets {
        cum += r;
        if cum > peak {
            peak = cum;
        }
        let dd = cum - peak;
        if dd < mdd {
            mdd = dd;
        }
    }
    mdd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trades() -> Vec<Trade> {
        vec![
            Trade { enter: 10, exit: 20, direction: 1, ret_bp: 50.0 },
            Trade { enter: 25, exit: 35, direction: -1, ret_bp: 30.0 },
            Trade { enter: 40, exit: 50, direction: 1, ret_bp: -20.0 },
            Trade { enter: 55, exit: 65, direction: -1, ret_bp: 80.0 },
            Trade { enter: 70, exit: 80, direction: 1, ret_bp: -40.0 },
        ]
    }

    #[test]
    fn test_trade_stats() {
        let trades = make_trades();
        let stats = trade_stats(&trades, 9.0);
        assert_eq!(stats.n_trades, 5);
        assert_eq!(stats.n_long, 3);
        assert_eq!(stats.n_short, 2);
        assert!((stats.gross_cum_bp - 100.0).abs() < 1e-10);
        assert!((stats.net_cum_bp - 55.0).abs() < 1e-10); // 100 - 5*9
        assert!((stats.win_rate - 0.6).abs() < 1e-10); // 3/5
    }

    #[test]
    fn test_direction_stats() {
        let trades = make_trades();
        let long = direction_stats(&trades, 9.0, 1);
        assert_eq!(long.n, 3);
        let short = direction_stats(&trades, 9.0, -1);
        assert_eq!(short.n, 2);
        assert!(short.mean_bp > long.mean_bp); // shorts did better
    }

    #[test]
    fn test_max_drawdown() {
        let rets = vec![10.0, -5.0, -15.0, 20.0, -3.0];
        let mdd = max_drawdown(&rets);
        // cum: 10, 5, -10, 10, 7. peak: 10,10,10,10,10. dd: 0,-5,-20,0,-3
        assert!((mdd - (-20.0)).abs() < 1e-10);
    }
}
