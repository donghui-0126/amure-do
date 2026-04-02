/// Generic signal state machine.
/// States and transitions are configurable — the engine just executes.

use serde::{Deserialize, Serialize};

/// Exit mode determines when an active position is closed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ExitMode {
    /// Exit when primary signal crosses zero.
    SignChange,
    /// Exit when |primary signal| drops below entry threshold.
    PrimaryThreshold,
    /// Exit when secondary signal crosses zero.
    SecondaryFlip,
    /// Exit on PrimaryThreshold OR SecondaryFlip (whichever first).
    PrimaryOrSecondary,
}

/// Configuration for the signal FSM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalConfig {
    /// Primary signal threshold for entry (e.g. |p_norm| >= this).
    pub primary_threshold: f64,
    /// Secondary signal threshold for entry (e.g. |o_norm| >= this).
    pub secondary_threshold: f64,
    /// How to determine exit.
    pub exit_mode: ExitMode,
}

impl Default for SignalConfig {
    fn default() -> Self {
        Self {
            primary_threshold: 0.1,
            secondary_threshold: 0.25,
            exit_mode: ExitMode::PrimaryOrSecondary,
        }
    }
}

/// Build a signal vector from primary and secondary feature vectors.
///
/// - Entry long:  primary >= threshold AND secondary >= sec_threshold
/// - Entry short: primary <= -threshold AND secondary <= -sec_threshold
/// - Exit: determined by exit_mode
/// - Output: primary value when in position, 0.0 when flat.
/// - On exit: immediate reverse entry check.
///
/// O(n), single pass, deterministic.
pub fn build_signal(
    primary: &[f64],
    secondary: &[f64],
    config: &SignalConfig,
) -> Vec<f64> {
    let n = primary.len();
    assert_eq!(n, secondary.len());

    let mut out = vec![0.0f64; n];
    let mut in_pos = false;
    let mut pos_dir: i32 = 0;
    let pt = config.primary_threshold;
    let st = config.secondary_threshold;

    for i in 0..n {
        let pn = primary[i];
        let sn = secondary[i];

        if !in_pos {
            // Entry check
            if !pn.is_nan() && !sn.is_nan() {
                if pn >= pt && sn >= st {
                    in_pos = true;
                    pos_dir = 1;
                    out[i] = pn;
                } else if pn <= -pt && sn <= -st {
                    in_pos = true;
                    pos_dir = -1;
                    out[i] = pn;
                }
            }
        } else {
            let should_exit = match config.exit_mode {
                ExitMode::SignChange => {
                    pn.is_nan()
                        || (pos_dir > 0 && pn <= 0.0)
                        || (pos_dir < 0 && pn >= 0.0)
                }
                ExitMode::PrimaryThreshold => {
                    pn.is_nan()
                        || (pos_dir > 0 && pn < pt)
                        || (pos_dir < 0 && pn > -pt)
                }
                ExitMode::SecondaryFlip => {
                    sn.is_nan()
                        || (pos_dir > 0 && sn < 0.0)
                        || (pos_dir < 0 && sn > 0.0)
                }
                ExitMode::PrimaryOrSecondary => {
                    let p_exit = pn.is_nan()
                        || (pos_dir > 0 && pn < pt)
                        || (pos_dir < 0 && pn > -pt);
                    let s_exit = sn.is_nan()
                        || (pos_dir > 0 && sn < 0.0)
                        || (pos_dir < 0 && sn > 0.0);
                    p_exit || s_exit
                }
            };

            if should_exit {
                in_pos = false;
                pos_dir = 0;
                // Immediate reverse entry check
                if !pn.is_nan() && !sn.is_nan() {
                    if pn >= pt && sn >= st {
                        in_pos = true;
                        pos_dir = 1;
                        out[i] = pn;
                    } else if pn <= -pt && sn <= -st {
                        in_pos = true;
                        pos_dir = -1;
                        out[i] = pn;
                    }
                }
            } else {
                out[i] = pn;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_long_entry_exit() {
        let primary =   vec![0.0, 0.5, 1.5, 1.2, 0.8, 0.05, -0.5];
        let secondary = vec![0.0, 0.3, 0.5, 0.4, 0.3, 0.1,  -0.3];
        let config = SignalConfig {
            primary_threshold: 1.0,
            secondary_threshold: 0.25,
            exit_mode: ExitMode::PrimaryOrSecondary,
        };
        let sig = build_signal(&primary, &secondary, &config);

        assert_eq!(sig[0], 0.0); // flat
        assert_eq!(sig[1], 0.0); // below threshold
        assert!(sig[2] > 0.0);  // entry: 1.5 >= 1.0, 0.5 >= 0.25
        assert!(sig[3] > 0.0);  // still in
        assert_eq!(sig[5], 0.0); // exit: 0.05 < 1.0
    }

    #[test]
    fn test_short_entry() {
        let primary =   vec![0.0, -1.5, -1.2, -0.5, 0.0];
        let secondary = vec![0.0, -0.5, -0.3,  0.1, 0.0];
        let config = SignalConfig {
            primary_threshold: 1.0,
            secondary_threshold: 0.25,
            exit_mode: ExitMode::SecondaryFlip,
        };
        let sig = build_signal(&primary, &secondary, &config);

        assert_eq!(sig[0], 0.0);
        assert!(sig[1] < 0.0);  // short entry
        assert!(sig[2] < 0.0);  // still in
        assert_eq!(sig[3], 0.0); // exit: secondary flipped to +0.1
    }

    #[test]
    fn test_immediate_reverse() {
        let primary =   vec![0.0, 1.5, 1.0, -1.5, -1.0];
        let secondary = vec![0.0, 0.5, 0.3, -0.5, -0.3];
        let config = SignalConfig {
            primary_threshold: 1.0,
            secondary_threshold: 0.25,
            exit_mode: ExitMode::SignChange,
        };
        let sig = build_signal(&primary, &secondary, &config);

        assert!(sig[1] > 0.0);  // long
        assert!(sig[3] < 0.0);  // sign change → exit long, immediate short entry
    }

    #[test]
    fn test_nan_handling() {
        let primary =   vec![1.5, f64::NAN, 1.2, 0.8];
        let secondary = vec![0.5, 0.3,      0.4, 0.2];
        let config = SignalConfig::default();
        let sig = build_signal(&primary, &secondary, &config);

        assert!(sig[0] > 0.0);  // entry
        assert_eq!(sig[1], 0.0); // NaN → exit
    }
}
