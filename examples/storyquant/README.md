# StoryQuant — amure-do Integration

News-driven multi-asset price attribution with graph-based causal reasoning.

## Architecture

```
amure-do (port 8080) — Hypothesis validation engine
    |
    v
StoryQuant Backend (port 5050) — News/price/event pipeline
    |
    v
amure-db (port 8081) — Graph knowledge database
```

## Quick Start

```bash
# 1. Start amure-db
cd ../../crates/amure-db  # or standalone amure-db repo
cargo run

# 2. Start StoryQuant backend
cd examples/storyquant/backend
pip install -r requirements.txt
python server.py

# 3. Start amure-do engine
cd ../..
cargo run -- --config examples/storyquant/amure-do.toml
```

## Available Helpers

In amure-do experiments, these functions are pre-loaded:

### Data
- `crawl_news(hours=6)` — Crawl RSS feeds (CoinDesk, CNBC, CoinTelegraph...)
- `fetch_prices(tickers, period)` — OHLCV from yfinance (50+ tickers)
- `get_price(ticker, hours=72)` — Single ticker from SQLite

### Events
- `detect_events(price_df)` — Detect surges/crashes/volume spikes
- `get_recent_events(limit=20)` — Events from knowledge graph

### Graph (amure-db)
- `graph_search(query, top_k=10)` — RAG search (token + synonym + BFS + MMR)
- `graph_summary()` — Node/edge counts by kind
- `get_narratives()` — Active market narratives (Claims)
- `get_evidence(market=None)` — Recent news Evidence nodes
- `create_claim(statement, keywords)` — Create narrative Claim
- `attribute_events()` — Run RAG attribution on new events

### Analysis
- `sentiment(text)` — Rule-based sentiment (bullish/bearish/neutral)
- `compute_factor(df, name, window)` — Factor signals (momentum, volatility, mean_reversion, volume_surge, event_momentum)
- `compute_ic(factor, returns)` — Information coefficient
- `factor_summary(factor, returns)` — Full stats (IC, IR, hit ratio, t-stat)
- `narrative_returns(narrative_id)` — Returns linked to a narrative

### Pipeline
- `run_pipeline()` — Full pipeline (crawl -> graph -> attribute -> narratives)
- `discover_narratives()` — Auto-discover from evidence clusters

## Example Experiment

```python
# Test: Do narratives with 5+ evidence predict positive returns?
narratives = get_narratives()
for n in narratives:
    if n["evidence_count"] >= 5:
        stats = narrative_returns(n["id"])
        print(f'{n["statement"][:50]}')
        print(f'  avg: {stats["avg_return"]:+.2f}%, win: {stats["win_rate"]:.0f}%, n={stats["count"]}')
```
