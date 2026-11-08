#!/usr/bin/env node
// Gate 6 evidence: drive a running debug Granary (mock server, real libmpv, real test clips) over the WebView2
// DevTools protocol, play clips end to end and check the host log for the required facts.
//
// Start the app first (PowerShell):
//   $env:GRANARY_MOCK_SERVER = "1"; $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
//   pnpm --filter desktop tauri dev
// then: node scripts/playback-evidence.mjs [outDir] [port]
//
// Plays the HDR10 movie clip (resume → pause → seek → stats → keyboard stop), then an episode (next episode via
// `N`), and asserts from the UI, the host and %LOCALAPPDATA%\Granary\logs:
//   - a direct-play plan (original file, no transcode parameters in the stream URL),
//   - `hwdec-current` is a hardware decoder,
//   - progress samples acknowledged by the server, and the session stopped once.
// Local only: talks to 127.0.0.1. Test values of the in-process mock (alice / pw) are used for sign-in.
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const outDir = process.argv[2] ?? "docs/evidence/phase-6";
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

// 1) The HDR10 movie (m1-f1 → hdr10_hevc_main10_2160p.mkv), from the beginning (the demo resume point lies past
//    the end of the 60 s clip).
await evaluate(`location.hash = ${JSON.stringify(`${serverPath.slice(1)}/item/m1`)}; true`);
await waitFor(`!!document.querySelector("h1")`, "movie page");
await reinject();
await waitFor(`!!__g.find("button", /^(From the beginning|Play)$/)`, "play button");
await evaluate(`__g.click("button", /^(From the beginning|Play)$/)`);
await waitFor(`!!document.querySelector('[data-testid="player-view"]')`, "player view", 30000);
check("player overlay replaces the library", true);
check(
  "page is transparent over the native video window",
  await evaluate(`document.documentElement.dataset.videoActive === "true" && getComputedStyle(document.body).backgroundColor === "rgba(0, 0, 0, 0)"`),
);
await waitFor(`__g.invoke("player_state").then((s) => s.phase === "playing")`, "playing", 30000);
let s = await state();
check("mpv plays the clip", s.active && s.phase === "playing", `position ${s.position.toFixed(1)} s of ${s.duration}`);
// The picture must be visible under the shown overlay on screen, not only in mpv (0.1.0 user report: the overlay
// made the video disappear). Page screenshots cannot see the native video window, so sample the composed screen.
await sleep(1500);
const pixels = (visible) => {
  return evaluate(`__g.invoke("player_overlay", { visible: ${visible} }).then(() => true)`).then(() =>
    JSON.parse(
      execFileSync(
        "powershell.exe",
        ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/overlay-pixels.ps1"],
        { encoding: "utf8" },
      ).trim(),
    ),
  );
};
await wiggle(); // controls visible, not only the WebView
const onScreenShown = await pixels(true);
const onScreenHidden = await pixels(false);
await evaluate(`__g.invoke("player_overlay", { visible: true }).then(() => true)`);
check("video visible on screen under the shown overlay", onScreenShown.buckets >= 4, JSON.stringify(onScreenShown));
check("video visible on screen with the overlay hidden", onScreenHidden.buckets >= 4, JSON.stringify(onScreenHidden));
await sleep(11500); // one periodic progress sample (10 s)
await wiggle();
await reinject();
await shot("01-player-overlay");

// Pause and resume (each a progress sample), then seek by the seek step.
await key(" ");
await waitFor(`__g.invoke("player_state").then((s) => s.paused)`, "paused");
check("space pauses", true);
await sleep(1500);
await shot("02-paused");
await key(" ");
await waitFor(`__g.invoke("player_state").then((s) => !s.paused)`, "resumed");
const before = (await state()).position;
await key("ArrowRight");
await sleep(1500);
const after = (await state()).position;
check("right arrow seeks forward", after - before >= 5, `${before.toFixed(1)} → ${after.toFixed(1)} s`);

// Stats overlay (`i`) and the host stats behind it.
await wiggle();
await key("i");
await sleep(1500);
const stats = await evaluate(`__g.invoke("player_stats")`);
writeFileSync(join(outDir, "stats-hdr10.json"), JSON.stringify(stats, null, 2));
await shot("03-stats");
const hw = String(stats["hwdec-current"] ?? "");
check("hwdec-current is a hardware decoder", hw !== "" && hw !== "no", `hwdec-current=${hw}`);
check("gpu-next output", String(stats["current-vo"] ?? "") === "gpu-next", `current-vo=${stats["current-vo"]}`);
await key("i");

// Idle: the overlay hides itself (the host hides the WebView); input brings it back.
await sleep(3500);
const hidden = await evaluate(`getComputedStyle(document.querySelector('[data-testid="player-view"] > div:nth-child(2)')).opacity`);
check("controls auto-hide after ~2.5 s", Number(hidden) === 0, `opacity ${hidden}`);
await wiggle();

// Help overlay, then stop with `q`.
await key("?", true);
await sleep(500);
check("? shows the keyboard shortcuts", await evaluate(`!!document.querySelector('[role="dialog"][aria-label="Keyboard shortcuts"]')`));
await shot("04-shortcuts");
await key("Escape");
await key("q");
await waitFor(`!document.querySelector('[data-testid="player-view"]')`, "player closed", 20000);
check("q stops; the library is back", await evaluate(`document.documentElement.dataset.videoActive === undefined`));

// 2) An episode, then the next one through `N` (series_id + season episodes).
await evaluate(`location.hash = ${JSON.stringify(`${serverPath.slice(1)}/library/l2`)}; true`);
await sleep(300);
await evaluate(`location.hash = ${JSON.stringify(`${serverPath.slice(1)}/home`)}; true`);
await reinject();
await waitFor(`!!__g.find("a", /^Shows$/)`, "shows link");
await evaluate(`__g.click("a", /^Shows$/)`);
await waitFor(`!!document.querySelector('a[href*="/item/"]')`, "shows grid");
await evaluate(`document.querySelector('a[href*="/item/"]').click(); true`);
await waitFor(`!!document.querySelector('[role="tab"]')`, "season tabs");
await reinject();
await waitFor(`!!__g.find("button", /^Play /)`, "episode play button");
await evaluate(`__g.find("button", /^Play /).click(); true`);
await waitFor(`__g.invoke("player_state").then((s) => s.active && s.phase === "playing")`, "episode playing", 30000);
const first = (await state()).itemId;
await sleep(4000);
await wiggle();
await reinject();
await shot("05-episode");
const hasNext = await evaluate(`!!document.querySelector('button[aria-label="Next episode"]')`);
check("next-episode control for a series episode", hasNext);
await key("N", true);
await waitFor(
  `__g.invoke("player_state").then((s) => s.active && s.phase === "playing" && s.itemId !== ${JSON.stringify(first)})`,
  "next episode playing",
  30000,
);
const second = (await state()).itemId;
check("N starts the next episode", second !== first, `${first} → ${second}`);
await sleep(3000);
const stats2 = await evaluate(`__g.invoke("player_stats")`);
writeFileSync(join(outDir, "stats-sdr.json"), JSON.stringify(stats2, null, 2));
await key("q");
await waitFor(`!document.querySelector('[data-testid="player-view"]')`, "player closed", 20000);
await sleep(1500);

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
const keep = lines.filter((l) =>
  /play requested|playback plan accepted|mpv hardware decoding|progress acknowledged|playback stopped|stopped|media relay|hardware decoders|mpv created|video window attached|ERROR|WARN/.test(
    l,
  ),
);
writeFileSync(join(outDir, "playback-log.txt"), keep.join("\n") + "\n");
const plans = keep.filter((l) => l.includes("playback plan accepted"));
check("direct-play plans", plans.length >= 3 && plans.every((l) => /direct_play=true/.test(l)), `${plans.length} plans`);
check(
  "stream URLs carry no transcode parameters",
  plans.every((l) => /stream=\S+/.test(l) && !/stream=\S*(transcode|video_codec|audio_codec|bitrate|max_|hls)/i.test(l)),
  plans.map((l) => l.match(/stream=(\S+)/)?.[1]).join(" "),
);
const hwLines = keep.filter((l) => l.includes("mpv hardware decoding"));
check(
  "log: hwdec-current active",
  hwLines.some((l) => /hwdec_current=(?!no\b)\w+/.test(l)),
  hwLines.map((l) => l.match(/hwdec_current=(\S+)/)?.[1]).join(", "),
);
const acks = keep.filter((l) => l.includes("progress acknowledged"));
check("log: progress acknowledged by the server", acks.length >= 4, `${acks.length} samples`);
const stops = keep.filter((l) => l.includes("playback stopped on the server"));
// Every negotiated session is stopped exactly once (movie by q, an episode replaced by N, one that ended at once
// because the mock resume point lies past the clip end and autoplay moved on, the last by q).
check("log: every session stopped once", stops.length === plans.length, `${stops.length} stops for ${plans.length} sessions`);

writeFileSync(join(outDir, "checks.json"), JSON.stringify(checks, null, 2));
ws.close();
log(`PLAYBACK EVIDENCE OK (${checks.length} checks) -> ${outDir}`);
