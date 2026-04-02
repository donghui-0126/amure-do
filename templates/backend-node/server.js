/**
 * amure-do Node.js HTTP Backend Server
 *
 * Implements the amure-do backend protocol:
 *   GET  /health  -> {"status": "ok"}
 *   POST /exec    -> {"status": "ok", "output": "..."}
 *
 * /exec body: {"code": "...", "timeout_secs": 300}
 *
 * The code string is executed via Node's vm module with stdout captured.
 */

const http = require("http");
const vm = require("vm");
const fs = require("fs");
const path = require("path");

const HOST = process.env.AMUREDO_HOST || "0.0.0.0";
const PORT = parseInt(process.env.AMUREDO_PORT || "8090", 10);
const READY_FILE = path.join(process.cwd(), "_ready");

/**
 * Execute a code string, capturing console.log output.
 * Returns { output, success }.
 */
function executeCode(code) {
  const lines = [];
  const fakeConsole = {
    log: (...args) => lines.push(args.map(String).join(" ")),
    error: (...args) => lines.push(args.map(String).join(" ")),
    warn: (...args) => lines.push(args.map(String).join(" ")),
  };

  const sandbox = {
    console: fakeConsole,
    require,
    process,
    Buffer,
    setTimeout,
    clearTimeout,
  };

  try {
    vm.runInNewContext(code, sandbox, { timeout: 30_000 });
    return { output: lines.join("\n"), success: true };
  } catch (err) {
    return { output: String(err), success: false };
  }
}

const server = http.createServer((req, res) => {
  const send = (status, body) => {
    const payload = JSON.stringify(body);
    res.writeHead(status, {
      "Content-Type": "application/json",
      "Content-Length": Buffer.byteLength(payload),
    });
    res.end(payload);
  };

  if (req.method === "GET" && req.url === "/health") {
    return send(200, { status: "ok" });
  }

  if (req.method === "POST" && req.url === "/exec") {
    let raw = "";
    req.on("data", (chunk) => (raw += chunk));
    req.on("end", () => {
      let body;
      try {
        body = JSON.parse(raw);
      } catch {
        return send(400, { error: "invalid JSON" });
      }

      const { output, success } = executeCode(body.code || "");
      send(200, { status: success ? "ok" : "error", output });
    });
    return;
  }

  send(404, { error: "not found" });
});

server.listen(PORT, HOST, () => {
  console.log(`amure-do backend listening on ${HOST}:${PORT}`);
  fs.writeFileSync(READY_FILE, "ready\n");
});

process.on("SIGINT", () => {
  try { fs.unlinkSync(READY_FILE); } catch {}
  process.exit(0);
});
