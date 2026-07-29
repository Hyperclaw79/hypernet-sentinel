import { spawn } from "node:child_process";
import { watch } from "node:fs";
import http from "node:http";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(scriptDirectory, "..");
const webDirectory =
  process.env.SENTINEL_WEB_DIR ?? path.join(workspaceRoot, "web");
const bind = process.env.SENTINEL_BIND ?? "127.0.0.1:8080";
const upstream = new URL(`http://${bind}`);
const liveReloadPort = Number(process.env.SENTINEL_LIVE_RELOAD_PORT ?? "3000");
const watchexec = process.env.SENTINEL_WATCHEXEC ?? "watchexec";
const cargo = process.env.SENTINEL_CARGO ?? "cargo";
const clients = new Set();
const sourceWatchers = [];
let webReloadTimer;
let restartCheckRunning = false;
let restartCheckQueued = false;
let shuttingDown = false;

const reloadSnippet = `<script>new EventSource('/__sentinel_reload').onmessage=()=>location.reload();</script>`;

function broadcastReload() {
  for (const response of clients) response.write("data: reload\n\n");
}

function scheduleWebReload() {
  clearTimeout(webReloadTimer);
  webReloadTimer = setTimeout(broadcastReload, 100);
}

async function serverHealthy() {
  return await new Promise((resolve) => {
    const request = http.get(new URL("/healthz", upstream), (response) => {
      response.resume();
      resolve(response.statusCode === 200);
    });
    request.setTimeout(500, () => request.destroy());
    request.on("error", () => resolve(false));
  });
}

async function waitForRestart() {
  if (restartCheckRunning) {
    restartCheckQueued = true;
    return;
  }
  restartCheckRunning = true;
  let sawUnavailable = false;
  const deadline = Date.now() + 120_000;
  while (Date.now() < deadline && !shuttingDown) {
    const healthy = await serverHealthy();
    if (!healthy) sawUnavailable = true;
    if (healthy && sawUnavailable) {
      broadcastReload();
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  restartCheckRunning = false;
  if (restartCheckQueued) {
    restartCheckQueued = false;
    void waitForRestart();
  }
}

const serverProcess = spawn(
  watchexec,
  [
    "--restart",
    "--watch",
    "crates",
    "--watch",
    "Cargo.toml",
    "--watch",
    "Cargo.lock",
    "--watch",
    "rust-toolchain.toml",
    "--exts",
    "rs,toml",
    "--",
    cargo,
    "run",
    "--locked",
    "--package",
    "sentinel-server",
    "--bin",
    "sentinel-server",
  ],
  { cwd: workspaceRoot, env: process.env, stdio: "inherit" },
);

serverProcess.on("exit", (code, signal) => {
  if (!shuttingDown) {
    console.error(`Development server watcher exited (${signal ?? code}).`);
    shutdown(code ?? 1);
  }
});

const proxy = http.createServer((request, response) => {
  const requestUrl = new URL(
    request.url ?? "/",
    `http://${request.headers.host ?? "localhost"}`,
  );
  if (requestUrl.pathname === "/__sentinel_reload") {
    response.writeHead(200, {
      "Cache-Control": "no-cache",
      Connection: "keep-alive",
      "Content-Type": "text/event-stream",
    });
    response.write(": connected\n\n");
    clients.add(response);
    request.on("close", () => clients.delete(response));
    return;
  }

  const headers = { ...request.headers, host: upstream.host };
  delete headers["accept-encoding"];
  const upstreamRequest = http.request(
    new URL(request.url ?? "/", upstream),
    { method: request.method, headers },
    (upstreamResponse) => {
      const contentType = upstreamResponse.headers["content-type"] ?? "";
      if (!contentType.includes("text/html")) {
        response.writeHead(
          upstreamResponse.statusCode ?? 502,
          upstreamResponse.headers,
        );
        upstreamResponse.pipe(response);
        return;
      }
      const chunks = [];
      upstreamResponse.on("data", (chunk) => chunks.push(chunk));
      upstreamResponse.on("end", () => {
        const html = Buffer.concat(chunks)
          .toString("utf8")
          .replace("</body>", `${reloadSnippet}</body>`);
        const responseHeaders = { ...upstreamResponse.headers };
        delete responseHeaders["content-length"];
        delete responseHeaders["content-encoding"];
        response.writeHead(upstreamResponse.statusCode ?? 200, responseHeaders);
        response.end(html);
      });
    },
  );
  upstreamRequest.on("error", (error) => {
    if (!response.headersSent)
      response.writeHead(502, { "Content-Type": "text/plain" });
    response.end(`Development server unavailable: ${error.message}`);
  });
  request.pipe(upstreamRequest);
});

sourceWatchers.push(
  watch(webDirectory, { recursive: true }, scheduleWebReload),
);
sourceWatchers.push(
  watch(
    path.join(workspaceRoot, "crates"),
    { recursive: true },
    waitForRestart,
  ),
);
for (const file of ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]) {
  sourceWatchers.push(watch(path.join(workspaceRoot, file), waitForRestart));
}

proxy.listen(liveReloadPort, "127.0.0.1", () => {
  console.log(`Hot reload: http://127.0.0.1:${liveReloadPort}`);
});

function shutdown(code = 0) {
  if (shuttingDown) return;
  shuttingDown = true;
  clearTimeout(webReloadTimer);
  for (const watcher of sourceWatchers) watcher.close();
  for (const response of clients) response.end();
  proxy.close();
  if (!serverProcess.killed) serverProcess.kill("SIGTERM");
  process.exitCode = code;
}

process.on("SIGINT", () => shutdown());
process.on("SIGTERM", () => shutdown());
