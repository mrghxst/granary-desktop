#!/usr/bin/env node
// Phase 8: time to first frame (docs/perf.md). Drives a running debug Granary (mock server, real libmpv, real test
// clips) over the WebView2 DevTools protocol, plays the SDR and the HDR10 clip -Runs times each with display switching
// off (HDR mode "off", refresh "never"; the display switch cost is measured by display_roundtrip), and reads the host's
// own "file loaded since_play_ms" and "first frame time_to_first_frame_ms" log lines (play request -> first frame).
//
// Start the app first (PowerShell):
//   $env:GRANARY_MOCK_SERVER = "1"; $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
//   pnpm --filter desktop tauri dev
// then: node scripts/measure-ttff.mjs [outDir] [port] [runs]
// Local only: talks to 127.0.0.1. Test values of the in-process mock (alice / pw) are used for sign-in.
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const outDir = process.argv[2] ?? "docs/evidence/phase-8";
const port = Number(process.argv[3] ?? 9333);
mkdirSync(outDir, { recursive: true });
const startedAt = new Date();

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(...a);

async function target() {
  for (let i = 0; i < 120; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find((t) => t.type === "page" && /127\.0\.0\.1:1420|tauri\.localhost/.test(t.url));
      if (page) return page.webSocketDebuggerUrl;
    } catch {
      /* not up yet */
    }
    await sleep(1000);
  }
  throw new Error(`no Granary page on DevTools port ${port}`);
}

const ws = new WebSocket(await target());
await new Promise((r, j) => {
  ws.onopen = r;
  ws.onerror = j;
});
let nextId = 1;
const pending = new Map();
ws.onmessage = (m) => {
  const msg = JSON.parse(String(m.data));
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) reject(new Error(JSON.stringify(msg.error)));
    else resolve(msg.result);
  }
};
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });

async function evaluate(expression) {
  const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(`${expression.slice(0, 80)}: ${JSON.stringify(r.exceptionDetails)}`);
  return r.result.value;
}

const HELPERS = `window.__g = {
  control(label) {
    const byAria = document.querySelector('[aria-label="' + label + '"]');
    if (byAria) return byAria;
    const l = [...document.querySelectorAll("label")].find((x) => x.textContent.trim().startsWith(label));
    if (!l) return null;
    return l.htmlFor ? document.getElementById(l.htmlFor) : l.querySelector("input,select,textarea");
  },
  fill(label, value) {
    const el = this.control(label);
    if (!el) throw new Error("no control " + label);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    return true;
  },
  find(sel, text) {
    return [...document.querySelectorAll(sel)].find((e) => (e.getAttribute("aria-label") ?? e.textContent).trim().match(text));
  },
  click(sel, text) {
    const el = this.find(sel, text);
    if (!el) throw new Error("nothing to click: " + sel + " " + text);
    el.click();
    return true;
  },
  invoke(cmd, args) {
    return window.__TAURI_INTERNALS__.invoke(cmd, args ?? {});
  },
};`;
const reinject = () => evaluate(HELPERS + "; true");

async function waitFor(expression, what, timeout = 20000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    try {
      if (await evaluate(expression)) return;
    } catch {
      /* page navigating */
    }
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${what}`);
}

const checks = [];
function check(name, ok, detail = "") {
  checks.push({ name, ok, detail });
  log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? ` (${detail})` : ""}`);
  if (!ok) {
    writeFileSync(join(outDir, "checks.json"), JSON.stringify(checks, null, 2));
    process.exit(1);
  }
}

async function shot(name) {
  await sleep(300);
  const { data } = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(outDir, `${name}.png`), Buffer.from(data, "base64"));
  log(`  saved ${name}.png`);
}

const VK = { " ": 0x20, ArrowRight: 0x27, i: 0x49, q: 0x51, N: 0x4e, "?": 0xbf, Escape: 0x1b };
async function key(k, shift = false) {
  const base = { key: k, code: k, windowsVirtualKeyCode: VK[k] ?? 0, modifiers: shift ? 8 : 0 };
  await send("Input.dispatchKeyEvent", { type: "keyDown", ...base, text: k.length === 1 ? k : undefined });
  await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
}
/** Mouse movement over the page: the overlay is shown again (it may have auto-hidden). */
async function wiggle() {
  await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 400, y: 300 });
  await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 420, y: 320 });
  await sleep(300);
}
const state = () => evaluate(`__g.invoke("player_state")`);

await send("Page.enable");
await send("Runtime.enable");
await evaluate(`location.hash = "#/"; true`);
await sleep(500);
await reinject();

// Connect to the in-process mock server (or update the saved entry), sign in if asked, pick the profile.
await evaluate(`location.hash = "#/servers/add"; true`);
await waitFor(`!!window.__g.control("Server address")`, "add-server form");
await evaluate(`__g.fill("Server address", "media.example.test")`);
await evaluate(`__g.click("button", /^Check$/)`);
await waitFor(`!!__g.find("button", /^(Add server|Update address)$/)`, "probe result");
await evaluate(`__g.click("button", /^(Add server|Update address)$/)`);
await waitFor(
  `!!__g.control("Username") || !!__g.find("button", /Alice/) || location.hash.includes("/home")`,
  "sign-in, profiles or home",
);
if (await evaluate(`!!__g.control("Username")`)) {
  await evaluate(`__g.fill("Username", "alice") && __g.fill("Password", "pw")`);
  await evaluate(`__g.click("button", /^Sign in$/)`);
  await waitFor(`!!__g.find("button", /Alice/) || location.hash.includes("/home")`, "profile picker");
}
if (await evaluate(`!!__g.find("button", /Alice/)`)) await evaluate(`__g.click("button", /Alice/)`);
await waitFor(`location.hash.includes("/home")`, "home");
const serverPath = await evaluate(`location.hash.replace(/\\/home.*$/, "")`);
await reinject();

// ---- Time to first frame ----
const runs = Number(process.argv[4] ?? 5);
const originalSettings = await evaluate(`__g.invoke("settings_get")`);
await evaluate(
  `__g.invoke("settings_get").then((s) => __g.invoke("settings_set", { settings: { ...s, display: { ...s.display, hdrMode: "off", refresh: "never" } } }))`,
);
const plays = [];
for (const [id, clip] of [
  ["m2", "SDR H.264 1080p25"],
  ["m1", "HDR10 HEVC Main10 2160p23.976"],
]) {
  for (let i = 1; i <= runs; i++) {
    await evaluate(`location.hash = ${JSON.stringify(`${serverPath.slice(1)}/item/${id}`)}; true`);
    await waitFor(`!!document.querySelector("h1")`, `movie ${id}`);
    await reinject();
    await waitFor(`!!__g.find("button", /^(From the beginning|Play)$/)`, "play button");
    await evaluate(`__g.click("button", /^(From the beginning|Play)$/)`);
    await waitFor(`__g.invoke("player_state").then((s) => s.phase === "playing")`, "playing", 40000);
    await sleep(1500);
    await wiggle();
    await key("q");
    await waitFor(`!document.querySelector('[data-testid="player-view"]')`, "player closed", 20000);
    await sleep(1000);
    plays.push({ clip, run: i });
    log(`played ${clip} #${i}`);
  }
}
await evaluate(`__g.invoke("settings_set", { settings: ${JSON.stringify(originalSettings)} })`);

// The host's own measurements, in play order.
const logDir = join(process.env.LOCALAPPDATA ?? "", "Granary", "logs");
const lines = readdirSync(logDir)
  .map((f) => join(logDir, f))
  .filter((f) => statSync(f).mtime >= startedAt)
  .flatMap((f) => readFileSync(f, "utf8").split(/\r?\n/))
  .filter((l) => Date.parse(l.slice(0, 30).split(" ")[0]) >= startedAt.getTime() - 1000);
const loaded = lines.map((l) => /file loaded since_play_ms=(\d+)/.exec(l)).filter(Boolean).map((m) => Number(m[1]));
const first = lines.map((l) => /first frame time_to_first_frame_ms=(\d+)/.exec(l)).filter(Boolean).map((m) => Number(m[1]));
check("one first-frame measurement per play", first.length === plays.length && loaded.length === plays.length, `${first.length} first frames, ${loaded.length} loads, ${plays.length} plays`);
plays.forEach((p, i) => {
  p.fileLoadedMs = loaded[i];
  p.firstFrameMs = first[i];
});
const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor((s.length - 1) / 2)];
};
const summary = {};
for (const clip of [...new Set(plays.map((p) => p.clip))]) {
  const ps = plays.filter((p) => p.clip === clip);
  summary[clip] = {
    firstPlayMs: ps[0].firstFrameMs,
    medianFirstFrameMs: median(ps.map((p) => p.firstFrameMs)),
    medianFileLoadedMs: median(ps.map((p) => p.fileLoadedMs)),
    maxFirstFrameMs: Math.max(...ps.map((p) => p.firstFrameMs)),
  };
}
const out = { build: "debug (tauri dev), in-process mock server, loopback relay", measuredAt: new Date().toISOString(), runs: plays, summary };
writeFileSync(join(outDir, "ttff.json"), JSON.stringify(out, null, 2));
for (const [clip, s] of Object.entries(summary)) {
  log(`TTFF ${clip}: median ${s.medianFirstFrameMs} ms (file loaded ${s.medianFileLoadedMs} ms), first play ${s.firstPlayMs} ms, max ${s.maxFirstFrameMs} ms`);
}
ws.close();
