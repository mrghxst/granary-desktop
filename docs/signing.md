# Code signing (Windows Authenticode)

**Status: not configured.** Installers are unsigned. Nothing in the repository holds a certificate, key or signing
secret. This page is the owner's checklist for turning signing on. Last checked 2026-11-18; prices and eligibility
rules change, so confirm them on the provider's page before paying or applying.

Authenticode signing is separate from the **updater signature** (ADR-0015): the updater key only proves to an
installed Granary that an update is ours; Windows does not know it. Authenticode is what Windows SmartScreen and
Smart App Control look at.

## What unsigned means for testers

- **SmartScreen:** the installer shows "Windows protected your PC". Click **More info → Run anyway**. Reputation
  resets with every unsigned version. With the in-app updater (ADR-0015) testers see this only for the first install.
- **Smart App Control** (Windows 11, on by default on some clean installs) blocks unsigned executables with no
  reputation outright. On some Windows versions it cannot be turned back on without resetting Windows, so do not ask
  testers to turn it off; tell them the app will run once it is signed.
- Integrity holds without a signature: every installer ships with a `.sha256` file, and updates are verified against
  the updater key.
- A signature does not make SmartScreen silent at once: since 2024 every certificate (OV and EV) builds reputation
  through downloads over days or weeks. What signing gives at once: a named publisher, reputation that carries over
  from version to version, and no Smart App Control block once reputation exists.

## The options

| Option                                                | Cost                                                                                           | Works in GitHub Actions               | Who can get it                                                                                                | Publisher shown                      |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| **SignPath Foundation** (recommended)                 | free for open-source projects                                                                  | Yes, built for it                     | OSI-licensed public projects, after review                                                                    | "SignPath Foundation"                |
| **Certum Open Source Code Signing**                   | about €69 a year for the card set (official shop; the cloud variant's price was not confirmed) | Hard: card or phone-based cloud login | Individual open-source developers, identity check                                                             | "Open Source Developer, <your name>" |
| **Azure Artifact Signing** (formerly Trusted Signing) | $9.99 a month (Basic)                                                                          | Yes                                   | Organisations in the USA, Canada, EU and UK; **individuals only in the USA and Canada** (Microsoft Q&A, 2026) | your name or organisation            |
| OV/EV certificate from a commercial CA                | roughly €200 to €500 a year                                                                    | Only with a cloud HSM                 | anyone, identity check                                                                                        | your name or organisation            |

**Recommendation:** apply to the SignPath Foundation first. It costs nothing, signs in CI from the existing release
workflow, and Granary qualifies (AGPL-3.0, public repository, real releases). If the application is refused or takes
too long, Certum's open-source certificate is the cheapest certificate in your own name, but it is awkward to
automate. Use Azure only if you are eligible where you live.

## Option A: SignPath Foundation (free)

What you prepare:

1. **Two-factor authentication** on your GitHub account (required for everyone with signing rights).
2. A **code signing policy page** in the repository or on the website. It states who may approve a signing, that only
   builds from this repository's release workflow are signed, the team roles (you as author, reviewer and approver),
   and the privacy note that Granary sends nothing to third parties without the user's action. Many projects publish
   this as `CODE_SIGNING.md`; Granary can do the same.
3. The line **"Free code signing provided by SignPath.io, certificate by SignPath Foundation"** on the website and in
   the README (a condition of the program).
4. Accept that the publisher shown in Windows is **SignPath Foundation**, not your name.
5. Apply on signpath.org with: the repository URL, the licence, a short description, the release page, and the build
   system (GitHub Actions, `.github/workflows/release.yml`).

After approval (the maintainer side, done here once you have access):

1. SignPath gives an organisation id, a project slug and a signing policy; you create an API token for CI and store it
   as the GitHub secret `SIGNPATH_API_TOKEN` (never in the repository).
2. The release workflow uploads the unsigned installer artifact to SignPath with the official `signpath/github-action-submit-signing-request` action and
   downloads the signed result before publishing. SignPath checks that the artifact came from this repository's
   workflow run.
3. `granary.exe` and `libmpv-2.dll` are signed inside the installer too: SignPath can sign nested files when the
   artifact configuration lists them, or the workflow signs the binaries first and the installer second.
4. Afterwards `Get-AuthenticodeSignature <file>` must report `Valid` for the installer, `granary.exe` and
   `libmpv-2.dll`, and the `.sha256` files and the updater signature are written after signing (a signed file is
   never changed again).

## Option B: Certum Open Source Code Signing

What you prepare:

1. An identity document (passport or ID card) and a recent proof of address; Certum verifies you as a person.
2. Choose the variant: the **card set** (smart card and USB reader, shipped by post) or **SimplySign** (cloud key,
   unlocked with a one-time code from the SimplySign phone app).
3. Order in the Certum shop, activate the certificate (Certum's activation manual walks through it), and note the
   certificate's SHA-1 thumbprint.

Signing then happens **on your PC**, because the key never leaves the card or needs your phone:

1. Build the installer as usual (`scripts/release-build.ps1`), then sign `granary.exe`, `libmpv-2.dll` and the
   installer with `signtool sign /sha1 <thumbprint> /fd sha256 /tr http://time.certum.pl /td sha256 <file>`.
   Tauri can do this itself: set `bundle.windows.certificateThumbprint`, `digestAlgorithm: "sha256"` and
   `timestampUrl: "http://time.certum.pl"` in `tauri.conf.json`.
2. Upload the signed installer to the release. This replaces the CI build for releases, so it is the least convenient
   option.

## Option C: Azure Artifact Signing

Only if you (or an organisation you own) are in a supported country.

1. An Azure account with a paid subscription; create an Artifact Signing account in a supported region.
2. Identity validation (individual or organisation) in the Azure portal; this can take days.
3. A certificate profile ("Public Trust").
4. In GitHub: a federated credential (OIDC) for the repository, so no secret is stored; `id-token: write` in the
   signing job.
5. Signing via `signCommand` in `bundle.windows`, for example
   `artifact-signing-cli -e https://<region>.codesigning.azure.net -a <account> -c <profile> -d Granary %1`.

## What stays the same in every option

- Never change a file after signing. The SHA-256 file and the updater signature are made from the signed installer.
- Timestamps are mandatory, so signatures stay valid after the certificate expires.
- The updater key (ADR-0015) is unaffected; signed and unsigned versions update each other normally.

## Sources

- SignPath Foundation: <https://signpath.org/>, terms <https://signpath.org/terms>
- Certum open-source code signing: <https://shop.certum.eu/open-source-code-signing.html>,
  <https://shop.certum.eu/open-source-code-signing-on-simplysign.html>
- Azure Artifact Signing pricing: <https://azure.microsoft.com/en-gb/pricing/details/artifact-signing/>; eligibility
  discussion: <https://learn.microsoft.com/en-us/answers/questions/5810735/cant-create-a-new-trusted-signing-individual-ident>
- Older research and dates: [research/packaging-and-licensing.md](research/packaging-and-licensing.md) §4
