// Writes the in-app update feed (ADR-0015): the latest.json of the highest published vX.Y.Z release that has one.
// Pre-releases count (every Granary release is one); drafts and -dev builds never do. No release with a feed
// entry yet: nothing is written and installed apps simply find no update.
// usage: node site/scripts/update-feed.mjs <owner/repo> <out-file>
import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

const [repo, out] = process.argv.slice(2);
if (!repo || !out) {
  console.error("usage: update-feed.mjs <owner/repo> <out-file>");
  process.exit(2);
}

const headers = {
  Accept: "application/vnd.github+json",
  "User-Agent": "granary-pages",
};
if (process.env.GH_TOKEN)
  headers.Authorization = `Bearer ${process.env.GH_TOKEN}`;

const res = await fetch(
  `https://api.github.com/repos/${repo}/releases?per_page=100`,
  { headers },
);
if (!res.ok) throw new Error(`releases: HTTP ${String(res.status)}`);
const releases = await res.json();

const semver = (tag) =>
  /^v(\d+)\.(\d+)\.(\d+)$/.exec(tag)?.slice(1).map(Number);
const newer = (a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];

const candidates = releases
  .filter(
    (r) =>
      !r.draft &&
      semver(r.tag_name) &&
      r.assets.some((a) => a.name === "latest.json"),
  )
  .sort((a, b) => newer(semver(b.tag_name), semver(a.tag_name)));

const best = candidates[0];
if (!best) {
  // No process.exit here: with fetch handles still open it trips a libuv assertion on Windows.
  console.log(
    "no published release with latest.json yet; no update feed written",
  );
} else {
  await writeFeed(best);
}

async function writeFeed(best) {
  const asset = best.assets.find((a) => a.name === "latest.json");
  const feedRes = await fetch(asset.browser_download_url, {
    headers: { "User-Agent": "granary-pages" },
  });
  if (!feedRes.ok)
    throw new Error(
      `latest.json of ${best.tag_name}: HTTP ${String(feedRes.status)}`,
    );
  const feed = await feedRes.json();
  if (
    feed.version !== best.tag_name.slice(1) ||
    !feed.platforms?.["windows-x86_64"]?.signature
  ) {
    throw new Error(
      `latest.json of ${best.tag_name} is not a valid feed entry`,
    );
  }

  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, `${JSON.stringify(feed, null, 2)}\n`);
  console.log(`update feed: ${best.tag_name}`);
}
