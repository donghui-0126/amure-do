/// External data fetchers — Yahoo Finance etc.

use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct YahooQuery {
    pub symbol: String,
    pub period1: Option<String>,  // "2024-01-01"
    pub period2: Option<String>,  // "2026-03-01"
    pub interval: Option<String>, // "1d", "1h"
}

#[derive(Serialize, Clone)]
pub struct OhlcvBar {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

pub async fn fetch_yahoo(
    Json(req): Json<YahooQuery>,
) -> Json<serde_json::Value> {
    let symbol = &req.symbol;
    let interval = req.interval.as_deref().unwrap_or("1d");

    // Use Yahoo Finance v8 API
    let period1 = req.period1.as_deref().unwrap_or("2024-01-01");
    let period2 = req.period2.as_deref().unwrap_or("2026-03-31");

    let p1 = date_to_unix(period1).unwrap_or(1704067200);
    let p2 = date_to_unix(period2).unwrap_or(1743379200);

    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?period1={}&period2={}&interval={}",
        symbol, p1, p2, interval
    );

    let client = reqwest::Client::new();
    let resp = match client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return Json(serde_json::json!({"error": format!("Request failed: {}", e)})),
    };

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => return Json(serde_json::json!({"error": format!("Parse failed: {}", e)})),
    };

    // Parse Yahoo response
    let result = &body["chart"]["result"];
    if result.is_null() || !result.is_array() || result.as_array().unwrap().is_empty() {
        return Json(serde_json::json!({"error": "No data from Yahoo", "raw": body}));
    }

    let r0 = &result[0];
    let timestamps = r0["timestamp"].as_array();
    let quotes = &r0["indicators"]["quote"][0];

    if timestamps.is_none() {
        return Json(serde_json::json!({"error": "No timestamps in response"}));
    }

    let ts = timestamps.unwrap();
    let opens = quotes["open"].as_array();
    let highs = quotes["high"].as_array();
    let lows = quotes["low"].as_array();
    let closes = quotes["close"].as_array();
    let volumes = quotes["volume"].as_array();

    if opens.is_none() || closes.is_none() {
        return Json(serde_json::json!({"error": "Missing OHLCV data"}));
    }

    let mut bars: Vec<OhlcvBar> = Vec::new();
    let n = ts.len();
    for i in 0..n {
        let close = closes.unwrap()[i].as_f64().unwrap_or(f64::NAN);
        if close.is_nan() { continue; }

        bars.push(OhlcvBar {
            timestamp: ts[i].as_i64().unwrap_or(0),
            open: opens.unwrap()[i].as_f64().unwrap_or(f64::NAN),
            high: highs.unwrap()[i].as_f64().unwrap_or(f64::NAN),
            low: lows.unwrap()[i].as_f64().unwrap_or(f64::NAN),
            close,
            volume: volumes.map(|v| v[i].as_f64().unwrap_or(0.0)).unwrap_or(0.0),
        });
    }

    // Compute basic stats
    let closes_vec: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let n_bars = closes_vec.len();

    let mut analysis = serde_json::json!({});
    if n_bars > 1 {
        // Log returns
        let rets: Vec<f64> = closes_vec.windows(2)
            .map(|w| (w[1] / w[0]).ln() * 10000.0)
            .collect();
        let mean_ret = rets.iter().sum::<f64>() / rets.len() as f64;
        let var: f64 = rets.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>() / rets.len() as f64;

        // Momentum
        let mom = crate::ops::features::spread_adjusted_slope(&closes_vec, 24.min(n_bars/4).max(2), 120.min(n_bars/2).max(5));
        let valid_mom: Vec<f64> = mom.iter().filter(|v| !v.is_nan()).copied().collect();

        analysis = serde_json::json!({
            "n_bars": n_bars,
            "first_close": closes_vec.first(),
            "last_close": closes_vec.last(),
            "total_return_bp": ((closes_vec.last().unwrap() / closes_vec.first().unwrap()).ln() * 10000.0),
            "daily_mean_ret_bp": mean_ret,
            "daily_std_bp": var.sqrt(),
            "annualized_vol_pct": var.sqrt() * (252.0f64).sqrt() / 100.0,
            "momentum_last": valid_mom.last().copied(),
            "momentum_mean": if valid_mom.is_empty() { None } else { Some(valid_mom.iter().sum::<f64>() / valid_mom.len() as f64) },
        });
    }

    Json(serde_json::json!({
        "symbol": symbol,
        "interval": interval,
        "bars": bars.len(),
        "data": bars,
        "analysis": analysis,
    }))
}

/// Fetch Yahoo data and save as Arrow file for Julia.
pub async fn fetch_and_save(
    Json(req): Json<YahooQuery>,
) -> Json<serde_json::Value> {
    // First fetch
    let fetch_result = fetch_yahoo(Json(req)).await;
    let data = fetch_result.0;

    if data.get("error").is_some() {
        return Json(data);
    }

    let symbol = data["symbol"].as_str().unwrap_or("UNKNOWN");
    let bars = match data["data"].as_array() {
        Some(b) => b,
        None => return Json(serde_json::json!({"error": "No data to save"})),
    };

    // Save as CSV (Julia can read this easily)
    let dir = std::path::PathBuf::from("data/feature_store/external");
    let _ = std::fs::create_dir_all(&dir);
    let csv_path = dir.join(format!("{}.csv", symbol));

    let mut csv_content = String::from("timestamp,open,high,low,close,volume\n");
    for bar in bars {
        csv_content.push_str(&format!(
            "{},{},{},{},{},{}\n",
            bar["timestamp"].as_i64().unwrap_or(0),
            bar["open"].as_f64().unwrap_or(f64::NAN),
            bar["high"].as_f64().unwrap_or(f64::NAN),
            bar["low"].as_f64().unwrap_or(f64::NAN),
            bar["close"].as_f64().unwrap_or(f64::NAN),
            bar["volume"].as_f64().unwrap_or(0.0),
        ));
    }

    if let Err(e) = std::fs::write(&csv_path, &csv_content) {
        return Json(serde_json::json!({"error": format!("Failed to save: {}", e)}));
    }

    Json(serde_json::json!({
        "status": "saved",
        "symbol": symbol,
        "path": csv_path.to_string_lossy(),
        "bars": bars.len(),
        "analysis": data.get("analysis"),
    }))
}

fn date_to_unix(date_str: &str) -> Option<i64> {
    let dt = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    Some(dt.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}
