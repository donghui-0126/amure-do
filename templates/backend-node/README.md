# amure-do Node.js Backend

Minimal HTTP backend using Node's built-in `http` and `vm` modules. No npm install needed.

## Setup

```bash
node server.js
```

The server listens on `0.0.0.0:8090` by default. Set `AMUREDO_PORT` to override.
Configure `amure-do.toml` with `type = "http"` and `url = "http://localhost:8090"`.
