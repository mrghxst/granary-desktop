# ADR-0015: In-app updates with "Restart to update"

- Status: Accepted (supersedes the "no self-update" part of ADR-0005)
- Date: 2026-11-18
- Inputs:
  - Owner request (2026-11-18): updating should be as simple and fast as "restart to update" in AIOStreams, before
    other people test the app.
  - `tauri-plugin-updater` 2.14.1 source (`src/updater.rs`: `download` verifies the minisign signature,
    `install` runs the NSIS installer with `/UPDATE` and the restart arguments, then exits the process;
    `on_before_exit` runs first; `src/config.rs`: non-https endpoints are refused in release builds).

## Context

- Until 0.6.1 Granary only linked to the releases page; every tester had to download and run each installer by hand.
- Releases are public **pre-releases** (owner's decision, 2026-11-16), so GitHub's `/releases/latest/download/...`
  URL, which skips pre-releases, cannot serve as the feed.
- The installer is not Authenticode-signed yet (docs/signing.md); the update must still be provably ours.
- Installing replaces the running app. Playback can hold a display switched to HDR or another refresh rate that must
  be restored (ADR-0006), and a progress report that must reach the server.

## Decision

- **Plugin:** `tauri-plugin-updater` (Rust side only; the WebView gets three app commands `update_status`,
  `update_check`, `update_install` and an `update-status` event, no plugin permissions).
- **Signature:** every installer is signed with a minisign key whose public half is in `tauri.conf.json`
  (`plugins.updater.pubkey`). The private key and its password live outside the repository (owner's machine, GitHub
  secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). An installer that does not verify is
  never installed. Losing the key means installed apps can no longer update themselves (a new key needs one manual
  install), so the owner keeps a backup.
- **Feed:** `https://mrghxst.github.io/granary-desktop/updates/latest.json`. The release workflow signs the installer in
  its publishing job (no checkout; only the pinned Tauri CLI sees the key) and attaches `<installer>.sig` and
  `latest.json` to the release. The pages workflow runs after every Release run and copies the `latest.json` of the
  highest published `vX.Y.Z` release that has one (`site/scripts/update-feed.mjs`). Drafts and `-dev` builds never
  reach the feed. Without the key secret a release is still published, only without a feed entry.
- **Flow:** 20 s after start and then every 6 h (setting "Download updates automatically", on by default) the feed is
  checked; a newer version is downloaded and verified in the background. Nothing is installed until the user picks
  **Restart to update** (sidebar button, About → Updates). Then playback is shut down as on quit (final position,
  session released, display restored), the log is flushed, the installer runs in passive mode (progress only, no
  questions) and reopens Granary. After the update a one-time notice links to the release notes.
- **Development builds** never check the real feed, so a debug build cannot replace the installed app; they check only
  `GRANARY_UPDATE_FEED` (http allowed there), which is how the flow was tested.

## Consequences

- Testers install once by hand; every later version is one click.
- Updates from 0.6.1 or older still need the installer once (they have no updater).
- The key is a long-lived secret: it is backed up by the owner and never committed (`/.secrets/` is ignored).
- The updater trusts the signature, not the transport or the feed host; a compromised feed can at most offer an
  older or no update, not a foreign installer.
