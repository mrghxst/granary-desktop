import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

/** Playback is native (libmpv); the WebView must never get a browser media element (prompt.md Phase 5). */
function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? files(p) : /\.(tsx?|html)$/.test(name) ? [p] : [];
  });
}

describe("no browser playback path", () => {
  it("has no <video> or <audio> element and no media element creation in the UI source", () => {
    const root = resolve(process.cwd(), "src");
    const offenders = files(root)
      .filter((f) => !f.endsWith(".test.ts") && !f.endsWith(".test.tsx"))
      .filter((f) => /<video|<audio|createElement\(\s*["'](video|audio)["']/.test(readFileSync(f, "utf8")));
    expect(offenders).toEqual([]);
  });
});
