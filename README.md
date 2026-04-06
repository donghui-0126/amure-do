# amure-do

**Hypothesis engine that works, regardless.**

amure-do is a hypothesis-driven research engine that structures argumentation, manages evidence, and runs experiments across any domain. The name comes from the Korean word "아무래도" (amuredo), meaning "regardless" — it works regardless of domain, backend, or AI provider.

## Features

- **Knowledge System**: Structure research as Claims → Reasons (Support/Rebut) → Evidence → Experiments → Verdicts. Enforce rigor at each stage.
- **7 Quality Gates**: Claim Gate, Thesis Gate, Experiment Gate, Argument Gate, Validity Gate, DSR Gate, Judge Gate — each validates a specific aspect of research.
- **Pluggable Backends**: HTTP, File-based protocol, Subprocess, or None. Integrate Python, Julia, Node.js, R, or any language.
- **Universal LLM Routing**: Support for 11 providers out of the box (Claude, OpenAI, Google AI, Azure, Groq, Together AI, Ollama, LM Studio, and custom). Route different tasks to different models.
- **Web Dashboard**: 4-tab interface (Research, Knowledge, Lab, Settings) with real-time activity feed.
- **Adaptive Mode**: Learn from user disagreements and adjust research behavior dynamically.
- **Canvas**: Tree-based idea notepad with direct knowledge base references.

## Architecture

```
Rust Engine (:8080)           Web Dashboard
  ↓                             ↓
Knowledge DB + Gates ←→ HTTP API
  ↓                             ↓
Backend (pluggable)      Lab Chat
  ↓
LLM Provider (11 options)
```

Single-binary deployment. Async runtime (Tokio). JSON-based knowledge store with Arrow data support.

## Quick Start

### 1. Build

```bash
cargo build --release
```

### 2. Configure

Edit `amure-do.toml`:

```toml
[project]
name = "My Research"
domain = "general"

[server]
host = "0.0.0.0"
port = 8080

[llm]
default_provider = "claude_cli"
# Other options: claude_api, openai, google, azure, groq, together, ollama, lmstudio, openai_compatible, custom

[gates]
enabled = ["claim_gate", "argument_gate"]
```

### 3. Run

```bash
./target/release/amure-do
```

Open http://localhost:8080 in your browser. The dashboard loads immediately; no external dependencies needed.

## Configuration

### amure-do.toml Sections

#### `[project]`
- `name` (string): Research project name
- `domain` (string): Domain identifier (e.g., "crypto", "biology", "general")
- `description` (string, optional): Project description

#### `[server]`
- `host` (string): Bind address (default: "0.0.0.0")
- `port` (integer): Listen port (default: 8080)

#### `[backend]`
- `type` (string): One of `none`, `http`, `file`, `subprocess`
- `url` (string, for HTTP): Backend URL (e.g., "http://localhost:5000")
- `dir` (string, for file-based): Backend working directory
- `command` (string, for subprocess): Executable to run
- `script` (string, for subprocess): Script file to execute
- `timeout_secs` (integer): Execution timeout (default: 300)

#### `[llm]`
- `default_provider` (string): Default LLM provider
- `default_model` (string, optional): Default model name
- `default_api_key` (string, optional): API key (or use env var)
- `default_api_url` (string, optional): Custom API endpoint
- `max_tokens` (integer): Max output tokens (default: 4096)

#### `[gates]`
- `enabled` (list): Enabled quality gates. Available: `claim_gate`, `thesis_gate`, `experiment_gate`, `argument_gate`, `validity_gate`, `dsr_gate`, `judge_gate`

#### `[dashboard]`
- `title` (string): Dashboard title
- `accent_color` (string): UI accent color (hex, e.g., "#58a6ff")

## Backend Setup

### HTTP Backend (Python Example)

1. Copy the Python template:

```bash
cp -r templates/backend-python/ my-backend/
cd my-backend
pip install -r requirements.txt
```

2. Start the backend:

```bash
python server.py
```

The backend listens on `http://0.0.0.0:8090` by default. Set `AMUREDO_PORT` to change it.

3. Configure amure-do.toml:

```toml
[backend]
type = "http"
url = "http://localhost:8090"
```

4. The backend implements the amuredo protocol:
   - `GET /health` → `{"status": "ok"}`
   - `POST /exec` → Execute code from `{"code": "...", "timeout_secs": 300}`

### File-Based Backend (Julia Example)

1. Copy the Julia template:

```bash
cp -r templates/backend-julia/ my-backend/
cd my-backend
```

2. Start the backend:

```bash
julia server.jl
```

The backend watches for `_cmd.txt`, executes it, and writes output to `_out.txt`.

3. Configure amure-do.toml:

```toml
[backend]
type = "file"
dir = "./my-backend"
```

### Subprocess Backend (Node.js Example)

1. Copy the Node.js template:

```bash
cp -r templates/backend-node/ my-backend/
cd my-backend
npm install
```

2. Configure amure-do.toml:

```toml
[backend]
type = "subprocess"
command = "node"
script = "my-backend/server.js"
timeout_secs = 300
```

### No Backend

For LLM-only research without code execution:

```toml
[backend]
type = "none"
```

## LLM Providers

amuredo routes to 11 providers with automatic fallback. Configure each via environment variables or `amure-do.toml`.

| Provider | Type | Setup | Default URL |
|----------|------|-------|-------------|
| `claude_cli` | Cloud | `claude` CLI installed | (N/A) |
| `claude_api` | Cloud | `ANTHROPIC_API_KEY` env var | api.anthropic.com |
| `openai` | Cloud | `OPENAI_API_KEY` env var | api.openai.com |
| `google` | Cloud | `GOOGLE_API_KEY` env var | generativelanguage.googleapis.com |
| `azure` | Cloud | `AZURE_OPENAI_KEY`, `AZURE_OPENAI_ENDPOINT` | {resource}.openai.azure.com |
| `groq` | Cloud | `GROQ_API_KEY` env var | api.groq.com |
| `together` | Cloud | `TOGETHER_API_KEY` env var | api.together.xyz |
| `ollama` | Local | `ollama serve` running | localhost:11434 |
| `lmstudio` | Local | LM Studio running | localhost:1234 |
| `openai_compatible` | Any | `OPENAI_COMPATIBLE_KEY`, custom URL | (custom) |
| `custom` | Any | `CUSTOM_API_KEY`, custom URL | (custom) |

### Example: Using Claude API

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

```toml
[llm]
default_provider = "claude_api"
default_model = "claude-sonnet-4-20250514"
```

### Example: Using Ollama (Local)

```bash
ollama serve  # in another terminal
```

```toml
[llm]
default_provider = "ollama"
default_model = "neural-chat"
```

## Knowledge System

amuredo structures research as a progression through increasing rigor:

```
Claim (hypothesis)
  ↓
Thesis (refined argument structure)
  ↓
Experiment (testable design)
  ↓
Argument (support/rebut evidence)
  ↓
Verdict (judgment with confidence)
```

Each stage has an associated gate that validates assumptions and enforces standards.

### Gates

1. **Claim Gate**: Is the claim well-formed and testable?
2. **Thesis Gate**: Is the argument structure sound?
3. **Experiment Gate**: Can the hypothesis be tested?
4. **Argument Gate**: Are supporting/rebutting arguments rigorous?
5. **Validity Gate**: Are results reproducible and valid?
6. **DSR Gate**: Data, Source, Reasoning — is the evidence complete?
7. **Judge Gate**: Final verdict confidence and caveats.

Enable gates in `amure-do.toml`:

```toml
[gates]
enabled = ["claim_gate", "argument_gate", "validity_gate"]
```

## Dashboard

### Research Tab

Create and manage claims. Each claim flows through gates to a verdict.

### Knowledge Tab

Search the knowledge base. Filter by type (hypothesis, experiment, insight). View evidence and argument chains.

### Lab Tab

Interactive chat with the LLM. Reference claims and evidence inline. Build experiments interactively.

### Settings Tab

Configure:
- Active gates
- LLM provider and model
- Backend URL/type
- Dashboard accent color

## Examples

### StoryQuant — Narrative Market Intelligence (Recommended)

News-driven multi-asset price attribution with graph-based causal reasoning. StoryQuant crawls 20+ sources, detects price events across 50+ tickers, and attributes causes via RAG search.

```
amure-do (:8080) — Hypothesis engine + embedded graph DB
       |
       v  POST /exec
StoryQuant Backend (:5050) — 20+ helper functions
       |
       v  Streamlit (:8501) + Cloudflare Tunnel
Dashboard — Signals, Events, Paper Trade, Explorer
```

1. Build and start amure-do:

```bash
cargo build --release
./target/release/amure-do --config examples/storyquant/amure-do.toml
```

2. Start StoryQuant backend:

```bash
cd examples/storyquant/backend
pip install -r requirements.txt
STORYQUANT_ROOT=/path/to/StoryQuant python server.py
```

3. Open http://localhost:8080 for amure-do dashboard

4. Available helpers in Lab experiments:

```python
# Crawl news and detect events
news = crawl_news(hours=6)
events = detect_events()

# RAG search the knowledge graph
results = graph_search("BTC ETF inflow", top_k=10)

# Get narratives with returns
narratives = get_narratives()
stats = narrative_returns(narratives[0]["id"])
print(f"Avg: {stats['avg_return']:+.2f}%, Win: {stats['win_rate']:.0f}%")

# Factor analysis
df = get_price("BTC-USD", hours=120)
factor = compute_factor(df, "momentum", window=24)
returns = df["close"].pct_change().shift(-1)
summary = factor_summary(factor, returns)
print(f"IC: {summary['mean_ic']:.4f}, IR: {summary['ir']:.4f}")

# Full pipeline
run_pipeline()  # crawl -> graph -> attribute -> narratives
```

See [examples/storyquant/README.md](examples/storyquant/README.md) for full documentation.

### Simple Research (LLM Only)

1. Create `amure-do.toml`:

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
3. Submit a claim: "Quantum entanglement enables faster-than-light communication"
4. The Claim Gate validates it's well-formed and testable
5. Refine the thesis, design experiments, and gather evidence

### Crypto Research (With Python Backend)

1. Start the Python backend on port 8090:

```bash
cd templates/backend-python
python server.py &
```

2. Create `amure-do.toml`:

```toml
[project]
name = "Crypto Research"
domain = "crypto"

[backend]
type = "http"
url = "http://localhost:8090"

[llm]
default_provider = "openai"

[gates]
enabled = ["claim_gate", "argument_gate", "validity_gate"]
```

3. Run amuredo and submit claims like:
   - "Bitcoin correlation with tech stocks has decreased in 2024"
   - "Ethereum L2 adoption follows an S-curve"

4. In the Lab, write Python code to test claims:

```python
import pandas as pd
import numpy as np

# Load price data
df = pd.read_csv("crypto_prices.csv")
correlation = df["BTC"].corr(df["TECH"])
print(f"Correlation: {correlation:.3f}")
```

5. The Verdict Gate summarizes findings with confidence.

## Building from Source

### Prerequisites

- Rust 1.70+ ([install](https://rustup.rs/))
- Optional: Python 3.11+ (for Python backend)
- Optional: Julia 1.9+ (for Julia backend)
- Optional: Node.js 18+ (for Node.js backend)

### Build

```bash
cargo build --release
```

Binary: `target/release/amure-do`

### Development

```bash
cargo build      # Debug build
cargo run        # Run in debug mode
cargo test       # Run tests
cargo clippy     # Lint check
cargo fmt        # Format code
```

## Extending amuredo

### Custom Backend

Implement the amuredo backend protocol:

1. **HTTP**: Serve `GET /health` and `POST /exec` endpoints
2. **File-based**: Watch `_cmd.txt`, write `_out.txt`, signal `_ready`
3. **Subprocess**: Accept code on stdin, write output to stdout

See `templates/backend-python`, `templates/backend-julia`, `templates/backend-node` for examples.

### Custom LLM Provider

Edit `engine/server/llm_provider.rs` and add a new provider variant. Implement the provider trait and wire it into the router.

### Custom Gate

Create a new gate struct in `engine/knowledge/framework.rs`. Implement validation logic. Enable in `amure-do.toml`.

## Contributing

Contributions are welcome. Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Write tests for new functionality
4. Run `cargo test` and `cargo clippy` to verify
5. Commit with clear messages
6. Open a pull request with a description

## License

MIT License. See [LICENSE](LICENSE) for details.

## Support

- **Issues**: Report bugs and feature requests on GitHub
- **Discussions**: Ask questions in Discussions
- **Documentation**: Extended docs at https://amure-do.dev (coming soon)

---

**amure-do** — Hypothesis engine that works, regardless.
