# Crypto Factor Research Example

This example configures amuredo for systematic crypto alpha factor discovery,
inspired by the AlphaFactor research methodology.

## What it does

You use amuredo's Knowledge system to build a structured argument graph:

1. **Claims** — hypotheses about market factors (e.g. "momentum predicts 7-day returns on BTC")
2. **Reasons** — supporting or rebutting evidence attached to each claim
3. **Evidence** — data artifacts (backtest results, correlation matrices, regime tables)
4. **Experiments** — Julia code run against live or historical price data via the file backend
5. **Verdicts** — LLM-synthesized judgments that gate progression through the pipeline

The full gate chain (`claim_gate` → `thesis_gate` → `experiment_gate` → `argument_gate` →
`validity_gate` → `dsr_gate` → `judge_gate`) enforces rigorous reasoning before a factor
is promoted to a strategy.

## Setup

1. Copy `templates/backend-julia/server.jl` into a `backend/` subdirectory here.
2. Start amuredo from this directory:
   ```bash
   amuredo serve
   ```
3. Open `http://localhost:8080` and start adding claims on the Research tab.

## Backend

Uses the Julia file-based backend for compute-intensive backtesting.
amuredo writes experiment code to `backend/_cmd.txt`; Julia executes it and
returns output via `backend/_out.txt`.

## LLM

Defaults to `claude_cli`. Change `[llm] default_provider` in `amuredo.toml`
to any of the 11+ supported providers (openai, ollama, gemini, etc.).
