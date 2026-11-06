#!/usr/bin/env node
// Drive a running debug Granary (mock server) over the WebView2 DevTools protocol and capture UI evidence.
//
// Start the app first (PowerShell):
//   $env:GRANARY_MOCK_SERVER = "1"; $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
//   pnpm --filter desktop tauri dev
// then: node scripts/ui-evidence.mjs [outDir] [port]
//
// Walks connect → sign in (mock user from crates/silo-api/src/mock.rs) → profile → home → library → item → series →
// settings, saves screenshots at several widths/scales, and asserts what can be asserted without human eyes:
// artwork decodes, reduced motion removes transitions, true-black is #000, the navigation guard holds, no <video>.
// Exits non-zero on the first failed check. Local only: talks to 127.0.0.1.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const outDir = process.argv[2] ?? "docs/evidence/phase-5";
const port = Number(process.argv[3] ?? 9333);
mkdirSync(outDir, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(...a);

async function target() {
  for (let i = 0; i < 60; i++) {
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

// Page-side helpers: React-compatible input filling and lookups by accessible text.
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
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
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
};`;

async function waitFor(expression, what, timeout = 15000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    try {
      if (await evaluate(expression)) return;
    } catch {
      /* page navigating */
    }
    await sleep(200);
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

async function viewport(width, height, scale = 1) {
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: scale, mobile: false });
  await sleep(400);
}

async function shot(name) {
  await sleep(500);
  const { data } = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(outDir, `${name}.png`), Buffer.from(data, "base64"));
  log(`  saved ${name}.png`);
}

const go = (hash) => evaluate(`location.hash = ${JSON.stringify(hash)}; true`);
const reinject = () => evaluate(HELPERS + "; true");

await send("Page.enable");
await send("Runtime.enable");
await viewport(1600, 1000);
await go("#/");
await sleep(500);
await reinject();

// Connect and sign in (test values of the in-process mock server).
const signedIn = await evaluate(`!!document.querySelector('a[href*="/home"]')`);
if (!signedIn) {
  await evaluate(`location.hash = "#/servers/add"; true`);
  await waitFor(`!!window.__g.control("Server address")`, "add-server form");
  await evaluate(`__g.fill("Server address", "media.example.test")`);
  await evaluate(`__g.click("button", /^Check$/)`);
  await waitFor(`!!__g.find("button", /^Add server$/)`, "probe result");
  await shot("01-add-server");
  await evaluate(`__g.click("button", /^Add server$/)`);
  await waitFor(`!!__g.control("Username")`, "sign-in form");
  await evaluate(`__g.fill("Username", "alice") && __g.fill("Password", "pw")`);
  await shot("02-sign-in");
  await evaluate(`__g.click("button", /^Sign in$/)`);
  await waitFor(`!!__g.find("button", /Alice/)`, "profile picker");
  await shot("03-profiles");
  await evaluate(`__g.click("button", /Alice/)`);
}
await waitFor(`location.hash.includes("/home") && document.querySelectorAll("img").length > 0`, "home");
const serverPath = await evaluate(`location.hash.replace(/\\/home.*$/, "")`);
await waitFor(
  `[...document.querySelectorAll("img")].filter((i) => i.complete && i.naturalWidth > 0).length > 0`,
  "artwork decoded",
);
const art = await evaluate(`(() => {
  const imgs = [...document.querySelectorAll("img")].filter((i) => i.src.includes("granary-img"));
  return { total: imgs.length, decoded: imgs.filter((i) => i.complete && i.naturalWidth > 0).length };
})()`);
check("artwork loads through granary-img as PNG", art.total > 0 && art.decoded > 0, `${art.decoded}/${art.total} decoded`);

for (const [w, h, s, name] of [
  [960, 720, 1, "04-home-960"],
  [1920, 1080, 1, "05-home-1920"],
  [1280, 800, 1.5, "06-home-1280-150pct"],
  [1920, 1080, 2, "07-home-1920-200pct"],
  [3840, 2160, 1, "08-home-3840"],
]) {
  await viewport(w, h, s);
  const overflow = await evaluate(`document.documentElement.scrollWidth - document.documentElement.clientWidth`);
  check(`no horizontal overflow at ${w}px x${s}`, overflow <= 0, `overflow ${overflow}px`);
  await shot(name);
}
const rail = await (async () => {
  await viewport(960, 720);
  return evaluate(`(() => { const n = document.querySelector("nav"); return n ? n.getBoundingClientRect().width : -1; })()`);
})();
check("sidebar collapses to the icon rail below lg", rail > 0 && rail < 120, `nav width ${rail}px`);
await viewport(3840, 2160);
const main = await evaluate(`(() => { const m = document.querySelector("main [class*='max-w']") ?? document.querySelector("main"); return m.getBoundingClientRect().width; })()`);
check("content is capped at 2400 css px on 4K", main <= 2400, `width ${main}px`);

// Library → item → series.
await viewport(1600, 1000);
await reinject();
await evaluate(`__g.click("a", /^Movies$/)`);
await waitFor(`!!document.querySelector('a[href*="/item/"]')`, "library grid");
await shot("09-library");
await evaluate(`document.querySelector('a[href*="/item/"]').click(); true`);
await waitFor(`!!document.querySelector("h1")`, "item page");
await sleep(800);
await shot("10-item-movie");
await reinject();
await evaluate(`__g.click("a", /^Shows$/)`);
await waitFor(`!!document.querySelector('a[href*="/item/"]')`, "shows grid");
await evaluate(`document.querySelector('a[href*="/item/"]').click(); true`);
await waitFor(`!!document.querySelector('[role="tab"]')`, "season tabs");
await sleep(800);
await shot("11-item-series");
check("no <video> element anywhere", await evaluate(`document.querySelectorAll("video").length === 0`));

// Keyboard focus ring.
await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
const ring = await evaluate(`(() => { const a = document.activeElement; const s = getComputedStyle(a); return a !== document.body && (s.outlineStyle !== "none" || s.boxShadow !== "none"); })()`);
check("Tab moves focus to a control with a visible focus style", ring);
await shot("12-focus-ring");

// Reduced motion: transitions are gone.
await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
const reduced = await evaluate(`(() => {
  const els = [...document.querySelectorAll("*")].filter((e) => getComputedStyle(e).transitionProperty !== "all" || true);
  return els.every((e) => getComputedStyle(e).transitionDuration.split(",").every((d) => parseFloat(d) === 0));
})()`);
check("prefers-reduced-motion removes every transition", reduced);
await send("Emulation.setEmulatedMedia", { features: [] });

// True-black theme through the settings screen.
await go(`${serverPath}/settings`);
await waitFor(`!!document.querySelector('[aria-label="Theme"]') || !!(window.__g && __g.control("Theme"))`, "settings");
await reinject();
await evaluate(`__g.fill("Theme", "true-black")`);
await sleep(600);
const bg = await evaluate(`getComputedStyle(document.body).backgroundColor`);
check("true-black theme paints the page #000", bg === "rgb(0, 0, 0)", bg);
await shot("13-settings-true-black");
await go(`${serverPath}/home`);
await sleep(1200);
await shot("14-home-true-black");
await go(`${serverPath}/settings`);
await sleep(600);
await reinject();
await evaluate(`__g.fill("Theme", "midnight-cinema")`);
await sleep(300);

// Navigation guard: the artwork scheme can never become the top-level document.
const before = await evaluate(`location.origin`);
await evaluate(`location.href = "http://granary-img.localhost/x?u=%2Fapi%2Fv2%2Fartwork%2Fk"; true`);
await sleep(1500);
const after = await evaluate(`location.origin`);
check("navigation to the artwork scheme is blocked", after === before, `${before} -> ${after}`);

writeFileSync(join(outDir, "checks.json"), JSON.stringify(checks, null, 2));
await send("Emulation.clearDeviceMetricsOverride");
ws.close();
log(`UI EVIDENCE OK (${checks.length} checks) -> ${outDir}`);
