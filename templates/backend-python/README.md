# amure-do Python Backend

Minimal HTTP backend using Python's built-in `http.server`. No framework required.

## Setup

```bash
pip install -r requirements.txt
python server.py
```

The server listens on `0.0.0.0:8090` by default. Set `AMUREDO_PORT` to override.
Configure `amure-do.toml` to point at this backend with `type = "http"` and `url = "http://localhost:8090"`.
