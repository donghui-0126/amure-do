# Simple Research Example

A minimal amuredo setup for literature review and knowledge management —
no compute backend required.

## What it does

Uses amuredo purely as a structured reasoning layer:

- Add **Claims** (hypotheses or propositions from the literature)
- Attach **Reasons** that support or rebut each claim
- Link **Evidence** (paper excerpts, citations, notes)
- Let the LLM synthesize **Verdicts** via `claim_gate`

This is useful for systematic reviews, debate preparation, or any task where
you want to build and stress-test an argument graph without running code.

## Setup

```bash
amuredo serve
```

Open `http://localhost:8080`. No backend process needed — `type = "none"` in the config.

## LLM

Defaults to a local Ollama instance (`llama3.1`). Change `[llm] default_provider`
and `default_model` in `amuredo.toml` to use any other provider.
