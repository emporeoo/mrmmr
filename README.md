# MRMMR

## Marvel Rivals Mod Manager Redux

<a href="https://ko-fi.com/emporeo">
    <img src="https://ko-fi.com/img/githubbutton_sm.svg" alt="Donate on Ko-fi" width="350">
</a>

MRMMR is a modern, lightweight desktop mod manager dedicated to Marvel
Rivals. It provides an organized workshop, verified installations, readable
asset conflict reports, and local tools for maintaining a modded game
installation.

MRMMR is designed around a simple promise: every installation should be
verified, reversible, and explainable.

## Features

- Organized Nexus Mods workshop browsing with sorting, search, categories,
  pagination, and infinite scrolling.
- Nexus account-tier awareness. Premium accounts can use direct API downloads
  when Nexus provides them. Free and unsupported download flows open Nexus in
  the browser and validate the downloaded archives locally.
- Installation previews showing archive identity, file changes, replacements,
  required disk space, UTOC requirements, and asset-level conflicts before
  changes are applied.
- Nexus file identity and archive checksum validation, including multipart
  file grouping and protection against selecting an archive belonging to
  another mod.
- Native extraction for ZIP, 7z, RAR, TAR, GZ, and TGZ archives without
  requiring WinRAR or another separately installed archive program.
- Installed mod management with enable, disable, update, uninstall, archive
  cleanup, mod links, and installed-size statistics.
- Asset-level conflict reporting that identifies the internal asset path and
  the other enabled mod providing it.
- Transactional installation changes with a one-step undo action and rollback
  safeguards.
- Mod Doctor diagnostics for missing files, orphaned files, duplicate
  ownership, invalid disabled states, missing UTOC setup, and related
  installation problems.
- Local diagnostic export with redacted reports that can be shared when
  requesting help.
- Automatic Marvel Rivals installation detection for Steam and Epic Games,
  with a custom location option and an Open mods folder shortcut.
- UTOC Signature Bypass setup and status checking.
- Game launch protection that checks the platform launcher, prevents duplicate
  game launches, and blocks game-file changes while Marvel Rivals is running.
- Optional deletion of downloaded mod archives after successful extraction.

## Download and install

Use the official GitHub release page:

<https://github.com/emporeoo/mrmmr/releases/latest>

1. Under **Assets**, download the ZIP file.
2. Extract the ZIP and run the EXE file.
3. On first launch, select **Continue with Nexus Mods**.
4. Sign in on the official Nexus Mods page opened by MRMMR and approve the
   application authorization.
5. Return to MRMMR, confirm the detected Marvel Rivals installation, and
   install the required UTOC Signature Bypass before installing mods.

Only download MRMMR from the official GitHub repository or an official Nexus
Mods listing. Do not use repacked installers or unofficial mirrors.

## Nexus Mods authentication

MRMMR uses Nexus Mods SSO protocol 2 over the official SSO WebSocket. After
the user approves MRMMR in a browser, Nexus Mods returns an
application-scoped credential for authenticated REST and GraphQL requests.
MRMMR does not accept manually entered credentials.

The SSO implementation is ready for application registration. The assigned
application slug is currently represented by the centralized
`PENDING_NEXUS_APPLICATION_SLUG` placeholder in
[`src/lib/nexusSso.ts`](src/lib/nexusSso.ts). Once Nexus Mods supplies the
registered slug, that single value enables sign-in.

## Privacy and safety

MRMMR is a local-first desktop application:

- There is no MRMMR backend, telemetry service, advertising SDK, or automatic
  diagnostic upload.
- Network requests are made directly from the application to Nexus Mods for
  account validation, workshop browsing, file metadata, and permitted
  downloads.
- Nexus Mods authentication uses its browser-based SSO flow. MRMMR never asks
  you to copy a credential from your account settings.
- The application-scoped SSO credential is encrypted with Windows DPAPI and
  stored only on your device. It is not stored on a MRMMR server.
- Mod archives are downloaded and extracted locally. MRMMR does not rehost
  mod files.
- Diagnostic reports are created and exported only when you choose. They do
  not include the stored Nexus authorization, and exported reports are
  designed to avoid exposing the full local game path.
- MRMMR does not inject into Marvel Rivals or modify game memory. Process
  checks are used for launch protection and to prevent unsafe changes while
  the game is running.
- Installation operations use temporary files, archive validation, safe path
  checks, and rollback data to reduce the chance of an incomplete or
  cross-mod file change.

MRMMR modifies the configured Marvel Rivals mod directory and, when requested,
the UTOC Signature Bypass files. Keep backups of important game data and
follow the instructions supplied by mod authors.

## Help and community

For reproducible problems, open an issue in the GitHub repository:

<https://github.com/emporeoo/mrmmr/issues>

When reporting an issue, include the MRMMR version, relevant steps to
reproduce the problem, and the exported diagnostic report if appropriate.
Never post access tokens, authorization credentials, or private system
information.

Visit the official Nexus Mods page for MRMMR, endorse the
work, and vote where applicable:

<https://www.nexusmods.com/marvelrivals/mods/11829>

Support development on Ko-fi:

<https://ko-fi.com/emporeo>

## Source visibility and licensing

MRMMR is source-visible proprietary software. The source is published for
user trust, moderation review, and security inspection. It is not open source.
The source, original assets, interface, branding, and creator-authored
release materials may not be copied, modified, redistributed, rebranded, or
used to create another application without written permission.

Official, unmodified releases may be downloaded and used personally under the
terms in [LICENSE](LICENSE). Third-party dependencies remain subject to their
own licenses and notices.
