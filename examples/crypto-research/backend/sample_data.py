"""
sample_data.py — 합성 암호화폐 OHLCV + 펀딩비 데이터 생성기

실제 거래소 데이터 없이도 factor 연구를 시작할 수 있도록
통계적으로 현실적인 synthetic 데이터를 생성합니다.
"""

import numpy as np
import pandas as pd
from datetime import datetime, timedelta

SYMBOLS = ["BTC", "ETH", "SOL", "BNB", "ARB", "OP", "AVAX", "DOGE"]

# 심볼별 대략적인 특성 파라미터
SYMBOL_PARAMS = {
    "BTC":  {"vol": 0.03, "drift": 0.0002, "base_price": 65000},
    "ETH":  {"vol": 0.04, "drift": 0.0001, "base_price": 3500},
    "SOL":  {"vol": 0.06, "drift": 0.0003, "base_price": 150},
    "BNB":  {"vol": 0.035, "drift": 0.0001, "base_price": 400},
    "ARB":  {"vol": 0.07, "drift": 0.0002, "base_price": 1.2},
    "OP":   {"vol": 0.07, "drift": 0.0002, "base_price": 2.5},
    "AVAX": {"vol": 0.055, "drift": 0.0002, "base_price": 35},
    "DOGE": {"vol": 0.08, "drift": 0.0001, "base_price": 0.15},
}


def generate_ohlcv(symbol: str, timeframe: str = "1h", n_bars: int = 500, seed: int = None) -> pd.DataFrame:
    """
    주어진 심볼에 대한 합성 OHLCV 데이터를 생성합니다.

    Parameters
    ----------
    symbol : str
        티커 심볼 (예: "BTC")
    timeframe : str
        "1h", "4h", "1d" 중 하나
    n_bars : int
        생성할 캔들 수
    seed : int, optional
        재현성을 위한 랜덤 시드

    Returns
    -------
    pd.DataFrame
        columns: [open, high, low, close, volume, funding_rate]
        index: DatetimeIndex
    """
    if seed is not None:
        np.random.seed(seed)

    params = SYMBOL_PARAMS.get(symbol, {"vol": 0.05, "drift": 0.0001, "base_price": 100})

    freq_map = {"1h": "h", "4h": "4h", "1d": "D"}
    freq = freq_map.get(timeframe, "h")
    end = datetime(2024, 12, 31)
    index = pd.date_range(end=end, periods=n_bars, freq=freq)

    # GBM으로 close 가격 생성
    returns = np.random.normal(params["drift"], params["vol"], n_bars)
    close = params["base_price"] * np.exp(np.cumsum(returns))

    # OHLC 생성 (실제 캔들 패턴과 유사하게)
    noise = params["vol"] * 0.5
    open_ = close * np.exp(np.random.normal(0, noise * 0.3, n_bars))
    high = np.maximum(open_, close) * np.exp(np.abs(np.random.normal(0, noise, n_bars)))
    low = np.minimum(open_, close) * np.exp(-np.abs(np.random.normal(0, noise, n_bars)))

    # 거래량 (가격 변동성에 비례)
    base_vol = params["base_price"] * 1000
    volume = base_vol * np.exp(np.random.normal(0, 0.5, n_bars)) * (1 + 5 * np.abs(returns))

    # 펀딩비: 주로 -0.1% ~ 0.1% 범위, 가끔 극단값
    funding_rate = np.random.normal(0.0001, 0.0003, n_bars)
    # 가끔 극단적인 펀딩비 (롱/숏 쏠림 시뮬레이션)
    spikes = np.random.choice(n_bars, size=n_bars // 20, replace=False)
    funding_rate[spikes] = np.random.choice([-1, 1], size=len(spikes)) * np.random.uniform(0.001, 0.003, len(spikes))

    df = pd.DataFrame({
        "open": open_,
        "high": high,
        "low": low,
        "close": close,
        "volume": volume,
        "funding_rate": funding_rate,
    }, index=index)

    return df.round({"open": 4, "high": 4, "low": 4, "close": 4, "volume": 2, "funding_rate": 6})


def load_universe(timeframe: str = "1h", n_bars: int = 500) -> dict[str, pd.DataFrame]:
    """전체 유니버스의 OHLCV 데이터를 딕셔너리로 반환합니다."""
    return {sym: generate_ohlcv(sym, timeframe, n_bars, seed=hash(sym) % 9999)
            for sym in SYMBOLS}
