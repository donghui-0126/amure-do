# amuredo Julia Backend

File-based backend for Julia compute. amuredo writes code to `_cmd.txt`; this server executes it and writes results to `_out.txt`.

## Setup

```bash
julia server.jl
```

No external packages required. Configure `amuredo.toml` with `type = "file"` and `dir` pointing to this directory.
