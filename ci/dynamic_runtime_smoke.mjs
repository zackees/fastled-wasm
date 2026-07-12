#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:net";
import { mkdtemp, rm, unlink, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";

const SERVER_URL_RE = /http:\/\/127\.0\.0\.1:\d+/;

async function main() {
  const fastledBin = process.env.FASTLED_BIN || process.argv[2];
  const outputDir = process.env.FASTLED_OUTPUT_DIR || process.argv[3];
  const noApp = process.argv.includes("--no-app");
  if (!fastledBin || !outputDir) {
    throw new Error("usage: dynamic_runtime_smoke.mjs <fastled-bin> <fastled-output-dir>");
  }
  const expectedAssetManifest = noApp
    ? undefined
    : existsSync(path.join(outputDir, "sketch_assets.json"))
      ? "sketch_assets.json"
      : existsSync(path.join(outputDir, "files.json"))
        ? "files.json"
        : undefined;
  if (!noApp && !expectedAssetManifest) {
    throw new Error("app smoke requires sketch_assets.json or legacy files.json");
  }

  let profileDir;
  let server;
  let chrome;
  let cdp;
  let noAppHost;
  try {
    if (noApp) {
      noAppHost = path.join(outputDir, "__dynamic_smoke.html");
      await writeFile(noAppHost, `<!doctype html>
<meta charset="utf-8">
<script src="fastled.js"></script>
<script>
globalThis.__dynamicSmoke = { ready: false };
fastled().then(() => { globalThis.__dynamicSmoke.ready = true; }).catch((error) => {
  globalThis.__dynamicSmoke.error = String(error?.stack || error);
  console.error("no-app FastLED startup failed", error);
});
</script>`, "utf8");
    }
    profileDir = await mkdtemp(path.join(os.tmpdir(), "fastled-dynamic-smoke-"));
    const served = await startServer(fastledBin, outputDir);
    server = served.child;
    console.log(`Dynamic smoke server: ${served.url}`);
    const port = await getFreePort();
    chrome = launchChrome(findChrome(), port, profileDir);
    const webSocketUrl = await waitFor(async () => {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`).catch(() => undefined);
      return response?.ok ? (await response.json()).webSocketDebuggerUrl : undefined;
    }, 30_000, "Chrome debugging endpoint");
    cdp = await CdpConnection.connect(webSocketUrl);
    console.log("Dynamic smoke connected to Chrome");

    const sessionsByTarget = new Map();
    const exceptions = [];
    const errorLogs = [];
    const allLogs = [];
    const requests = [];
    const responses = new Map();
    cdp.onEvent((message) => {
      if (message.method === "Target.attachedToTarget") {
        const { sessionId, targetInfo } = message.params;
        sessionsByTarget.set(targetInfo.targetId, sessionId);
        instrument(cdp, sessionId, targetInfo.type).catch((error) => exceptions.push(error.message));
      } else if (message.method === "Runtime.exceptionThrown") {
        exceptions.push(message.params.exceptionDetails?.exception?.description || message.params.exceptionDetails?.text || "exception");
      } else if (message.method === "Runtime.consoleAPICalled" && message.params.type === "error") {
        const text = (message.params.args || []).map(formatRemoteObject).join(" ");
        errorLogs.push(text);
        allLogs.push(`error: ${text}`);
      } else if (message.method === "Runtime.consoleAPICalled") {
        allLogs.push(`${message.params.type}: ${(message.params.args || []).map(formatRemoteObject).join(" ")}`);
      } else if (message.method === "Network.requestWillBeSent") {
        requests.push(message.params.request.url);
      } else if (message.method === "Network.responseReceived") {
        responses.set(message.params.response.url, message.params.response.status);
      }
    });

    await cdp.send("Target.setAutoAttach", {
      autoAttach: true,
      waitForDebuggerOnStart: false,
      flatten: true,
    });
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const pageSession = await waitFor(
      () => sessionsByTarget.get(targetId),
      30_000,
      "page target attachment",
    );
    console.log("Dynamic smoke attached to page");
    await cdp.send("Page.enable", {}, pageSession);
    const pageUrl = noApp ? `${served.url}/__dynamic_smoke.html` : `${served.url}/?gfx=0`;
    await cdp.send("Page.navigate", { url: pageUrl }, pageSession);

    let lastState;
    let state;
    try {
      state = await waitFor(async () => {
        const result = await cdp.send("Runtime.evaluate", {
          expression: noApp ? `(() => ({
            readyState: document.readyState,
            ready: !!globalThis.__dynamicSmoke?.ready,
            error: globalThis.__dynamicSmoke?.error || "",
          }))()` : `(() => ({
            readyState: document.readyState,
            hasController: !!globalThis.fastLEDController,
            setupCompleted: !!globalThis.fastLEDController?.setupCompleted,
            workerActive: !!globalThis.fastLEDWorkerManager?.isWorkerActive,
            bodyText: document.body?.innerText?.slice(0, 500) || "",
          }))()`,
          returnByValue: true,
        }, pageSession);
        const value = result.result?.value;
        lastState = value;
        return noApp
          ? (value?.ready ? value : undefined)
          : (value?.setupCompleted && value?.workerActive ? value : undefined);
      }, 30_000, "FastLED dynamic runtime startup");
    } catch (error) {
      error.message += `\nlast state: ${JSON.stringify(lastState)}`;
      error.message += `\nexceptions: ${exceptions.join(" | ") || "(none)"}`;
      error.message += `\nconsole errors: ${errorLogs.join(" | ") || "(none)"}`;
      error.message += `\nconsole tail:\n${allLogs.slice(-100).join("\n")}`;
      error.message += `\nrequests: ${requests.slice(-30).join(", ")}`;
      throw error;
    }

    const sketchRequest = requests.find((url) => /\/sketch\.wasm(?:[?#]|$)/.test(url));
    if (!sketchRequest) {
      throw new Error(`browser did not request sketch.wasm; requests: ${requests.join(", ")}`);
    }
    if (expectedAssetManifest) {
      const manifestResponse = [...responses.entries()].find(([url]) =>
        url.endsWith(`/${expectedAssetManifest}`)
      );
      if (!manifestResponse || manifestResponse[1] !== 200) {
        throw new Error(
          `browser did not load ${expectedAssetManifest} successfully; responses: ${JSON.stringify([...responses])}`,
        );
      }
    }
    if (exceptions.length || errorLogs.length) {
      throw new Error(
        `browser errors after startup:\n${[...exceptions, ...errorLogs].join("\n")}`,
      );
    }
    console.log(
      noApp
        ? "Dynamic no-app smoke passed: sketch.wasm loaded; FastLED factory resolved"
        : `Dynamic runtime smoke passed: sketch.wasm loaded; setup=${state.setupCompleted}; worker=${state.workerActive}`,
    );
  } finally {
    cdp?.close();
    await stopChild(chrome);
    await stopChild(server);
    if (noAppHost) {
      await unlink(noAppHost).catch(() => {});
    }
    if (profileDir) {
      await rm(profileDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 })
        .catch(() => {});
    }
  }
}

async function instrument(cdp, sessionId, targetType) {
  await cdp.send("Runtime.enable", {}, sessionId);
  await cdp.send("Network.enable", {}, sessionId).catch(() => {});
  if (targetType === "page" || targetType === "iframe") {
    await cdp.send("Target.setAutoAttach", {
      autoAttach: true,
      waitForDebuggerOnStart: false,
      flatten: true,
    }, sessionId).catch(() => {});
  }
}

async function startServer(fastledBin, outputDir) {
  const child = spawn(fastledBin, ["--internal-serve-dir-headless", outputDir], {
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, FASTLED_MANAGED_RUNTIME: "1" },
  });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  try {
    const url = await waitFor(() => stdout.match(SERVER_URL_RE)?.[0], 30_000, "server URL");
    return { child, url };
  } catch (error) {
    await stopChild(child);
    error.message += `\nserver stdout:\n${stdout}\nserver stderr:\n${stderr}`;
    throw error;
  }
}

function findChrome() {
  const candidates = [
    process.env.FASTLED_CHROME,
    process.env.CHROME_BIN,
    process.env.PROGRAMFILES && path.join(process.env.PROGRAMFILES, "Google/Chrome/Application/chrome.exe"),
    process.env["PROGRAMFILES(X86)"] && path.join(process.env["PROGRAMFILES(X86)"], "Google/Chrome/Application/chrome.exe"),
    "google-chrome-stable",
    "google-chrome",
    "chromium",
  ].filter(Boolean);
  for (const candidate of candidates) {
    if ((candidate.includes(path.sep) || path.isAbsolute(candidate)) && !existsSync(candidate)) {
      continue;
    }
    if (spawnSync(candidate, ["--version"], { stdio: "ignore" }).status === 0) {
      return candidate;
    }
  }
  throw new Error(`could not find Chrome; tried ${candidates.join(", ")}`);
}

function launchChrome(binary, port, profileDir) {
  return spawn(binary, [
    "--headless=new",
    `--remote-debugging-port=${port}`,
    "--remote-debugging-address=127.0.0.1",
    "--no-sandbox",
    "--disable-gpu",
    "--disable-dev-shm-usage",
    `--user-data-dir=${path.join(profileDir, "chrome-profile")}`,
    "about:blank",
  ], { stdio: "ignore" });
}

class CdpConnection {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Set();
    socket.addEventListener("message", (event) => this.handle(event.data));
  }

  static async connect(url) {
    const socket = new WebSocket(url);
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", () => reject(new Error("failed to connect to Chrome")), { once: true });
    });
    return new CdpConnection(socket);
  }

  send(method, params = {}, sessionId = undefined) {
    const id = this.nextId++;
    const message = { id, method, params };
    if (sessionId) message.sessionId = sessionId;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP command timed out: ${method}`));
      }, 15_000);
      this.pending.set(id, {
        resolve: (value) => { clearTimeout(timeout); resolve(value); },
        reject: (error) => { clearTimeout(timeout); reject(error); },
      });
      this.socket.send(JSON.stringify(message));
    });
  }

  onEvent(listener) {
    this.listeners.add(listener);
  }

  handle(raw) {
    const message = JSON.parse(typeof raw === "string" ? raw : Buffer.from(raw).toString("utf8"));
    if (message.id) {
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      if (message.error) pending.reject(new Error(message.error.message));
      else pending.resolve(message.result || {});
      return;
    }
    for (const listener of this.listeners) listener(message);
  }

  close() {
    this.socket.close();
  }
}

function formatRemoteObject(object) {
  if (Object.hasOwn(object || {}, "value")) return String(object.value);
  return object?.description || object?.type || "";
}

async function getFreePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      server.close(() => resolve(port));
    });
  });
}

async function waitFor(check, timeoutMs, label) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const value = await check();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`timed out waiting for ${label}`);
}

async function stopChild(child) {
  if (!child || child.exitCode !== null) return;
  if (process.platform === "win32" && child.pid) {
    const killed = spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
      stdio: "ignore",
      timeout: 10_000,
    });
    if (killed.error?.code === "ETIMEDOUT") child.kill();
    return;
  }
  child.kill("SIGTERM");
  await Promise.race([
    new Promise((resolve) => child.once("close", resolve)),
    new Promise((resolve) => setTimeout(resolve, 2_000)),
  ]);
  if (child.exitCode === null) child.kill("SIGKILL");
}

main().catch((error) => {
  console.error(`Dynamic runtime smoke failed: ${error.message}`);
  process.exitCode = 1;
});
