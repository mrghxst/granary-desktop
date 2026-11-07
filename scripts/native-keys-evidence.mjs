// Native key check (Gate 8 M2 / phase-6 L6): with the overlay hidden the video window has focus; keys pressed there
// must follow the active keyboard layout. Run after measure-ttff.mjs has signed in to the mock (dev app on 9333).
// The key codes below are for a German (QWERTZ) layout; scripts/send-key.ps1 presses them through keybd_event.
// Needs an unlocked desktop session. Usage: node scripts/native-keys-evidence.mjs
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const port = 9333;
const S = dirname(fileURLToPath(import.meta.url));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const page = list.find(
  (t) => t.type === "page" && /127\.0\.0\.1:1420|tauri\.localhost/.test(t.url),
);
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r, j) => {
  ws.onopen = r;
  ws.onerror = j;
});
let id = 1;
const pending = new Map();
ws.onmessage = (m) => {
  const msg = JSON.parse(String(m.data));
  const p = pending.get(msg.id);
  if (p) {
    pending.delete(msg.id);
    msg.error ? p.j(new Error(JSON.stringify(msg.error))) : p.r(msg.result);
  }
};
const send = (method, params = {}) =>
  new Promise((r, j) => {
    const i = id++;
    pending.set(i, { r, j });
    ws.send(JSON.stringify({ id: i, method, params }));
  });
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
};
const state = () => ev(`window.__TAURI_INTERNALS__.invoke("player_state")`);
const until = async (expr, ms = 30000) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      if (await ev(expr)) return true;
    } catch {}
    await sleep(250);
  }
  return false;
};
const key = (vk, shift) => {
  keyLog.push(vk);
  const r = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      join(S, "send-key.ps1"),
      "-Vk",
      String(vk),
      ...(shift ? ["-Shift"] : []),
    ],
    { encoding: "utf8" },
  );
  return r.stdout.trim();
};
const results = [];
const keyLog = [];
const vis = () =>
  ev('document.visibilityState + "/focus=" + document.hasFocus()');
const check = (name, ok, detail) => {
  results.push(`${ok ? "PASS" : "FAIL"} ${name} (${detail})`);
  console.log(results.at(-1));
};

const hash = await ev("location.hash");
const server = hash.match(/^#\/servers\/[^/]+/)?.[0];
if (!server) throw new Error("not signed in: " + hash);
await ev(`location.hash = ${JSON.stringify(server + "/item/m2")}; true`);
await until(
  `[...document.querySelectorAll("button")].some((b) => /^(From the beginning|Play)$/.test(b.textContent.trim()))`,
);
await ev(
  `[...document.querySelectorAll("button")].find((b) => /^(From the beginning|Play)$/.test(b.textContent.trim())).click(); true`,
);
await until(
  `window.__TAURI_INTERNALS__.invoke("player_state").then((s) => s.phase === "playing")`,
  40000,
);
// Let the overlay auto-hide (2.5 s while playing): the host then focuses the native video window.
await sleep(4500);
// Control: "i" (same on every layout) toggles the stats panel through the same native path.
console.log("page before key:", await vis());
console.log(key(0x49, false));
const statsShown = await until(
  `!!document.querySelector('div.font-mono[aria-label]')`,
  3000,
);
check(
  "i from the video window reaches the UI (control)",
  statsShown,
  statsShown ? "stats shown" : "no stats",
);
console.log(key(0x49, false));
await sleep(4500);
const before = (await state()).volume;
console.log("page before key:", await vis());
console.log(key(0xbf, false)); // "#" on German QWERTZ: audio cycle; the old US map read it as "/" (volume down)
await sleep(800);
const after = (await state()).volume;
check(
  "# from the video window does not change the volume",
  after === before,
  `volume ${before} -> ${after}`,
);
await sleep(4500);
console.log("page before key:", await vis());
console.log(key(0xdb, true)); // Shift+VK_OEM_4 = "?" on German QWERTZ: shortcuts help; the old map read "{"
const helpOpen = await until(
  `!!document.querySelector('[role="dialog"]')`,
  3000,
);
check(
  "? from the video window opens the shortcuts help",
  helpOpen,
  helpOpen ? "dialog shown" : "no dialog",
);
await ev(`window.__TAURI_INTERNALS__.invoke("player_stop")`).catch(
  () => undefined,
);
await sleep(1500);
ws.close();
console.log(
  results.every((r) => r.startsWith("PASS"))
    ? "KEYTEST PASSED"
    : "KEYTEST FAILED",
);
