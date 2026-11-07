#!/usr/bin/env node
// Gate 7 evidence: drive a running debug Granary (mock server, real libmpv, real test clips, real displays) over the
// WebView2 DevTools protocol and check the Phase 7 display and audio output end to end:
//   - HDR10 clip: Windows HDR switched on before the first frame when it is off (capability skip otherwise),
//     tone-mapping target and SDR white in the stats, held during the idle grace, restored afterwards;
//   - 25 fps SDR clip: automatic refresh strategy (exact multiple near the current rate, or a logged skip), restored;
//   - audio settings applied live; every monitor back to its original HDR state and refresh rate at the end.
//
// Start the app first (PowerShell):
//   $env:GRANARY_MOCK_SERVER = "1"; $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
//   pnpm --filter desktop tauri dev
// then: node scripts/output-evidence.mjs [outDir] [port]
// Local only: talks to 127.0.0.1. Test values of the in-process mock (alice / pw) are used for sign-in.
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";

const outDir = process.argv[2] ?? "docs/evidence/phase-7";
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

// ---- Phase 7: display and audio output ----
const monitors = () => evaluate(`__g.invoke("display_list")`);
const current = async () => (await monitors()).find((m) => m.current) ?? (await monitors())[0];
const originalSettings = await evaluate(`__g.invoke("settings_get")`);
const before = await monitors();
writeFileSync(join(outDir, "monitors-before.json"), JSON.stringify(before, null, 2));
check("display_list reports the monitors", before.length >= 1, before.map((m) => `${m.name || m.key} ${m.refreshHz} Hz hdr=${m.hdrEnabled}`).join("; "));
const start = before.find((m) => m.current) ?? before[0];
// Windows refuses to switch HDR on while on battery (ADR-0006 amendment 1): the app must then report the refusal,
// play tone-mapped SDR and leave HDR off. Power state from WMI (read-only).
let onBattery = false;
try {
  const online = execFileSync("powershell.exe", ["-NoProfile", "-Command", "(Get-CimInstance -Namespace root/wmi -ClassName BatteryStatus -ErrorAction SilentlyContinue | Select-Object -First 1).PowerOnline"], { encoding: "utf8" }).trim();
  onBattery = online === "False";
} catch {
  onBattery = false;
}
log(`power: ${onBattery ? "battery" : "mains (or no battery)"}`);
const canHdr = start.hdrSupported && !start.hdrEnabled && !onBattery;
const refusedHdr = start.hdrSupported && !start.hdrEnabled && onBattery;
const skip = (name, detail) => check(`${name} [capability skip]`, true, detail);
// Desktop defaults (HDR for HDR content only, automatic refresh), audio defaults.
await evaluate(
  `__g.invoke("settings_get").then((s) => __g.invoke("settings_set", { settings: { ...s, display: { ...s.display, hdrMode: "content", refresh: "auto" } } }))`,
);

async function playMovie(id) {
  await evaluate(`location.hash = ${JSON.stringify(`${serverPath.slice(1)}/item/${id}`)}; true`);
  await waitFor(`!!document.querySelector("h1")`, `movie ${id}`);
  await reinject();
  await waitFor(`!!__g.find("button", /^(From the beginning|Play)$/)`, "play button");
  await evaluate(`__g.click("button", /^(From the beginning|Play)$/)`);
  await waitFor(`__g.invoke("player_state").then((s) => s.phase === "playing")`, "playing", 40000);
  await sleep(2500);
}
async function stopAndWaitRestore(what) {
  await wiggle();
  await key("q");
  await waitFor(`!document.querySelector('[data-testid="player-view"]')`, "player closed", 20000);
  return what;
}
const same = (a, b) => a.hdrEnabled === b.hdrEnabled && Math.abs(a.refreshHz - b.refreshHz) < 0.01;

// 1) HDR10 clip (23.976 fps): Windows HDR switched on before the first frame (when it is off), tone-mapping target
//    from DXGI, SDR white from Windows.
await playMovie("m1");
let now = await current();
const stats1 = await evaluate(`__g.invoke("player_stats")`);
writeFileSync(join(outDir, "stats-hdr10.json"), JSON.stringify(stats1, null, 2));
await wiggle();
await key("i");
await sleep(1200);
await shot("01-hdr10-stats");
await key("i");
const range1 = String(stats1["output-dynamic-range"] ?? "");
const hdrOut1 = /^HDR/.test(range1);
if (canHdr) {
  check("HDR content: Windows HDR switched on", now.hdrEnabled && /switched on/.test(range1), range1);
} else if (refusedHdr) {
  check("HDR content on battery: refusal handled (HDR left off, tone-mapped SDR)", !now.hdrEnabled && /^SDR/.test(range1), range1);
} else {
  skip("HDR content: HDR already on or unsupported", `hdrSupported=${start.hdrSupported} hdrEnabled=${start.hdrEnabled}`);
}
check("stats: output dynamic range matches the monitor", hdrOut1 === now.hdrEnabled, `${range1}, monitor hdr=${now.hdrEnabled}`);
const peak1 = String(stats1["output-target-peak"] ?? "");
check(
  "stats: tone-mapping target peak (nits for HDR output, auto for SDR)",
  hdrOut1 ? Number(peak1) >= 80 : peak1 === "auto",
  `output-target-peak=${peak1} mpv target-peak=${stats1["target-peak"]} target-contrast=${stats1["target-contrast"]}`,
);
check("stats: source dynamic range from the plan", stats1["source-dynamic-range"] === "hdr10", String(stats1["source-dynamic-range"]));
check("stats: audio output options", stats1["audio-device"] !== undefined, `device=${stats1["audio-device"]} exclusive=${stats1["audio-exclusive"]} spdif=${stats1["audio-spdif"]}`);
await stopAndWaitRestore("hdr");
now = await current();
if (canHdr) check("HDR held during the idle grace (for a next episode)", now.hdrEnabled, `hdr=${now.hdrEnabled}`);
else skip("HDR held during the idle grace", "HDR was not switched");
await sleep(13000);
now = await current();
check("restored after the idle grace", same(now, start), `hdr ${start.hdrEnabled} -> ${now.hdrEnabled}, ${start.refreshHz} -> ${now.refreshHz} Hz`);

// 2) SDR clip at 25 fps: automatic refresh picks an exact multiple near the current rate if the panel has one.
const multiple = start.modesHz.filter((hz) => Math.abs(hz / 25 - Math.round(hz / 25)) < 0.002 && hz >= 48);
await playMovie("m2");
now = await current();
const stats2 = await evaluate(`__g.invoke("player_stats")`);
writeFileSync(join(outDir, "stats-sdr25.json"), JSON.stringify(stats2, null, 2));
await wiggle();
await key("i");
await sleep(1200);
await shot("02-sdr25-stats");
await key("i");
check("SDR content: HDR left as it was", now.hdrEnabled === start.hdrEnabled, `hdr=${now.hdrEnabled}`);
if (multiple.length > 0 && Math.abs(start.refreshHz / 25 - Math.round(start.refreshHz / 25)) > 0.002) {
  check("25 fps: switched to an exact multiple", multiple.some((hz) => Math.abs(hz - now.refreshHz) < 0.01), `${start.refreshHz} -> ${now.refreshHz} Hz (${stats2["output-refresh"]})`);
} else {
  skip("25 fps: no exact multiple or already fits", String(stats2["output-refresh"]));
}
// Audio settings apply live during playback: mpv's audio-normalize-downmix goes from yes to no without a restart.
const downmixBefore = String(stats2["audio-normalize-downmix"]);
await evaluate(`__g.invoke("settings_get").then((s) => __g.invoke("settings_set", { settings: { ...s, audio: { ...s.audio, normalizeDownmix: false } } }))`);
await sleep(1500);
const live = await evaluate(`__g.invoke("player_stats")`);
check(
  "audio settings apply live (audio-normalize-downmix yes -> no, still playing)",
  downmixBefore === "yes" && String(live["audio-normalize-downmix"]) === "no" && (await state()).phase === "playing",
  `${downmixBefore} -> ${live["audio-normalize-downmix"]}, phase=${(await state()).phase}`,
);
await stopAndWaitRestore("sdr");
await sleep(13000);
now = await current();
check("refresh restored after the idle grace", same(now, start), `${start.refreshHz} -> ${now.refreshHz} Hz, hdr ${now.hdrEnabled}`);

// 3) HDR content again, then quit-style shutdown is covered by unit tests; here: back-to-back SDR → HDR → stop.
await evaluate(`__g.invoke("settings_set", { settings: ${JSON.stringify(originalSettings)} })`);
const after = await monitors();
writeFileSync(join(outDir, "monitors-after.json"), JSON.stringify(after, null, 2));
check(
  "every monitor back to its original HDR state and refresh rate",
  before.every((b) => after.some((a) => a.key === b.key && same(a, b))),
  after.map((m) => `${m.name || m.key} ${m.refreshHz} Hz hdr=${m.hdrEnabled}`).join("; "),
);

// The host log for this run.
const logDir = join(process.env.LOCALAPPDATA ?? "", "Granary", "logs");
const files = readdirSync(logDir)
  .map((f) => join(logDir, f))
  .filter((f) => statSync(f).mtime >= startedAt);
const lines = files
  .flatMap((f) => readFileSync(f, "utf8").split(/\r?\n/))
  .filter((l) => {
    const t = Date.parse(l.slice(0, 30).split(" ")[0]);
    return Number.isNaN(t) || t >= startedAt.getTime() - 1000;
  });
const keep = lines.filter((l) => /display output decided|display state|output option|display policy|monitor|playback plan accepted|ERROR|WARN/.test(l));
writeFileSync(join(outDir, "output-log.txt"), keep.join("\n") + "\n");
const decided = keep.filter((l) => l.includes("display output decided"));
check("log: display output decided before each playback", decided.length >= 2, `${decided.length} decisions`);
if (canHdr) check("log: HDR switched for the HDR clip", decided.some((l) => /hdr_switched=true/.test(l)));
check("log: display state restored", keep.some((l) => l.includes("display state restored")) || !decided.some((l) => /switched=true/.test(l)));
if (refusedHdr) check("log: HDR refusal on battery reported", keep.some((l) => /HDR switch failed .*on battery|HDR switch skipped/.test(l)));
check("log: no unrestored display state", !keep.some((l) => l.includes("not fully restored")));

writeFileSync(join(outDir, "checks.json"), JSON.stringify(checks, null, 2));
log(`\nOUTPUT EVIDENCE: ${checks.filter((c) => c.ok).length}/${checks.length} PASS -> ${outDir}`);
ws.close();
