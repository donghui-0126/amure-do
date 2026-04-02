"""
server.py — amure-do HTTP backend for crypto factor research

Flask server on port 5000. amure-do calls:
  GET  /health         → {"status": "ok"}
  POST /exec           → {"code": "...", "timeout_secs": 60}
                       ← {"status": "ok"|"error", "output": "..."}

실험 코드는 pre-loaded namespace에서 실행됩니다.
아래 헬퍼 함수들을 실험 코드 안에서 직접 호출할 수 있습니다:
  - load_ohlcv(symbol, timeframe)
  - compute_factor(df, name)
  - cross_sectional_rank(factors)
  - compute_ic(factor, returns, method)
  - factor_summary(factor, returns)
"""

import io
import sys
import traceback
from contextlib import redirect_stdout, redirect_stderr

import numpy as np
import pandas as pd
from flask import Flask, jsonify, request
from scipy import stats

from sample_data import generate_ohlcv, load_universe, SYMBOLS

app = Flask(__name__)

# ---------------------------------------------------------------------------
# 헬퍼 함수 — 실험 코드에서 직접 호출 가능
# ---------------------------------------------------------------------------

def load_ohlcv(symbol: str, timeframe: str = "1h", n_bars: int = 500) -> pd.DataFrame:
    """
    심볼의 OHLCV + funding_rate 데이터를 로드합니다.

    Example
    -------
    df = load_ohlcv("BTC", "1h")
    print(df.tail())
    """
    return generate_ohlcv(symbol, timeframe, n_bars, seed=hash(symbol) % 9999)


def compute_factor(df: pd.DataFrame, name: str, window: int = 24) -> pd.Series:
    """
    단일 심볼 DataFrame으로부터 팩터 시그널을 계산합니다.

    Parameters
    ----------
    df : pd.DataFrame
        load_ohlcv() 반환값
    name : str
        "momentum"       — window 기간 수익률
        "volatility"     — 실현 변동성 (낮을수록 high signal)
        "mean_reversion" — z-score 기반 평균회귀 시그널
        "funding"        — 펀딩비 역방향 시그널 (고펀딩 → 숏 시그널)
        "volume_surge"   — 거래량 급증 시그널
    window : int
        룩백 기간 (시간 단위)

    Returns
    -------
    pd.Series
        팩터 값 (index = DatetimeIndex)
    """
    close = df["close"]
    returns = close.pct_change()

    if name == "momentum":
        return close.pct_change(window)

    elif name == "volatility":
        # 낮은 변동성 = 높은 팩터 (low-vol anomaly)
        return -returns.rolling(window).std()

    elif name == "mean_reversion":
        roll_mean = close.rolling(window).mean()
        roll_std = close.rolling(window).std()
        z = (close - roll_mean) / roll_std.replace(0, np.nan)
        return -z  # 가격이 높으면 negative signal

    elif name == "funding":
        # 극단적 펀딩비 이후 반전 시그널
        fr = df["funding_rate"]
        return -fr.rolling(window // 4).mean()  # 역방향

    elif name == "volume_surge":
        vol_ratio = df["volume"] / df["volume"].rolling(window).mean()
        return (vol_ratio - 1) * np.sign(returns)

    else:
        raise ValueError(f"Unknown factor: {name}. Choose from: momentum, volatility, mean_reversion, funding, volume_surge")


def cross_sectional_rank(factors: dict[str, pd.Series]) -> pd.DataFrame:
    """
    여러 심볼의 팩터를 cross-sectional rank로 정규화합니다.

    Parameters
    ----------
    factors : dict[str, pd.Series]
        {"BTC": series, "ETH": series, ...}

    Returns
    -------
    pd.DataFrame
        columns = symbols, values = rank (0~1 normalized)
    """
    df = pd.DataFrame(factors)
    # 각 시점에서 rank → percentile 변환
    ranked = df.rank(axis=1, pct=True)
    return ranked


def compute_ic(factor: pd.Series, returns: pd.Series, method: str = "spearman") -> float:
    """
    팩터와 forward return 간의 Information Coefficient를 계산합니다.

    Parameters
    ----------
    factor : pd.Series
        팩터 시그널 (t 시점)
    returns : pd.Series
        미래 수익률 (t+h 시점, factor와 동일 index)
    method : str
        "spearman" (rank IC) 또는 "pearson"

    Returns
    -------
    float
        IC 값 (-1 ~ 1)
    """
    # NaN 제거 후 공통 index
    valid = factor.notna() & returns.notna()
    f = factor[valid]
    r = returns[valid]

    if len(f) < 10:
        return np.nan

    if method == "spearman":
        corr, _ = stats.spearmanr(f, r)
    elif method == "pearson":
        corr, _ = stats.pearsonr(f, r)
    else:
        raise ValueError(f"method must be 'spearman' or 'pearson', got '{method}'")

    return float(corr)


def factor_summary(factor: pd.Series, returns: pd.Series, horizons: list = None) -> dict:
    """
    팩터 성과 요약 통계를 계산합니다.

    Returns
    -------
    dict with keys:
        mean_ic, ic_std, ir (information ratio), hit_ratio, t_stat
        + per-horizon ICs if horizons provided
    """
    if horizons is None:
        horizons = [1, 4, 8, 24]

    results = {}

    # rolling IC 시계열 계산 (window=50)
    ic_series = []
    for i in range(50, len(factor)):
        f_slice = factor.iloc[i - 50:i]
        r_slice = returns.iloc[i - 50:i]
        ic = compute_ic(f_slice, r_slice)
        ic_series.append(ic)

    ic_arr = np.array([x for x in ic_series if not np.isnan(x)])

    if len(ic_arr) == 0:
        return {"error": "IC 계산에 충분한 데이터가 없습니다"}

    mean_ic = float(np.mean(ic_arr))
    ic_std = float(np.std(ic_arr))
    ir = mean_ic / ic_std if ic_std > 0 else 0.0
    hit_ratio = float(np.mean(ic_arr > 0))
    t_stat = float(mean_ic / (ic_std / np.sqrt(len(ic_arr)))) if ic_std > 0 else 0.0

    results["mean_ic"] = round(mean_ic, 4)
    results["ic_std"] = round(ic_std, 4)
    results["ir"] = round(ir, 4)
    results["hit_ratio"] = round(hit_ratio, 4)
    results["t_stat"] = round(t_stat, 4)
    results["n_obs"] = len(ic_arr)

    # 다중 horizon IC
    for h in horizons:
        fwd = returns.shift(-h)
        ic_h = compute_ic(factor, fwd)
        results[f"ic_h{h}"] = round(ic_h, 4) if not np.isnan(ic_h) else None

    return results


# ---------------------------------------------------------------------------
# 실행 namespace — 실험 코드가 접근 가능한 globals
# ---------------------------------------------------------------------------

EXEC_NAMESPACE = {
    "np": np,
    "pd": pd,
    "stats": stats,
    # 헬퍼 함수
    "load_ohlcv": load_ohlcv,
    "compute_factor": compute_factor,
    "cross_sectional_rank": cross_sectional_rank,
    "compute_ic": compute_ic,
    "factor_summary": factor_summary,
    "load_universe": load_universe,
    "SYMBOLS": SYMBOLS,
}

# ---------------------------------------------------------------------------
# Flask 엔드포인트
# ---------------------------------------------------------------------------

@app.get("/health")
def health():
    """amure-do가 백엔드 상태를 확인하는 엔드포인트."""
    return jsonify({"status": "ok", "backend": "crypto-research-python"})


@app.post("/exec")
def exec_code():
    """
    amure-do 실험 코드를 실행합니다.

    Request body (JSON):
      {
        "code": "df = load_ohlcv('BTC')\nprint(df.tail())",
        "timeout_secs": 60
      }

    Response:
      {"status": "ok",  "output": "...stdout..."}
      {"status": "error", "output": "...traceback..."}
    """
    body = request.get_json(force=True, silent=True) or {}
    code = body.get("code", "")

    if not code.strip():
        return jsonify({"status": "error", "output": "No code provided"}), 400

    stdout_buf = io.StringIO()
    stderr_buf = io.StringIO()
    local_ns = dict(EXEC_NAMESPACE)  # fresh local scope per call

    try:
        with redirect_stdout(stdout_buf), redirect_stderr(stderr_buf):
            exec(compile(code, "<experiment>", "exec"), local_ns)  # noqa: S102

        output = stdout_buf.getvalue()
        err = stderr_buf.getvalue()
        if err:
            output += f"\n[stderr]\n{err}"

        return jsonify({"status": "ok", "output": output or "(no output)"})

    except Exception:
        tb = traceback.format_exc()
        return jsonify({"status": "error", "output": tb})


# ---------------------------------------------------------------------------
# 진입점
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    print("=== Crypto Research Backend ===")
    print("amure-do HTTP backend on http://localhost:5000")
    print("Available helpers: load_ohlcv, compute_factor, cross_sectional_rank, compute_ic, factor_summary")
    print()
    app.run(host="0.0.0.0", port=5000, debug=False)
