import { createServer } from "node:http";
import { randomUUID } from "node:crypto";
import { createWriteStream } from "node:fs";
import { spawn } from "node:child_process";
import { readFile, readdir, rename, stat, writeFile, mkdir } from "node:fs/promises";
import { dirname, extname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = join(ROOT, "../..");
const RECORDINGS_ROOT = join(ROOT, "recordings");
const RUNS_ROOT = join(ROOT, "runs");
const MAX_AUDIO_BYTES = 30 * 1024 * 1024;
const port = parsePort(process.argv.slice(2));
const cases = JSON.parse(await readFile(join(ROOT, "cases.json"), "utf8"));
const casesById = new Map(cases.map((item) => [item.id, item]));
const runningJobs = new Map();
const staticFiles = new Map([
  ["/", "index.html"],
  ["/index.html", "index.html"],
  ["/app.js", "app.js"],
  ["/styles.css", "styles.css"],
  ["/cases.json", "cases.json"],
]);

await mkdir(RECORDINGS_ROOT, { recursive: true });
await mkdir(RUNS_ROOT, { recursive: true });

const server = createServer(async (request, response) => {
  try {
    setSecurityHeaders(response);
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "localhost"}`);
    if (request.method === "GET" && url.pathname === "/favicon.ico") {
      response.writeHead(204);
      response.end();
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/status") {
      await sendStatus(response);
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/providers") {
      await sendProviders(response);
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/runs") {
      await sendRuns(response);
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/runs") {
      await startRun(request, response);
      return;
    }
    const runMatch = url.pathname.match(/^\/api\/runs\/([A-Za-z0-9_-]+)$/);
    if (request.method === "GET" && runMatch) {
      await sendRun(response, runMatch[1]);
      return;
    }
    const cancelMatch = url.pathname.match(/^\/api\/runs\/([A-Za-z0-9_-]+)\/cancel$/);
    if (request.method === "POST" && cancelMatch) {
      await cancelRun(response, cancelMatch[1]);
      return;
    }
    if (request.method === "GET" && url.pathname.startsWith("/api/recordings/")) {
      await sendRecording(response, decodeURIComponent(url.pathname.slice(16)));
      return;
    }
    if (request.method === "PUT" && url.pathname.startsWith("/api/recordings/")) {
      await saveRecording(request, response, decodeURIComponent(url.pathname.slice(16)));
      return;
    }
    if ((request.method === "GET" || request.method === "HEAD") && staticFiles.has(url.pathname)) {
      await sendStatic(request, response, staticFiles.get(url.pathname));
      return;
    }
    sendJson(response, 404, { error: "not_found" });
  } catch (error) {
    console.error(error);
    if (!response.headersSent) {
      sendJson(response, error?.statusCode ?? 500, {
        error: error?.statusCode === 413 ? "audio_too_large" : "local_server_error",
      });
    } else {
      response.destroy();
    }
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`Saymore voice evaluation: http://127.0.0.1:${port}`);
  console.log(`Recordings stay local: ${RECORDINGS_ROOT}`);
});

function parsePort(arguments_) {
  const index = arguments_.indexOf("--port");
  if (index === -1) return 4173;
  const value = Number.parseInt(arguments_[index + 1] ?? "", 10);
  if (!Number.isInteger(value) || value < 1024 || value > 65535) {
    throw new Error("--port must be an integer between 1024 and 65535");
  }
  return value;
}

function setSecurityHeaders(response) {
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  response.setHeader("X-Content-Type-Options", "nosniff");
  response.setHeader("Content-Security-Policy", "default-src 'self'; connect-src 'self'; media-src 'self' blob:; script-src 'self'; style-src 'self'");
}

async function sendStatic(request, response, filename) {
  const body = await readFile(join(ROOT, filename));
  response.writeHead(200, { "Content-Type": contentType(extname(filename)) });
  response.end(request.method === "HEAD" ? undefined : body);
}

function contentType(extension) {
  switch (extension) {
    case ".html": return "text/html; charset=utf-8";
    case ".js": return "text/javascript; charset=utf-8";
    case ".css": return "text/css; charset=utf-8";
    case ".json": return "application/json; charset=utf-8";
    default: return "application/octet-stream";
  }
}

async function sendStatus(response) {
  const recordings = {};
  for (const item of cases) {
    try {
      const metadata = JSON.parse(
        await readFile(join(RECORDINGS_ROOT, item.id, "metadata.json"), "utf8"),
      );
      const audio = await stat(join(RECORDINGS_ROOT, item.id, "recording.wav"));
      recordings[item.id] = {
        recordedAt: metadata.recordedAt,
        durationMs: metadata.durationMs,
        bytes: audio.size,
      };
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
  }
  sendJson(response, 200, { recordings });
}

async function sendProviders(response) {
  const result = await runCommand([
    "run", "-q", "-p", "template-cli", "--bin", "saymore-eval", "--",
    "providers", "--environment", "development",
  ]);
  if (result.status !== 0) {
    sendJson(response, 503, { error: "provider_discovery_failed" });
    return;
  }
  response.writeHead(200, { "Content-Type": "application/json; charset=utf-8" });
  response.end(result.stdout);
}

async function startRun(request, response) {
  const value = await readJsonBody(request);
  const caseIds = Array.isArray(value.caseIds) ? value.caseIds : [];
  const llmProviderIds = Array.isArray(value.llmProviderIds) ? value.llmProviderIds : [];
  if (value.confirmRemote !== true) {
    sendJson(response, 400, { error: "remote_confirmation_required" });
    return;
  }
  if (!caseIds.length || !llmProviderIds.length || typeof value.asrProviderId !== "string") {
    sendJson(response, 400, { error: "incomplete_run_selection" });
    return;
  }
  if (!caseIds.every((id) => casesById.has(id) && typeof id === "string")) {
    sendJson(response, 400, { error: "unknown_case" });
    return;
  }
  for (const caseId of caseIds) {
    try {
      await stat(join(RECORDINGS_ROOT, caseId, "recording.wav"));
    } catch {
      sendJson(response, 400, { error: "case_not_recorded", caseId });
      return;
    }
  }
  const discovery = await runCommand([
    "run", "-q", "-p", "template-cli", "--bin", "saymore-eval", "--",
    "providers", "--environment", "development",
  ]);
  if (discovery.status !== 0) {
    sendJson(response, 503, { error: "provider_discovery_failed" });
    return;
  }
  const providers = JSON.parse(discovery.stdout || "{}");
  const asr = providers.asr?.find((item) => item.id === value.asrProviderId && item.configured);
  const selectedLlms = llmProviderIds.map((id) =>
    providers.llm?.find((item) => item.id === id && item.configured && item.consented),
  );
  if (!asr || selectedLlms.some((item) => !item)) {
    sendJson(response, 400, { error: "provider_not_ready" });
    return;
  }

  const runId = `${new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14)}-${randomUUID().slice(0, 8)}`;
  const runDirectory = join(RUNS_ROOT, runId);
  const evaluationRequest = {
    run_id: runId,
    environment: "development",
    case_ids: caseIds,
    asr_provider_id: value.asrProviderId,
    llm_provider_ids: llmProviderIds,
    hotwords_enabled: value.hotwordsEnabled !== false,
    force_refinement: value.forceRefinement !== false,
  };
  await mkdir(runDirectory, { recursive: false });
  await writeJsonAtomic(join(runDirectory, "request.json"), evaluationRequest);
  await writeJsonAtomic(join(runDirectory, "status.json"), {
    runId,
    state: "running",
    startedAt: new Date().toISOString(),
    caseCount: caseIds.length,
    asr: { id: asr.id, name: asr.name, model: asr.model },
    llms: selectedLlms.map((item) => ({ id: item.id, name: item.name, model: item.model })),
  });
  launchRunner(runId, runDirectory);
  sendJson(response, 202, { runId });
}

function launchRunner(runId, runDirectory) {
  const log = createWriteStream(join(runDirectory, "runner.log"), { flags: "a", mode: 0o600 });
  const child = spawn("cargo", [
    "run", "-q", "-p", "template-cli", "--bin", "saymore-eval", "--",
    "run",
    "--request", join(runDirectory, "request.json"),
    "--manifest", join(ROOT, "cases.json"),
    "--recordings", RECORDINGS_ROOT,
    "--output", join(runDirectory, "result.json"),
  ], {
    cwd: WORKSPACE_ROOT,
    stdio: ["ignore", "ignore", "pipe"],
  });
  child.stderr.pipe(log);
  runningJobs.set(runId, child);
  child.on("error", async (error) => {
    runningJobs.delete(runId);
    log.end();
    const statusPath = join(runDirectory, "status.json");
    const status = await readJson(statusPath).catch(() => ({ runId }));
    await writeJsonAtomic(statusPath, {
      ...status,
      state: "failed",
      completedAt: new Date().toISOString(),
      error: error.message,
    });
  });
  child.on("close", async (code, signal) => {
    runningJobs.delete(runId);
    log.end();
    const statusPath = join(runDirectory, "status.json");
    const status = await readJson(statusPath).catch(() => ({ runId }));
    const cancelled = signal === "SIGTERM";
    await writeJsonAtomic(statusPath, {
      ...status,
      state: cancelled ? "cancelled" : code === 0 ? "completed" : "failed",
      completedAt: new Date().toISOString(),
      exitCode: code,
    });
  });
}

async function cancelRun(response, runId) {
  const child = runningJobs.get(runId);
  if (!child) {
    sendJson(response, 409, { error: "run_not_active" });
    return;
  }
  child.kill("SIGTERM");
  sendJson(response, 202, { cancelled: true });
}

async function sendRuns(response) {
  const entries = await readdir(RUNS_ROOT, { withFileTypes: true });
  const runs = [];
  for (const entry of entries) {
    if (!entry.isDirectory() || !/^[A-Za-z0-9_-]+$/.test(entry.name)) continue;
    const status = await readJson(join(RUNS_ROOT, entry.name, "status.json")).catch(() => null);
    if (status) runs.push(status);
  }
  runs.sort((left, right) => String(right.startedAt).localeCompare(String(left.startedAt)));
  sendJson(response, 200, { runs });
}

async function sendRun(response, runId) {
  const directory = join(RUNS_ROOT, runId);
  const status = await readJson(join(directory, "status.json")).catch(() => null);
  if (!status) {
    sendJson(response, 404, { error: "run_not_found" });
    return;
  }
  const progress = await readJson(join(directory, "progress.json")).catch(() => null);
  const result = await readJson(join(directory, "result.json")).catch(() => null);
  sendJson(response, 200, { status, progress, result });
}

async function saveRecording(request, response, caseId) {
  const item = casesById.get(caseId);
  if (!item) {
    sendJson(response, 404, { error: "unknown_case" });
    return;
  }
  if (request.headers["content-type"] !== "audio/wav") {
    sendJson(response, 415, { error: "wav_required" });
    return;
  }
  const durationMs = Number.parseInt(request.headers["x-duration-ms"] ?? "", 10);
  if (!Number.isInteger(durationMs) || durationMs < 500 || durationMs > 300_000) {
    sendJson(response, 400, { error: "invalid_duration" });
    return;
  }
  const audio = await readLimitedBody(request);
  if (audio.length < 44 || audio.subarray(0, 4).toString("ascii") !== "RIFF") {
    sendJson(response, 400, { error: "invalid_wav" });
    return;
  }

  const directory = join(RECORDINGS_ROOT, item.id);
  const audioPath = join(directory, "recording.wav");
  const metadataPath = join(directory, "metadata.json");
  await mkdir(directory, { recursive: true });
  await writeFile(`${audioPath}.tmp`, audio, { mode: 0o600 });
  await writeFile(
    `${metadataPath}.tmp`,
    `${JSON.stringify({
      caseId: item.id,
      category: item.category,
      title: item.title,
      read: item.read,
      expected: item.expected,
      check: item.check,
      recordedAt: new Date().toISOString(),
      durationMs,
      format: "wav-pcm16-mono",
      sampleRate: 16_000,
    }, null, 2)}\n`,
    { mode: 0o600 },
  );
  await rename(`${audioPath}.tmp`, audioPath);
  await rename(`${metadataPath}.tmp`, metadataPath);
  sendJson(response, 200, { saved: true, caseId: item.id });
}

async function sendRecording(response, caseId) {
  if (!casesById.has(caseId)) {
    sendJson(response, 404, { error: "unknown_case" });
    return;
  }
  try {
    const audio = await readFile(join(RECORDINGS_ROOT, caseId, "recording.wav"));
    response.writeHead(200, { "Content-Type": "audio/wav", "Content-Length": audio.length });
    response.end(audio);
  } catch (error) {
    if (error?.code === "ENOENT") {
      sendJson(response, 404, { error: "recording_not_found" });
      return;
    }
    throw error;
  }
}

async function readLimitedBody(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > MAX_AUDIO_BYTES) {
      const error = new Error("audio exceeds local recording limit");
      error.statusCode = 413;
      throw error;
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

async function readJsonBody(request) {
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > 64 * 1024) {
      const error = new Error("JSON request exceeds local limit");
      error.statusCode = 413;
      throw error;
    }
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

async function writeJsonAtomic(path, value) {
  const temporary = `${path}.tmp`;
  await writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  await rename(temporary, path);
}

function runCommand(arguments_) {
  return new Promise((resolve) => {
    const child = spawn("cargo", arguments_, { cwd: WORKSPACE_ROOT, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { if (stdout.length < 1024 * 1024) stdout += chunk; });
    child.stderr.on("data", (chunk) => { if (stderr.length < 1024 * 1024) stderr += chunk; });
    child.on("close", (status) => resolve({ status, stdout, stderr }));
    child.on("error", (error) => resolve({ status: -1, stdout, stderr: error.message }));
  });
}

function sendJson(response, status, value) {
  response.writeHead(status, { "Content-Type": "application/json; charset=utf-8" });
  response.end(JSON.stringify(value));
}
