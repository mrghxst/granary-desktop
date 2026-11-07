import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

/**
 * IPC contract (phase-5 review L8): a command missing from the capability, a handler that is not registered or an
 * argument renamed on one side passes every other test and fails only in the app. This checks the four places a
 * command lives against each other: build.rs `COMMANDS` (generated `allow-*` permissions), `generate_handler!` in
 * lib.rs, capabilities/main.json, and every `invoke`/`call` in the UI with the Rust parameter names (Tauri maps
 * snake_case parameters to camelCase keys).
 */
const root = process.cwd();
const host = resolve(root, "src-tauri");
const read = (p: string) => readFileSync(p, "utf8");

function files(dir: string, ext: RegExp): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? files(p, ext) : ext.test(name) ? [p] : [];
  });
}

/** Split `s` at top-level commas (ignoring commas inside (), <>, {}, [] and strings). */
function splitTop(s: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let quote: string | null = null;
  let cur = "";
  for (let i = 0; i < s.length; i++) {
    const c = s.charAt(i);
    if (quote) {
      if (c === quote && s.charAt(i - 1) !== "\\") quote = null;
    } else if (c === '"' || c === "`") {
      quote = c;
    } else if ("(<{[".includes(c)) {
      depth++;
    } else if (")}]".includes(c) || (c === ">" && s.charAt(i - 1) !== "=" && s.charAt(i - 1) !== "-")) {
      depth--;
    } else if (c === "," && depth === 0) {
      out.push(cur.trim());
      cur = "";
      continue;
    }
    cur += c;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}

/** Text between the bracket at `open` and its partner. */
function enclosed(s: string, open: number): string {
  const pairs: Record<string, string> = { "(": ")", "{": "}", "[": "]" };
  const o = s.charAt(open);
  const close = pairs[o];
  let depth = 0;
  for (let i = open; i < s.length; i++) {
    if (s.charAt(i) === o) depth++;
    else if (s.charAt(i) === close && --depth === 0) return s.slice(open + 1, i);
  }
  throw new Error(`unbalanced ${o} at ${String(open)}`);
}

const camel = (s: string) => s.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());

interface Param {
  key: string;
  optional: boolean;
}

/** `#[tauri::command]` functions in the host and their UI-visible parameters. */
function rustCommands(): Map<string, Param[]> {
  const out = new Map<string, Param[]>();
  for (const f of files(resolve(host, "src"), /\.rs$/)) {
    const src = read(f);
    for (const m of src.matchAll(/#\[tauri::command[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*pub\s+(?:async\s+)?fn\s+(\w+)/g)) {
      const name = m[1] ?? "";
      const open = src.indexOf("(", m.index + m[0].length);
      const params = splitTop(enclosed(src, open))
        .map((p) => {
          const at = p.indexOf(":");
          return {
            name: p
              .slice(0, at)
              .replace(/^mut\s+/, "")
              .trim(),
            ty: p.slice(at + 1).trim(),
          };
        })
        // Injected by Tauri, not sent by the UI.
        .filter(({ ty }) => !/^(tauri::)?(State|AppHandle|Window|WebviewWindow|Webview)\b/.test(ty))
        .map(({ name: n, ty }) => ({ key: camel(n), optional: ty.startsWith("Option<") }));
      out.set(name, params);
    }
  }
  return out;
}

interface Call {
  file: string;
  command: string;
  keys: string[];
}

/** `invoke("x", {...})` / `call("x", {...})` in the UI (tests excluded). */
function uiCalls(): Call[] {
  const out: Call[] = [];
  for (const f of files(resolve(root, "src"), /\.tsx?$/).filter((p) => !/\.test\.tsx?$/.test(p))) {
    const src = read(f);
    for (const m of src.matchAll(/\b(?:invoke|call)(?:<[^>(]*(?:<[^>]*>)?[^>(]*>)?\(\s*"(\w+)"\s*(,\s*\{)?/g)) {
      const keys = m[2]
        ? splitTop(enclosed(src, m.index + m[0].length - 1)).map((e) => /^(\w+)/.exec(e)?.[1] ?? e)
        : [];
      out.push({ file: f.slice(root.length + 1), command: m[1] ?? "", keys });
    }
  }
  return out;
}

const buildList = () => {
  const src = read(resolve(host, "build.rs"));
  const body = /const COMMANDS: &\[&str\] = &\[([^\]]*)\]/.exec(src)?.[1] ?? "";
  return [...body.matchAll(/"(\w+)"/g)].map((m) => m[1] ?? "").sort();
};
const handlerList = () => {
  const src = read(resolve(host, "src/lib.rs"));
  const body = /generate_handler!\[([^\]]*)\]/.exec(src)?.[1] ?? "";
  return splitTop(body)
    .map((p) => p.split("::").pop() ?? "")
    .filter(Boolean)
    .sort();
};
const capabilityList = () => {
  const cap = JSON.parse(read(resolve(host, "capabilities/main.json"))) as { permissions: string[] };
  return cap.permissions
    .filter((p) => p.startsWith("allow-"))
    .map((p) => p.slice("allow-".length).replace(/-/g, "_"))
    .sort();
};

describe("IPC contract", () => {
  const commands = rustCommands();

  it("registers, permits and generates permissions for exactly the same commands", () => {
    const defined = [...commands.keys()].sort();
    expect(defined.length).toBeGreaterThan(30);
    expect(handlerList()).toEqual(defined);
    expect(buildList()).toEqual(defined);
    expect(capabilityList()).toEqual(defined);
  });

  it("grants no core or plugin permission beyond listening for host events", () => {
    const cap = JSON.parse(read(resolve(host, "capabilities/main.json"))) as {
      permissions: string[];
      windows: string[];
    };
    expect(cap.windows).toEqual(["main"]);
    expect(cap.permissions.filter((p) => !p.startsWith("allow-"))).toEqual([
      "core:event:allow-listen",
      "core:event:allow-unlisten",
    ]);
  });

  it("calls only registered commands, with the Rust parameter names", () => {
    const calls = uiCalls();
    expect(calls.length).toBeGreaterThan(30);
    const problems: string[] = [];
    for (const c of calls) {
      const params = commands.get(c.command);
      if (!params) {
        problems.push(`${c.file}: unknown command ${c.command}`);
        continue;
      }
      const known = new Set(params.map((p) => p.key));
      for (const k of c.keys) if (!known.has(k)) problems.push(`${c.file}: ${c.command} has no parameter ${k}`);
      for (const p of params)
        if (!p.optional && !c.keys.includes(p.key)) problems.push(`${c.file}: ${c.command} misses ${p.key}`);
    }
    expect(problems).toEqual([]);
    // Every command the host offers is used by the UI (an unused command is attack surface without a purpose).
    const used = new Set(calls.map((c) => c.command));
    expect([...commands.keys()].filter((c) => !used.has(c))).toEqual([]);
  });

  it("parses the shapes it relies on", () => {
    expect(splitTop("a: State<'_, X>, b: Option<Vec<u8>>, c: String")).toEqual([
      "a: State<'_, X>",
      "b: Option<Vec<u8>>",
      "c: String",
    ]);
    expect(splitTop('serverId, pin: pin ?? null, value: f(a, "x,y") ? 1 : 2')).toEqual([
      "serverId",
      "pin: pin ?? null",
      'value: f(a, "x,y") ? 1 : 2',
    ]);
    expect(camel("server_id")).toBe("serverId");
  });
});
