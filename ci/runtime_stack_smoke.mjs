#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:net";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";

const PROBE_NAME = "issue_85_runtime_stack_probe";
const SERVER_URL_RE = /http:\/\/127\.0\.0\.1:\d+/;
const DEFAULT_CDP_TIMEOUT_MS = 180_000;

const base64Vlq = new Map(
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    .split("")
    .map((char, index) => [char, index]),
);

async function main() {
  const fastledBin = process.env.FASTLED_BIN || process.argv[2];
  if (!fastledBin) {
    throw new Error("FASTLED_BIN or first CLI arg must point to the fastled binary");
  }

  let tempDir;
  let serverChild;
  let chromeChild;
  let cdp;

  try {
    tempDir = await mkdtemp(path.join(os.tmpdir(), "fastled-runtime-stack-"));
    await runChecked(fastledBin, [
      "--init",
      "Blink",
      "--branch",
      "master",
      "--no-interactive",
      tempDir,
    ]);

    const sketchDir = path.join(tempDir, "Blink");
    const sketchPath = path.join(sketchDir, "Blink.ino");
    const patchedFrame = await patchBlinkSketch(sketchPath);

    await runChecked(fastledBin, [
      "--just-compile",
      "--debug",
      "--no-interactive",
      sketchDir,
    ]);

    const mapPath = path.join(sketchDir, "fastled_js", "fastled.wasm.map");
    const mapping = await findGeneratedWasmMapping(mapPath, patchedFrame);

    const serveDir = path.join(sketchDir, "fastled_js");
    const server = await startHeadlessServer(fastledBin, serveDir);
    serverChild = server.child;
    const serverUrl = server.url;

    const chromeBin = findChrome();
    const remoteDebuggingPort = await getFreePort();
    chromeChild = launchChrome(chromeBin, remoteDebuggingPort, tempDir);
    const wsUrl = await waitForChromeWebSocket(remoteDebuggingPort, chromeChild);

    cdp = await CdpConnection.connect(wsUrl);
    const evidence = await driveChromeToProbe(cdp, {
      appUrl: serverUrl,
      wasmUrl: `${serverUrl}/fastled.wasm`,
      mapping,
      timeoutMs: DEFAULT_CDP_TIMEOUT_MS,
    });

    if (evidence.kind === "breakpoint") {
      const frames = evidence.message.params.callFrames || [];
      if (!frames.some((frame) => frame.functionName?.includes(PROBE_NAME))) {
        throw new Error(
          `breakpoint stack did not contain ${PROBE_NAME}; frames: ${formatFunctionNames(frames)}`,
        );
      }
    }

    console.log(
      `Runtime stack smoke passed (${evidence.kind}): Blink.ino:${patchedFrame.line} ${PROBE_NAME} ` +
        `(wasm ${mapping.generatedLine + 1}:${mapping.generatedColumn})`,
    );
  } finally {
    if (cdp) {
      cdp.close();
    }
    await stopChild(chromeChild);
    await stopChild(serverChild);
    if (tempDir) {
      await rm(tempDir, { recursive: true, force: true });
    }
  }
}

async function patchBlinkSketch(sketchPath) {
  const original = (await readFile(sketchPath, "utf8")).replace(/\r\n/g, "\n");
  if (original.includes(PROBE_NAME)) {
    throw new Error(`${path.basename(sketchPath)} already contains ${PROBE_NAME}`);
  }

  const setupIndex = original.indexOf("void setup(");
  if (setupIndex === -1) {
    throw new Error("could not find setup() in Blink.ino");
  }

  const helper = [
    'extern "C" __attribute__((noinline, used))',
    `void ${PROBE_NAME}() {`,
    "    __builtin_trap();",
    "}",
    "",
  ].join("\n");

  let patched = `${original.slice(0, setupIndex)}${helper}${original.slice(setupIndex)}`;
  const loopMarker = "void loop() {";
  const loopIndex = patched.indexOf(loopMarker);
  if (loopIndex === -1) {
    throw new Error("could not find loop() in Blink.ino");
  }

  const callOnce = [
    "",
    `    static bool ${PROBE_NAME}_ran = false;`,
    `    if (!${PROBE_NAME}_ran) {`,
    `        ${PROBE_NAME}_ran = true;`,
    `        ${PROBE_NAME}();`,
    "    }",
  ].join("\n");

  patched = `${patched.slice(0, loopIndex + loopMarker.length)}${callOnce}${patched.slice(
    loopIndex + loopMarker.length,
  )}`;
  await writeFile(sketchPath, patched, "utf8");

  const lines = patched.split("\n");
  const trapIndex = lines.findIndex((line) => line.includes("__builtin_trap();"));
  if (trapIndex === -1) {
    throw new Error("patched Blink.ino did not contain the trap line");
  }

  return {
    file: sketchPath,
    line: trapIndex + 1,
    column: lines[trapIndex].indexOf("__builtin_trap"),
  };
}

async function findGeneratedWasmMapping(mapPath, patchedFrame) {
  const sourceMap = JSON.parse(await readFile(mapPath, "utf8"));
  if (!Array.isArray(sourceMap.sources) || typeof sourceMap.mappings !== "string") {
    throw new Error(`${mapPath} is not a version 3 source map with sources and mappings`);
  }

  const sourceIndexes = new Set();
  for (const [index, source] of sourceMap.sources.entries()) {
    if (isBlinkSource(source)) {
      sourceIndexes.add(index);
    }
  }
  if (sourceIndexes.size === 0) {
    throw new Error(`could not find Blink.ino in ${mapPath} sources`);
  }

  const mappings = decodeSourceMapMappings(sourceMap.mappings);
  const originalLine = patchedFrame.line - 1;
  const candidates = mappings.filter(
    (mapping) => sourceIndexes.has(mapping.sourceIndex) && mapping.originalLine === originalLine,
  );
  if (candidates.length === 0) {
    const blinkSources = [...sourceIndexes].map((index) => sourceMap.sources[index]).join(", ");
    throw new Error(
      `could not map Blink.ino:${patchedFrame.line} from ${mapPath}; Blink sources: ${blinkSources}`,
    );
  }

  candidates.sort((a, b) => {
    const columnDelta = Math.abs(a.originalColumn - patchedFrame.column) -
      Math.abs(b.originalColumn - patchedFrame.column);
    if (columnDelta !== 0) {
      return columnDelta;
    }
    if (a.generatedLine !== b.generatedLine) {
      return a.generatedLine - b.generatedLine;
    }
    return a.generatedColumn - b.generatedColumn;
  });

  return {
    ...candidates[0],
    sourceLine: patchedFrame.line,
    sourceColumn: candidates[0].originalColumn,
  };
}

function isBlinkSource(source) {
  const normalized = source.replace(/\\/g, "/");
  return normalized === "Blink.ino" || normalized.endsWith("/Blink.ino");
}

function decodeSourceMapMappings(mappingsText) {
  const decoded = [];
  let sourceIndex = 0;
  let originalLine = 0;
  let originalColumn = 0;
  let nameIndex = 0;

  const lines = mappingsText.split(";");
  for (let generatedLine = 0; generatedLine < lines.length; generatedLine += 1) {
    let generatedColumn = 0;
    const segments = lines[generatedLine].split(",");
    for (const segment of segments) {
      if (!segment) {
        continue;
      }

      const fields = decodeVlqSegment(segment);
      generatedColumn += fields[0];
      if (fields.length < 4) {
        continue;
      }

      sourceIndex += fields[1];
      originalLine += fields[2];
      originalColumn += fields[3];
      if (fields.length >= 5) {
        nameIndex += fields[4];
      }

      decoded.push({
        generatedLine,
        generatedColumn,
        sourceIndex,
        originalLine,
        originalColumn,
        nameIndex: fields.length >= 5 ? nameIndex : undefined,
      });
    }
  }

  return decoded;
}

function decodeVlqSegment(segment) {
  const values = [];
  let value = 0;
  let shift = 0;

  for (const char of segment) {
    const digit = base64Vlq.get(char);
    if (digit === undefined) {
      throw new Error(`invalid source-map VLQ character '${char}'`);
    }

    const continuation = (digit & 32) !== 0;
    value += (digit & 31) * 2 ** shift;
    if (continuation) {
      shift += 5;
      continue;
    }

    const sign = value & 1;
    values.push(sign ? -(value >> 1) : value >> 1);
    value = 0;
    shift = 0;
  }

  if (shift !== 0) {
    throw new Error(`unterminated source-map VLQ segment '${segment}'`);
  }

  return values;
}

async function startHeadlessServer(fastledBin, serveDir) {
  const child = spawn(fastledBin, ["--internal-serve-dir-headless", serveDir], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";

  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });

  try {
    const url = await waitForCondition(
      () => {
        const match = stdout.match(SERVER_URL_RE);
        return match?.[0];
      },
      30_000,
      "server URL",
      child,
    );
    return { child, url };
  } catch (error) {
    await stopChild(child);
    error.message += `\nserver stdout:\n${tail(stdout)}\nserver stderr:\n${tail(stderr)}`;
    throw error;
  }
}

function launchChrome(chromeBin, remoteDebuggingPort, tempDir) {
  const userDataDir = path.join(tempDir, "chrome-profile");
  const child = spawn(chromeBin, [
    "--headless=new",
    `--remote-debugging-port=${remoteDebuggingPort}`,
    "--remote-debugging-address=127.0.0.1",
    "--no-sandbox",
    "--disable-gpu",
    "--disable-dev-shm-usage",
    "--enable-features=WebAssemblyDWARFDebugging",
    `--user-data-dir=${userDataDir}`,
    "about:blank",
  ], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  return child;
}

async function waitForChromeWebSocket(port, chromeChild) {
  const endpoint = `http://127.0.0.1:${port}/json/version`;
  return waitForCondition(
    async () => {
      const response = await fetchWithTimeout(endpoint, 1_000).catch(() => undefined);
      if (!response?.ok) {
        return undefined;
      }
      const json = await response.json();
      return json.webSocketDebuggerUrl;
    },
    30_000,
    "Chrome DevTools websocket",
    chromeChild,
  );
}

async function driveChromeToProbe(cdp, options) {
  const { appUrl, wasmUrl, mapping, timeoutMs } = options;
  const breakpointIds = new Set();
  const targetSessions = new Map();
  const instrumentation = new Map();
  let instrumentationError;

  cdp.onEvent((message) => {
    if (message.method !== "Target.attachedToTarget") {
      return;
    }
    const { sessionId, targetInfo } = message.params;
    targetSessions.set(targetInfo.targetId, sessionId);
    const promise = instrumentSession(cdp, sessionId, targetInfo, wasmUrl, mapping, breakpointIds)
      .catch((error) => {
        instrumentationError = error;
      });
    instrumentation.set(sessionId, promise);
  });

  const pausedPromise = cdp.waitForEvent((message) => {
    if (message.method !== "Debugger.paused") {
      return false;
    }
    const hitBreakpoints = message.params.hitBreakpoints || [];
    return hitBreakpoints.some((id) => breakpointIds.has(id));
  }, timeoutMs, `${PROBE_NAME} breakpoint`).then((message) => ({
    kind: "breakpoint",
    message,
  }));

  const exceptionPromise = cdp.waitForEvent((message) => {
    if (message.method !== "Runtime.exceptionThrown") {
      return false;
    }
    return isWasmRuntimeException(message.params?.exceptionDetails);
  }, timeoutMs, `${PROBE_NAME} runtime exception`).then((message) => ({
    kind: "runtime exception",
    message,
  }));

  await cdp.send("Target.setAutoAttach", {
    autoAttach: true,
    waitForDebuggerOnStart: true,
    flatten: true,
  });
  await cdp.send("Target.setDiscoverTargets", { discover: true });

  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const pageSessionId = await waitForCondition(
    () => targetSessions.get(targetId),
    30_000,
    "page target attachment",
  );

  await instrumentation.get(pageSessionId);
  if (instrumentationError) {
    throw instrumentationError;
  }

  await cdp.send("Page.navigate", { url: appUrl }, pageSessionId);
  const evidence = await Promise.race([pausedPromise, exceptionPromise]);
  if (instrumentationError) {
    throw instrumentationError;
  }
  return evidence;
}

async function instrumentSession(cdp, sessionId, targetInfo, wasmUrl, mapping, breakpointIds) {
  await cdp.send("Runtime.enable", {}, sessionId);
  await cdp.send("Debugger.enable", {}, sessionId);
  const breakpoint = await cdp.send("Debugger.setBreakpointByUrl", {
    url: wasmUrl,
    lineNumber: mapping.generatedLine,
    columnNumber: mapping.generatedColumn,
  }, sessionId);
  breakpointIds.add(breakpoint.breakpointId);

  if (targetInfo.type === "page" || targetInfo.type === "iframe") {
    await cdp.send("Page.enable", {}, sessionId).catch(() => {});
    await cdp.send("Target.setAutoAttach", {
      autoAttach: true,
      waitForDebuggerOnStart: true,
      flatten: true,
    }, sessionId).catch(() => {});
  }

  await cdp.send("Runtime.runIfWaitingForDebugger", {}, sessionId).catch(async () => {
    await cdp.send("Debugger.resume", {}, sessionId).catch(() => {});
  });
}

class CdpConnection {
  constructor(webSocket) {
    this.webSocket = webSocket;
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Set();

    webSocket.addEventListener("message", (event) => this.handleMessage(event.data));
    webSocket.addEventListener("close", () => this.rejectAll(new Error("CDP websocket closed")));
    webSocket.addEventListener("error", () => this.rejectAll(new Error("CDP websocket error")));
  }

  static async connect(wsUrl) {
    if (typeof WebSocket !== "function") {
      throw new Error("Node global WebSocket is unavailable; Node 22 is required");
    }

    const webSocket = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("timed out opening CDP websocket")), 30_000);
      webSocket.addEventListener("open", () => {
        clearTimeout(timeout);
        resolve();
      }, { once: true });
      webSocket.addEventListener("error", () => {
        clearTimeout(timeout);
        reject(new Error("failed to open CDP websocket"));
      }, { once: true });
    });
    return new CdpConnection(webSocket);
  }

  send(method, params = {}, sessionId = undefined) {
    const id = this.nextId;
    this.nextId += 1;
    const message = { id, method, params };
    if (sessionId) {
      message.sessionId = sessionId;
    }

    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.webSocket.send(JSON.stringify(message));
    });
  }

  onEvent(listener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  waitForEvent(predicate, timeoutMs, label) {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        unsubscribe();
        reject(new Error(`timed out waiting for ${label}`));
      }, timeoutMs);

      const unsubscribe = this.onEvent((message) => {
        if (!predicate(message)) {
          return;
        }
        clearTimeout(timeout);
        unsubscribe();
        resolve(message);
      });
    });
  }

  handleMessage(rawData) {
    const text = typeof rawData === "string" ? rawData : Buffer.from(rawData).toString("utf8");
    const message = JSON.parse(text);
    if (message.id) {
      const pending = this.pending.get(message.id);
      if (!pending) {
        return;
      }
      this.pending.delete(message.id);
      if (message.error) {
        pending.reject(new Error(`${message.error.message}: ${JSON.stringify(message.error)}`));
      } else {
        pending.resolve(message.result || {});
      }
      return;
    }

    for (const listener of this.listeners) {
      listener(message);
    }
  }

  rejectAll(error) {
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }

  close() {
    try {
      this.webSocket.close();
    } catch {
      // Best-effort cleanup.
    }
  }
}

async function runChecked(command, args) {
  const result = await runProcess(command, args);
  if (result.code !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} exited with ${result.code}\n` +
        `stdout:\n${tail(result.stdout)}\nstderr:\n${tail(result.stderr)}`,
    );
  }
}

function runProcess(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

function findChrome() {
  const candidates = [
    process.env.FASTLED_CHROME,
    process.env.CHROME_BIN,
    "google-chrome-stable",
    "google-chrome",
    "chromium",
    "chromium-browser",
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (candidate.includes(path.sep) && !existsSync(candidate)) {
      continue;
    }
    const result = spawnSync(candidate, ["--version"], { stdio: "ignore" });
    if (result.status === 0) {
      return candidate;
    }
  }

  throw new Error(`could not find Chrome; tried ${candidates.join(", ")}`);
}

function getFreePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = address.port;
      server.close(() => resolve(port));
    });
    server.on("error", reject);
  });
}

async function waitForCondition(check, timeoutMs, label, child = undefined) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (child && child.exitCode !== null) {
      throw new Error(`${label} was not available before process exited with ${child.exitCode}`);
    }

    const value = await check();
    if (value) {
      return value;
    }
    await sleep(100);
  }
  throw new Error(`timed out waiting for ${label}`);
}

async function fetchWithTimeout(url, timeoutMs) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, { signal: controller.signal });
  } finally {
    clearTimeout(timeout);
  }
}

async function stopChild(child) {
  if (!child || child.exitCode !== null) {
    return;
  }

  child.kill("SIGTERM");
  const closed = new Promise((resolve) => child.once("close", resolve));
  await Promise.race([closed, sleep(2_000)]);
  if (child.exitCode === null) {
    child.kill("SIGKILL");
    await Promise.race([closed, sleep(2_000)]);
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function isWasmRuntimeException(exceptionDetails) {
  const text = JSON.stringify(exceptionDetails || {});
  return /WebAssembly|RuntimeError|unreachable|trap|abort/i.test(text);
}

function formatFunctionNames(frames) {
  return frames.map((frame) => frame.functionName || "<anonymous>").join(", ");
}

function tail(text, maxLength = 4_000) {
  return text.length > maxLength ? text.slice(text.length - maxLength) : text;
}

main().catch((error) => {
  console.error(`Runtime stack smoke failed: ${error.message}`);
  process.exitCode = 1;
});
