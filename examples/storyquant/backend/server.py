"""
server.py — amure-do HTTP backend for StoryQuant narrative intelligence

Flask server on port 5050. amure-do calls:
  GET  /health         -> {"status": "ok"}
  POST /exec           -> {"code": "...", "timeout_secs": 60}
                       <- {"status": "ok"|"error", "output": "..."}

Available helpers in experiment code:
  Data:
    crawl_news(hours=6)           -> DataFrame of recent news articles
    fetch_prices(tickers, period) -> DataFrame of OHLCV prices
    get_price(ticker, hours=72)   -> DataFrame for single ticker

  Events:
    detect_events(price_df)       -> DataFrame of price events (surge/crash/volume)
    get_recent_events(limit=20)   -> List of recent events from graph

  Graph (amure-db):
    graph_search(query, top_k=10)     -> RAG search results
    graph_summary()                    -> Node/edge counts
    get_narratives()                   -> Active narrative Claims
    get_evidence(market=None)          -> Recent Evidence nodes
    create_claim(statement, keywords)  -> Create Claim node
    attribute_events()                 -> Run RAG attribution on unattributed events

  Analysis:
    sentiment(text)                    -> (sentiment, score)
    compute_factor(df, name, window)   -> Factor signal series
    compute_ic(factor, returns)        -> Information coefficient
    factor_summary(factor, returns)    -> Full factor stats
    narrative_returns(narrative_id)     -> Returns linked to a narrative

  Pipeline:
    run_pipeline()                     -> Full pipeline (crawl -> graph -> attribute)
    discover_narratives()              -> Auto-discover narratives from evidence
"""

import io
import os
import sys
import traceback
from contextlib import redirect_stdout, redirect_stderr

# Add StoryQuant to path
STORYQUANT_ROOT = os.environ.get(
    "STORYQUANT_ROOT",
    os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", "..", "StoryQuant"))
)
sys.path.insert(0, STORYQUANT_ROOT)

import numpy as np
import pandas as pd
from flask import Flask, jsonify, request
from scipy import stats

# amure-do embeds amure-db graph API on its own port (8080)
# Override StoryQuant's default amure-db URL to point to amure-do
os.environ.setdefault("AMURE_DB_URL", "http://localhost:8080")

app = Flask(__name__)


# ---------------------------------------------------------------------------
# Data helpers
# ---------------------------------------------------------------------------

def crawl_news(hours: int = 6) -> pd.DataFrame:
    """Crawl RSS feeds and return news DataFrame."""
    from src.crawlers.news_crawler import crawl_all_news
    return crawl_all_news(hours_back=hours)


def fetch_prices(tickers: list = None, period: str = "5d", interval: str = "1h") -> pd.DataFrame:
    """Fetch OHLCV prices for tickers."""
    from src.prices.price_fetcher import fetch_prices as _fetch, get_default_tickers
    if tickers is None:
        tickers_map = get_default_tickers()
        tickers = [t for ts in tickers_map.values() for t in ts]
    return _fetch(tickers, period=period, interval=interval)


def get_price(ticker: str, hours: int = 72) -> pd.DataFrame:
    """Get price data for a single ticker from SQLite."""
    from src.db.schema import thread_connection
    from src.db.queries import get_recent_prices
    with thread_connection() as conn:
        return get_recent_prices(conn, ticker=ticker, hours=hours)


# ---------------------------------------------------------------------------
# Event helpers
# ---------------------------------------------------------------------------

def detect_events(price_df: pd.DataFrame = None) -> pd.DataFrame:
    """Detect price events from OHLCV data."""
    from src.prices.event_detector import detect_events as _detect
    if price_df is None:
        price_df = fetch_prices()
    return _detect(price_df)


def get_recent_events(limit: int = 20) -> list:
    """Get recent events from graph."""
    from src.graph.client import AmureClient
    from src.graph.reasoning import get_recent_events_from_graph
    with AmureClient() as client:
        return get_recent_events_from_graph(client, limit=limit)


# ---------------------------------------------------------------------------
# Graph helpers (amure-db)
# ---------------------------------------------------------------------------

def graph_search(query: str, top_k: int = 10) -> list:
    """RAG search the knowledge graph."""
    from src.graph.client import AmureClient
    with AmureClient() as client:
        results = client.search(query, top_k=top_k)
        return [{"node_id": r.node_id, "kind": r.kind, "statement": r.statement,
                 "score": r.score, "keywords": r.keywords} for r in results]


def graph_summary() -> dict:
    """Get graph node/edge counts."""
    from src.graph.client import AmureClient
    with AmureClient() as client:
        return client.graph_summary()


def get_narratives() -> list:
    """Get active narratives from graph."""
    from src.graph.client import AmureClient
    from src.graph.reasoning import get_active_narratives
    with AmureClient() as client:
        return get_active_narratives(client)


def get_evidence(market: str = None, limit: int = 50) -> list:
    """Get recent evidence from graph."""
    from src.graph.client import AmureClient
    from src.graph.reasoning import get_recent_evidence
    with AmureClient() as client:
        return get_recent_evidence(client, market=market, limit=limit)


def create_claim(statement: str, keywords: list, market: str = "", direction: str = "") -> str:
    """Create a new Claim (narrative) node in the graph."""
    from src.graph.client import AmureClient
    from src.graph.mapper import narrative_to_claim
    with AmureClient() as client:
        return narrative_to_claim(client, statement, keywords, market, direction)


def attribute_events() -> dict:
    """Run RAG-based attribution on unattributed events."""
    from src.graph.client import AmureClient
    from src.graph.attribution import attribute_unprocessed_events
    with AmureClient() as client:
        return attribute_unprocessed_events(client)


# ---------------------------------------------------------------------------
# Analysis helpers
# ---------------------------------------------------------------------------

def sentiment(text: str) -> tuple:
    """Score sentiment of text. Returns (sentiment, score)."""
    from src.analysis.sentiment import score_sentiment_rule_based
    return score_sentiment_rule_based(text)


def compute_factor(df: pd.DataFrame, name: str, window: int = 24) -> pd.Series:
    """
    Compute factor signal from OHLCV data.

    Factors: momentum, volatility, mean_reversion, volume_surge, event_momentum
    """
    close = df["close"]
    returns = close.pct_change()

    if name == "momentum":
        return close.pct_change(window)
    elif name == "volatility":
        return -returns.rolling(window).std()
    elif name == "mean_reversion":
        roll_mean = close.rolling(window).mean()
        roll_std = close.rolling(window).std()
        z = (close - roll_mean) / roll_std.replace(0, np.nan)
        return -z
    elif name == "volume_surge":
        vol_ratio = df["volume"] / df["volume"].rolling(window).mean()
        return (vol_ratio - 1) * np.sign(returns)
    elif name == "event_momentum":
        # StoryQuant-specific: momentum after significant events
        abs_ret = returns.abs()
        event_mask = abs_ret > abs_ret.rolling(window).mean() + 2 * abs_ret.rolling(window).std()
        signal = returns.where(event_mask, 0).rolling(window // 4).sum()
        return signal
    else:
        raise ValueError(f"Unknown factor: {name}")


def compute_ic(factor: pd.Series, returns: pd.Series, method: str = "spearman") -> float:
    """Information Coefficient between factor and returns."""
    valid = factor.notna() & returns.notna()
    f, r = factor[valid], returns[valid]
    if len(f) < 10:
        return np.nan
    if method == "spearman":
        corr, _ = stats.spearmanr(f, r)
    else:
        corr, _ = stats.pearsonr(f, r)
    return float(corr)


def factor_summary(factor: pd.Series, returns: pd.Series) -> dict:
    """Full factor performance stats."""
    ic_series = []
    for i in range(50, len(factor)):
        ic = compute_ic(factor.iloc[i-50:i], returns.iloc[i-50:i])
        ic_series.append(ic)

    ic_arr = np.array([x for x in ic_series if not np.isnan(x)])
    if len(ic_arr) == 0:
        return {"error": "Insufficient data"}

    mean_ic = float(np.mean(ic_arr))
    ic_std = float(np.std(ic_arr))

    return {
        "mean_ic": round(mean_ic, 4),
        "ic_std": round(ic_std, 4),
        "ir": round(mean_ic / ic_std, 4) if ic_std > 0 else 0,
        "hit_ratio": round(float(np.mean(ic_arr > 0)), 4),
        "t_stat": round(mean_ic / (ic_std / np.sqrt(len(ic_arr))), 4) if ic_std > 0 else 0,
        "n_obs": len(ic_arr),
    }


def narrative_returns(narrative_id: str) -> dict:
    """Get returns linked to a specific narrative."""
    from src.graph.client import AmureClient
    with AmureClient() as client:
        all_data = client.get_all()
        edges = [e for e in all_data.get("edges", [])
                 if e.get("kind") == "Support" and e.get("target") == narrative_id]
        node_map = {n["id"]: n for n in all_data.get("nodes", [])}

        returns_list = []
        for e in edges:
            source = node_map.get(e.get("source", ""), {})
            if source.get("kind") == "Fact":
                ret = source.get("metadata", {}).get("return_1h", 0)
                ticker = source.get("metadata", {}).get("ticker", "")
                ts = source.get("metadata", {}).get("timestamp", "")
                if ret:
                    returns_list.append({"ticker": ticker, "return_1h": float(ret), "timestamp": ts})

        if not returns_list:
            return {"count": 0, "returns": []}

        rets = [r["return_1h"] for r in returns_list]
        return {
            "count": len(rets),
            "avg_return": round(np.mean(rets) * 100, 4),
            "cum_return": round(sum(rets) * 100, 4),
            "win_rate": round(sum(1 for r in rets if r > 0) / len(rets) * 100, 1),
            "max": round(max(rets) * 100, 4),
            "min": round(min(rets) * 100, 4),
            "returns": returns_list,
        }


# ---------------------------------------------------------------------------
# Pipeline helpers
# ---------------------------------------------------------------------------

def run_pipeline() -> dict:
    """Run full StoryQuant pipeline: crawl -> prices -> graph -> attribute."""
    from src.pipeline import run_pipeline as _pipeline
    return _pipeline()


def discover_narratives(min_cluster: int = 3) -> dict:
    """Auto-discover narratives from evidence clusters."""
    from src.graph.client import AmureClient
    from src.graph.reasoning import discover_narratives as _discover
    with AmureClient() as client:
        return _discover(client, min_cluster_size=min_cluster)


# ---------------------------------------------------------------------------
# Tickers & config
# ---------------------------------------------------------------------------

from src.config.tickers import TICKERS, get_all_tickers, get_tickers_by_market

SYMBOLS = get_all_tickers()
CRYPTO = get_tickers_by_market("crypto")
US_STOCKS = get_tickers_by_market("us")
KR_STOCKS = get_tickers_by_market("kr")


# ---------------------------------------------------------------------------
# Execution namespace
# ---------------------------------------------------------------------------

EXEC_NAMESPACE = {
    "np": np, "pd": pd, "stats": stats,
    # Data
    "crawl_news": crawl_news,
    "fetch_prices": fetch_prices,
    "get_price": get_price,
    # Events
    "detect_events": detect_events,
    "get_recent_events": get_recent_events,
    # Graph
    "graph_search": graph_search,
    "graph_summary": graph_summary,
    "get_narratives": get_narratives,
    "get_evidence": get_evidence,
    "create_claim": create_claim,
    "attribute_events": attribute_events,
    # Analysis
    "sentiment": sentiment,
    "compute_factor": compute_factor,
    "compute_ic": compute_ic,
    "factor_summary": factor_summary,
    "narrative_returns": narrative_returns,
    # Pipeline
    "run_pipeline": run_pipeline,
    "discover_narratives": discover_narratives,
    # Config
    "TICKERS": TICKERS,
    "SYMBOLS": SYMBOLS,
    "CRYPTO": CRYPTO,
    "US_STOCKS": US_STOCKS,
    "KR_STOCKS": KR_STOCKS,
}


# ---------------------------------------------------------------------------
# Flask endpoints
# ---------------------------------------------------------------------------

@app.get("/health")
def health():
    from src.graph.client import AmureClient
    with AmureClient() as client:
        graph_ok = client.is_available()
    return jsonify({
        "status": "ok",
        "backend": "storyquant",
        "graph_connected": graph_ok,
        "tickers": len(SYMBOLS),
    })


@app.post("/exec")
def exec_code():
    body = request.get_json(force=True, silent=True) or {}
    code = body.get("code", "")

    if not code.strip():
        return jsonify({"status": "error", "output": "No code provided"}), 400

    stdout_buf = io.StringIO()
    stderr_buf = io.StringIO()
    local_ns = dict(EXEC_NAMESPACE)

    try:
        with redirect_stdout(stdout_buf), redirect_stderr(stderr_buf):
            exec(compile(code, "<experiment>", "exec"), local_ns)

        output = stdout_buf.getvalue()
        err = stderr_buf.getvalue()
        if err:
            output += f"\n[stderr]\n{err}"

        return jsonify({"status": "ok", "output": output or "(no output)"})

    except Exception:
        tb = traceback.format_exc()
        return jsonify({"status": "error", "output": tb})


# ---------------------------------------------------------------------------
# Additional endpoints for direct access (beyond /exec)
# ---------------------------------------------------------------------------

@app.get("/api/narratives")
def api_narratives():
    return jsonify(get_narratives())


@app.get("/api/events")
def api_events():
    limit = request.args.get("limit", 20, type=int)
    return jsonify(get_recent_events(limit=limit))


@app.get("/api/search")
def api_search():
    q = request.args.get("q", "")
    top_k = request.args.get("top_k", 10, type=int)
    return jsonify(graph_search(q, top_k=top_k))


@app.post("/api/pipeline")
def api_pipeline():
    result = run_pipeline()
    return jsonify(result)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    print("=== StoryQuant Backend for amure-do ===")
    print(f"StoryQuant root: {STORYQUANT_ROOT}")
    print(f"Tickers: {len(SYMBOLS)} ({len(CRYPTO)} crypto, {len(US_STOCKS)} US, {len(KR_STOCKS)} KR)")
    print()
    print("Helpers: crawl_news, fetch_prices, detect_events, graph_search,")
    print("         get_narratives, sentiment, compute_factor, compute_ic,")
    print("         factor_summary, narrative_returns, run_pipeline")
    print()
    print("amure-do backend on http://localhost:5050")
    app.run(host="0.0.0.0", port=5050, debug=False)
