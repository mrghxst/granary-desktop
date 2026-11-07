// Generate THIRD_PARTY_LICENSES.txt: the full licence texts of everything shipped in the installer (phase-9 review
// H1). Sources:
//   - libmpv / FFmpeg (GPL-3.0-or-later build): licenses/GPL-3.0.txt (verbatim FSF text);
//   - Rust crates linked into the Windows binary: `cargo metadata` for x86_64-pc-windows-msvc, normal dependencies
//     reachable from the `granary` package (build-only, dev-only and proc-macro crates are not shipped);
//   - JavaScript packages bundled into the UI: production dependencies of apps/desktop, plus Tailwind CSS (its
//     preflight CSS ends up in the stylesheet).
// Licence files are read from each package's published sources. Identical texts are printed once with every package
// that uses them. Usage: node scripts/gen-licenses.mjs [--check]   (--check exits 1 when the file is out of date).
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "THIRD_PARTY_LICENSES.txt");
const check = process.argv.includes("--check");
const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense)([-._].*)?$/i;
const norm = (t) => t.replace(/\r\n?/g, "\n").replace(/[ \t]+$/gm, "").trim();

function licenseFiles(dir, extra) {
  const names = existsSync(dir) ? readdirSync(dir).filter((n) => LICENSE_FILE.test(n)) : [];
  if (extra && !names.includes(extra) && existsSync(join(dir, extra))) names.push(extra);
  return names
    .filter((n) => statSync(join(dir, n)).isFile())
    .sort()
    .map((n) => ({ name: n, text: norm(readFileSync(join(dir, n), "utf8")) }))
    .filter((f) => f.text.length > 0);
}

// ---- Rust ----
function rustPackages() {
  const meta = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--format-version", "1", "--locked", "--filter-platform", "x86_64-pc-windows-msvc"],
      { cwd: root, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
    ),
  );
  const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const members = new Set(meta.workspace_members);
  const start = meta.packages.find((p) => p.name === "granary" && members.has(p.id));
  if (!start) throw new Error("package granary not found");
  const seen = new Set();
  const stack = [start.id];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id)?.deps ?? []) {
      if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
    }
  }
  return [...seen]
    .map((id) => pkgs.get(id))
    .filter((p) => !members.has(p.id))
    .filter((p) => !p.targets.every((t) => t.kind.includes("proc-macro")))
    .map((p) => ({
      label: `${p.name} ${p.version}`,
      license: p.license ?? (p.license_file ? `see ${p.license_file}` : "unspecified"),
      repo: p.repository ?? "",
      files: licenseFiles(dirname(p.manifest_path), p.license_file ?? undefined),
    }));
}

// ---- JavaScript ----
function jsPackages() {
  const app = join(root, "apps", "desktop");
  const manifest = JSON.parse(readFileSync(join(app, "package.json"), "utf8"));
  const seeds = [...Object.keys(manifest.dependencies ?? {}), "tailwindcss"];
  const found = new Map();
  // pnpm layout: a package's dependencies are its siblings in .pnpm/<id>/node_modules (or up the tree).
  const locate = (fromDir, name) => {
    let dir = fromDir;
    for (;;) {
      const candidate = join(dir, "node_modules", name);
      if (existsSync(join(candidate, "package.json"))) return realpathSync(candidate);
      const sibling = join(dir, name);
      if (dir.endsWith(`${sep}node_modules`) && existsSync(join(sibling, "package.json"))) return realpathSync(sibling);
      const up = dirname(dir);
      if (up === dir) return null;
      dir = up;
    }
  };
  const visit = (pkgDir) => {
    if (found.has(pkgDir)) return;
    const pj = JSON.parse(readFileSync(join(pkgDir, "package.json"), "utf8"));
    found.set(pkgDir, pj);
    const base = pj.name.startsWith("@") ? dirname(dirname(pkgDir)) : dirname(pkgDir);
    for (const dep of Object.keys({ ...pj.dependencies, ...pj.optionalDependencies })) {
      const dir = locate(base, dep) ?? locate(pkgDir, dep);
      if (dir) visit(dir);
      else if (!(dep in (pj.optionalDependencies ?? {}))) throw new Error(`${pj.name}: dependency ${dep} not installed`);
    }
  };
  for (const s of seeds) {
    const dir = locate(app, s);
    if (!dir) throw new Error(`${s} is not installed; run pnpm install`);
    visit(dir);
  }
  return [...found].map(([dir, pj]) => ({
    label: `${pj.name} ${pj.version}`,
    license: typeof pj.license === "string" ? pj.license : JSON.stringify(pj.license ?? "unspecified"),
    repo: typeof pj.repository === "string" ? pj.repository : (pj.repository?.url ?? ""),
    files: licenseFiles(dir),
  }));
}

function section(title, packages) {
  const byText = new Map();
  const missing = [];
  for (const p of packages) {
    if (p.files.length === 0) {
      missing.push(p);
      continue;
    }
    for (const f of p.files) {
      const key = createHash("sha256").update(f.text).digest("hex");
      const entry = byText.get(key) ?? { text: f.text, users: new Set() };
      entry.users.add(`${p.label} (${p.license}; ${f.name})`);
      byText.set(key, entry);
    }
  }
  const lines = [`${"=".repeat(100)}`, title, `${"=".repeat(100)}`, ""];
  const entries = [...byText.values()]
    .map((e) => ({ text: e.text, users: [...e.users].sort() }))
    .sort((a, b) => a.users[0].localeCompare(b.users[0]));
  for (const e of entries) {
    lines.push("Used by:", ...e.users.map((u) => `  - ${u}`), "", e.text, "", "-".repeat(100), "");
  }
  if (missing.length) {
    lines.push("Published without a licence file (licence by SPDX expression; text above or in the named source):");
    for (const p of missing.sort((a, b) => a.label.localeCompare(b.label))) {
      lines.push(`  - ${p.label}: ${p.license}${p.repo ? ` (${p.repo})` : ""}`);
    }
    lines.push("");
  }
  return lines;
}

const rust = rustPackages();
const js = jsPackages();
const gpl = norm(readFileSync(join(root, "licenses", "GPL-3.0.txt"), "utf8"));
const lgpl = norm(readFileSync(join(root, "licenses", "LGPL-2.1.txt"), "utf8"));
const text = [
  "Granary third-party licences",
  "",
  "Granary itself is licensed under the GNU Affero General Public License v3.0 or later (LICENSE.txt). This file",
  "contains the licence texts of the third-party components shipped with it; THIRD_PARTY_NOTICES.md lists versions",
  "and source links. Generated by scripts/gen-licenses.mjs; do not edit by hand.",
  "",
  `${"=".repeat(100)}`,
  "libmpv-2.dll: mpv and FFmpeg (and the libraries statically linked into this build), GPL-3.0-or-later",
  `${"=".repeat(100)}`,
  "",
  "This build of libmpv is configured with --enable-gpl --enable-version3 and is distributed under the GNU General",
  "Public License version 3 (or later). Corresponding source: see THIRD_PARTY_NOTICES.md.",
  "",
  gpl,
  "",
  `${"=".repeat(100)}`,
  "GNU Lesser General Public License version 2.1 (referenced by LGPL-2.1 components below, e.g. libmpv2-sys)",
  `${"=".repeat(100)}`,
  "",
  lgpl,
  "",
  ...section(`Rust crates linked into granary.exe (${rust.length})`, rust),
  ...section(`JavaScript packages bundled into the user interface (${js.length})`, js),
].join("\n") + "\n";

if (check) {
  const current = existsSync(out) ? readFileSync(out, "utf8").replace(/\r\n/g, "\n") : "";
  if (current !== text) {
    console.error("THIRD_PARTY_LICENSES.txt is out of date; run: node scripts/gen-licenses.mjs");
    process.exit(1);
  }
  console.log(`THIRD_PARTY_LICENSES.txt is current (${rust.length} crates, ${js.length} JS packages)`);
} else {
  writeFileSync(out, text);
  console.log(`wrote THIRD_PARTY_LICENSES.txt (${rust.length} crates, ${js.length} JS packages, ${text.length} bytes)`);
}
