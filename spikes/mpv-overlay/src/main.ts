import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const $ = (id: string) => document.getElementById(id) as HTMLElement;
const file = $("file") as HTMLInputElement;
const status = $("status");
let hideTimer: number | undefined;

function log(msg: string) {
  void invoke("spike_log", { msg });
}

async function act(id: string) {
  try {
    switch (id) {
      case "play":
        await invoke("spike_play", { path: file.value });
        break;
      case "pause":
        await invoke("spike_cmd", { name: "pause" });
        break;
      case "fwd":
        await invoke("spike_cmd", { name: "seek", arg: 3 });
        break;
      case "fs":
        await invoke("spike_fullscreen");
        break;
      case "hide":
        await invoke("spike_overlay", { visible: false });
        break;
    }
  } catch (e) {
    status.textContent = String(e);
    log(`error in ${id}: ${String(e)}`);
  }
}

for (const id of ["play", "pause", "fwd", "fs", "hide"]) {
  $(id).addEventListener("click", () => void act(id));
}

// Auto-hide after 3 s of inactivity (overlay HWND hidden => mpv presents without composition overhead).
function poke() {
  if (hideTimer) clearTimeout(hideTimer);
  hideTimer = window.setTimeout(() => {
    if (document.activeElement !== file) void invoke("spike_overlay", { visible: false });
  }, 3000);
}
document.addEventListener("mousemove", poke);

document.addEventListener("keydown", (e) => {
  if (e.target === file) return;
  log(`keydown ${e.code}`);
  if (e.code === "Space" || e.key === " ") {
    e.preventDefault();
    void act("pause");
  } else if (e.code === "KeyF") {
    void act("fs");
  }
});

void listen<string>("selftest-click", (e) => {
  log(`selftest-click received: ${e.payload}`);
  ($(e.payload) as HTMLButtonElement).click();
});
void listen<string>("selftest-set-file", (e) => {
  file.value = e.payload;
});
void listen<string>("video-activity", (e) => {
  status.textContent = `activity: ${e.payload}`;
  poke();
});
void listen<{ name: string; value: string }>("mpv-prop", (e) => {
  if (e.payload.name === "pause") $("pause").textContent = e.payload.value === "yes" ? "Resume" : "Pause";
});

void invoke<boolean>("spike_selftest_enabled").then((on) => {
  log(`overlay loaded, selftest=${on}`);
  if (!on) poke();
});
