// Packs the Corresponding Source of the bundled libmpv build (ADR-0005 amendment, 2026-11-18): one .tar.gz per
// repository at the exact commit in scripts/libmpv-sources.json (sub-paths and submodules as the build used them),
// the release tarballs it downloads (hash-checked), the build recipe with its patches, the manifest and SHA256SUMS.
// usage: node scripts/libmpv-source.mjs <out-dir> [--only name,name]
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
  copyFileSync,
  readdirSync,
} from "node:fs";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
const out = resolve(args[0] ?? "");
if (!args[0]) {
  console.error("usage: libmpv-source.mjs <out-dir> [--only a,b]");
  process.exit(2);
}
const only = args.includes("--only")
  ? new Set(args[args.indexOf("--only") + 1].split(","))
  : null;
const manifestPath = resolve("scripts/libmpv-sources.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const work = join(out, ".work");
mkdirSync(work, { recursive: true });

const git = (cwd, ...a) =>
  execFileSync("git", a, { cwd, stdio: ["ignore", "pipe", "inherit"] })
    .toString()
    .trim();

/** Fetch one commit (shallow) and archive it, optionally only some paths, under a prefix. */
function archive(name, repo, commit, file, prefix, paths = []) {
  const dir = join(work, name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  git(dir, "init", "-q", "--bare");
  git(dir, "fetch", "-q", "--depth", "1", repo, commit);
  if (git(dir, "rev-parse", "FETCH_HEAD") !== commit)
    throw new Error(`${name}: fetched commit differs`);
  git(
    dir,
    "archive",
    "--format=tar.gz",
    `--prefix=${prefix}`,
    "-o",
    join(out, file),
    commit,
    ...paths,
  );
  rmSync(dir, { recursive: true, force: true });
}

const wanted = (n) => !only || only.has(n);

for (const p of manifest.packages) {
  if (!wanted(p.name)) continue;
  const short = p.commit.slice(0, 12);
  console.log(`${p.name} ${short}`);
  archive(
    p.name,
    p.repo,
    p.commit,
    `${p.name}-${short}.tar.gz`,
    `${p.name}-${short}/`,
    p.paths ?? [],
  );
  for (const s of p.submodules ?? []) {
    const sub = s.path.replaceAll("/", "_");
    archive(
      `${p.name}-${sub}`,
      s.repo,
      s.commit,
      `${p.name}-${short}-${sub}.tar.gz`,
      `${p.name}-${short}/${s.path}/`,
    );
  }
}

for (const t of manifest.tarballs) {
  if (!wanted(t.name)) continue;
  console.log(`${t.name} ${t.url}`);
  const res = await fetch(t.url);
  if (!res.ok) throw new Error(`${t.name}: HTTP ${String(res.status)}`);
  const body = Buffer.from(await res.arrayBuffer());
  const algo = t.sha256 ? "sha256" : "sha1";
  const got = createHash(algo).update(body).digest("hex");
  if (got !== (t.sha256 ?? t.sha1))
    throw new Error(`${t.name}: ${algo} mismatch`);
  writeFileSync(join(out, decodeURIComponent(t.url.split("/").pop())), body);
}

if (wanted("recipe")) {
  const r = manifest.recipe;
  console.log(`recipe ${r.commit.slice(0, 12)}`);
  archive(
    "recipe",
    r.repo,
    r.commit,
    `mpv-winbuild-cmake-${r.commit.slice(0, 12)}.tar.gz`,
    "mpv-winbuild-cmake/",
  );
}

copyFileSync(manifestPath, join(out, "libmpv-sources.json"));
rmSync(work, { recursive: true, force: true });
const sums = readdirSync(out)
  .filter((f) => f !== "SHA256SUMS")
  .sort()
  .map(
    (f) =>
      `${createHash("sha256")
        .update(readFileSync(join(out, f)))
        .digest("hex")}  ${f}`,
  );
writeFileSync(join(out, "SHA256SUMS"), `${sums.join("\n")}\n`);
console.log(`${String(sums.length)} files in ${out}`);
