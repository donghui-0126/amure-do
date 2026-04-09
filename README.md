# amure-do

**Hypothesis-driven knowledge accumulation framework. Works regardless of domain, backend, or AI provider.**

amure-do (아무래도) structures research as a graph of claims, reasons, and evidence. It enforces rigor at each stage of the argument, runs experiments on any backend, and works with any LLM provider.

## Architecture Overview

```
amure-do (:8080)
  ├─ amure-db (graph knowledge engine)
  │   ├─ Nodes: Claim, Reason, Evidence, Experiment, Fact
  │   ├─ Edges: Support, Rebut, DependsOn, Contradicts, Refines, DerivedFrom
  │   ├─ 3-layer RAG search (token match → graph walk → MMR)
  │   └─ Synonym dictionary (Korean/English, 30+ term groups)
  │
  ├─ Claim Lifecycle
  │   Create → Add Reasons → Gather Evidence → Run Experiments → Verdict
  │   With configurable quality gates before acceptance
  │
  ├─ Lab (LLM Chat)
  │   Sessions with claim context injection
  │   Any LLM provider (11 supported)
  │
  ├─ Backend (pluggable)
  │   HTTP / File-based / Subprocess / None
  │
  └─ Dashboard (5-tab SPA)
      Setup Wizard → Research → Knowledge → Lab → Settings
```

**Key facts:** 15 Rust files, 6-field AppState, 7 handler modules, ~50 API endpoints, 23 unit tests, single ~7MB binary.

## Core Concept: Claim Lifecycle

Everything is a graph operation on amure-db:

1. **Create Claim** — a testable hypothesis ("X is true because Y")
2. **Add Reasons** — Support (why it's true) or Rebut (why it might be false)
3. **Gather Evidence** — tag: backtest, literature, intuition, observation
4. **Run Experiments** — design, execute via backend, record results
5. **Verdict** — Accept (→ Knowledge) or Reject, with gate checks

Quality gates (configurable):
- `claim_gate` — statement ≥10 chars, trigger present
- `argument_gate` — ≥1 support reason with bridge
- `evidence_gate` — ≥1 evidence per support reason
- `experiment_gate` — ≥1 completed experiment

## Configuration

### First Run: Setup Wizard

On first start (empty graph + default config), the dashboard shows a 6-step wizard:

1. Welcome
2. Project name + domain
3. Backend selection (None / Python / Julia / Node / Custom)
4. LLM provider (Claude CLI / Claude API / OpenAI / Ollama / Gemini / Groq / Together / Azure / LM Studio / Custom)
5. Quality gates (which to enable)
6. Done → writes amure-do.toml

### amure-do.toml

```toml
[project]
name = "My Research"
domain = "general"
description = "Hypothesis-driven research project"

[server]
host = "0.0.0.0"
port = 8080

[backend]
type = "none"         # none, http, file, subprocess
# url = "http://localhost:5000"
timeout_secs = 300

[llm]
default_provider = "claude_cli"
max_tokens = 4096

[gates]
enabled = ["claim_gate", "argument_gate"]

[dashboard]
title = "amure-do"
accent_color = "#7c3aed"
```

## LLM Providers (11 Supported)

| Provider | Type | Needs Key | Default URL |
|----------|------|-----------|-------------|
| claude_cli | Local | No | (CLI) |
| claude_api | Cloud | Yes | api.anthropic.com |
| openai | Cloud | Yes | api.openai.com |
| google | Cloud | Yes | generativelanguage.googleapis.com |
| azure | Cloud | Yes | {resource}.openai.azure.com |
| groq | Cloud | Yes | api.groq.com |
| together | Cloud | Yes | api.together.xyz |
| ollama | Local | No | localhost:11434 |
| lmstudio | Local | No | localhost:1234 |
| openai_compatible | Any | Optional | (custom) |
| custom | Any | Optional | (custom) |

Role-based routing: assign different providers to different tasks.

### Example: Claude API

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

```toml
[llm]
default_provider = "claude_api"
```

### Example: Ollama (Local)

```bash
ollama serve  # in another terminal
```

```toml
[llm]
default_provider = "ollama"
```

## Backend Types

| Type | Protocol | Use case |
|------|----------|----------|
| none | N/A | LLM-only research (no code execution) |
| http | GET /health, POST /exec | Python Flask, Node Express, any HTTP server |
| file | _cmd.txt → _out.txt | Julia file watcher, legacy protocols |
| subprocess | stdin/stdout | Direct command execution |

### HTTP Backend Setup (Python)

```bash
cd templates/backend-python
pip install -r requirements.txt
python3 server.py  # → localhost:5000
```

Configure amure-do.toml:

```toml
[backend]
type = "http"
url = "http://localhost:5000"
```

Backend protocol:
- `GET /health` → `{"status": "ok"}`
- `POST /exec` → `{"code": "...", "timeout_secs": 300}`

### File-Based Backend (Julia)

```bash
cd templates/backend-julia
julia server.jl
```

Configure amure-do.toml:

```toml
[backend]
type = "file"
dir = "./templates/backend-julia"
```

The backend watches `_cmd.txt`, executes it, writes output to `_out.txt`.

### Subprocess Backend (Node.js)

```bash
cd templates/backend-node
npm install
```

Configure amure-do.toml:

```toml
[backend]
type = "subprocess"
command = "node"
script = "./templates/backend-node/server.js"
timeout_secs = 300
```

## Graph Database: amure-db

- In-memory adjacency list, JSON persistence
- 3-layer RAG: token match + synonym expansion → BFS graph walk → MMR diversity reranking
- 30+ Korean/English synonym groups (e.g., "미결제약정" ↔ "open_interest" ↔ "OI")
- Knowledge utilization: failure warnings, revalidation, contradiction detection
- Yahoo Finance integration for Fact nodes
- Force-directed graph visualization at `/graph`

Node types:
- **Claim** — testable hypothesis
- **Reason** — support or rebut argument
- **Evidence** — backtest, literature, intuition, observation
- **Experiment** — design and execution record
- **Fact** — validated assertion from Yahoo Finance or other sources

Edge types:
- **Support** — reason strengthens claim
- **Rebut** — reason weakens claim
- **DependsOn** — causal dependency
- **Contradicts** — logical opposition
- **Refines** — clarification or improvement
- **DerivedFrom** — reasoning trail

## API Endpoints (Key)

| Path | Method | Description |
|------|--------|-------------|
| /api/health | GET | System status |
| /api/setup/status | GET | First-run detection |
| /api/setup/init | POST | Configure project |
| /api/claims | GET/POST | List/create claims |
| /api/claims/{id} | GET/DELETE | Claim detail/delete |
| /api/claims/{id}/reason | POST | Add support/rebut reason |
| /api/claims/{id}/verdict | POST | Accept/reject with gates |
| /api/claims/auto-generate | POST | LLM generates claim from idea |
| /api/lab/sessions | GET/POST | Chat sessions |
| /api/lab/send | POST | Send message, get LLM response |
| /api/graph/search?q= | GET | RAG search |
| /api/graph/all | GET | Full graph data |
| /api/backend/exec | POST | Execute code on backend |
| /graph | GET | Graph visualization dashboard |

## Dashboard

5-tab interface:

1. **Research** — Create and manage claims. Each claim flows through gates to a verdict.
2. **Knowledge** — Search the graph. Filter by type. View evidence and argument chains.
3. **Lab** — Interactive chat with LLM. Reference claims and evidence inline. Design experiments.
4. **Settings** — Configure gates, LLM provider, backend, accent color.
5. **Graph** — Force-directed visualization of knowledge graph.

## Quick Start

### 1. Build

```bash
cargo build --release
```

### 2. Run

```bash
./target/release/amure-do
```

Open http://localhost:8080. First run shows setup wizard. Or pre-configure `amure-do.toml`.

### 3. Create a Claim

Research tab → Create Claim button → enter hypothesis.

Example: "Bitcoin volatility decreases when correlation with tech stocks increases"

### 4. Add Reasons

Click claim → Add Reason button → enter support or rebut argument with bridge (why it's relevant).

### 5. Gather Evidence

Click reason → Add Evidence button → choose tag (backtest, literature, intuition, observation) → describe.

### 6. Run Experiment

Lab tab → write code → execute on backend → record result as experiment.

### 7. Verdict

Back to Research tab → click Verdict button → accept or reject → gate checks validate.

## Examples

### Simple Research (LLM Only, No Backend)

1. Create amure-do.toml:

```toml
[project]
name = "Quick Research"

[backend]
type = "none"

[llm]
default_provider = "claude_cli"

[gates]
enabled = ["claim_gate"]
```

2. Run and open http://localhost:8080
3. Submit a claim and gather literature evidence
4. Lab chat helps refine arguments

### Crypto Research (With Python Backend)

1. Start backend:

```bash
cd templates/backend-python
python3 server.py &
```

2. Create amure-do.toml:

```toml
[project]
name = "Crypto Research"
domain = "crypto"

[backend]
type = "http"
url = "http://localhost:5000"

[llm]
default_provider = "claude_api"

[gates]
enabled = ["claim_gate", "argument_gate", "evidence_gate", "experiment_gate"]
```

3. Run amure-do:

```bash
./target/release/amure-do
```

4. Create claim: "Funding rate reversal is predictive of price reversal"
5. Lab tab → write experiment code:

```python
import pandas as pd
import numpy as np

# Load funding rates and prices
df = pd.read_csv("funding_rates.csv")
df["price_change"] = df["price"].pct_change().shift(-1)
df["funding_reversal"] = df["funding_rate"].diff() < -0.001

correlation = df[df["funding_reversal"]]["price_change"].mean()
print(f"Avg return after reversal: {correlation:.3%}")
```

6. Execute → record results → verdict

### StoryQuant: News-Driven Market Intelligence

See `examples/storyquant/README.md` for full walkthrough with 20+ helper functions, news crawling, event detection, RAG search, and narrative attribution.

## Project Structure

```
amure-do/
  engine/
    main.rs                 # Entry point + first-run detection
    config/mod.rs           # TOML config system
    server/
      routes.rs             # 6-field AppState, all routes
      llm_provider.rs       # 11 AI providers + role routing
      backend.rs            # Pluggable backends
      handlers/
        claims.rs           # Claim lifecycle (10 endpoints)
        lab.rs              # LLM chat + context injection
        graph.rs            # amure-db API (27 endpoints)
        setup.rs            # Setup wizard
        health.rs           # Status check
        dashboard.rs        # Serve UI
        backend.rs          # Backend proxy
    dashboard/
      index.html            # 5-tab SPA with setup wizard
      graph.html            # Force-directed graph viz
  crates/
    amure-db/               # Graph knowledge engine
      src/ (7 modules)      # node, edge, graph, search, synonym, persistence
      tests/ (23 tests)
  examples/
    crypto-research/        # Crypto factor research with Python backend
    simple-research/        # LLM-only literature review
    storyquant/             # News-driven market intelligence
  templates/
    backend-python/         # Flask HTTP backend template
    backend-julia/          # File-based backend template
    backend-node/           # Node.js HTTP backend template
```

## Building from Source

### Prerequisites

- Rust 1.80+ ([install](https://rustup.rs/))
- Optional: Python 3.11+ (for Python backend)
- Optional: Julia 1.9+ (for Julia backend)
- Optional: Node.js 18+ (for Node.js backend)

### Build and Test

```bash
git clone https://github.com/donghui-0126/amure-do.git
cd amure-do

cargo build --release
cargo test -p amure-db --lib  # 23 unit tests
```

### Development

```bash
cargo build      # Debug build
cargo run        # Run in debug mode
cargo test       # Run all tests
cargo clippy     # Lint check
cargo fmt        # Format code
```

## License

MIT License. See [LICENSE](LICENSE) for details.

---

**아무래도 — 뭘 던져도 돌아간다.**

*Amuredo — it works regardless of what you throw at it.*
