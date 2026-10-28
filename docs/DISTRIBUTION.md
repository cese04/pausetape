# Distributing NeatLibrary on macOS, Windows and Linux

*Plan and status, 2026-10-27.* Related: [PROGRESS](PROGRESS.md) · [TESTING](TESTING.md)
(Windows/Linux test guide) · [ARCHITECTURE](ARCHITECTURE.md) · [README](../README.md).

## Decisions (2026-10-27)

| Question | Decision |
|---|---|
| Platforms | **macOS first**; Windows and Linux after |
| Licence | **MIT** (`LICENSE`) |
| Apple Developer Program | **Not yet**: ad-hoc signed builds; users approve the app once in System Settings |
| Country (for Windows signing) | **Mexico**: Azure's individual tier is unavailable, so SignPath (free, needs a public open-source repo, which MIT qualifies for) or a Certum certificate, when Windows comes |
| ffmpeg | **Bundled**, as a minimal LGPL build made from source |
| Name | **"NeatLibrary" is a placeholder**; renaming is a few known edits (below) |

## Status: macOS (done)

`scripts/package-macos.sh` builds `dist/NeatLibrary.app` and `dist/NeatLibrary-<version>.dmg`
(+ SHA-256). Verified on this Mac (Apple Silicon, macOS 27):

| Piece | Result |
|---|---|
| Universal app (Apple Silicon + Intel), macOS 12+ | 47 MB app, 24 MB DMG |
| Bundled ffmpeg (`scripts/build-ffmpeg-macos.sh`) | FFmpeg 9.0.2, LGPL only, decoders/demuxers this app needs, universal, **8.3 MB**, links only system libraries. Converted a *Love (Dolby Atmos)* E-AC-3 track in 1 s. |
| App finds its own ffmpeg first | `NeatLibrary --version` → `ffmpeg: bundled (…/Contents/MacOS/ffmpeg)` |
| Info.plist | Version, category, minimum macOS, and the permission messages for SD cards, Documents, Downloads, Desktop, network drives |
| Icon | `assets/icon/icon-1024.png`, redrawn by `scripts/make-icon.py [letter]` |
| Licences in the app | `Contents/Resources/Licenses`: MIT, FFmpeg LGPL + build details, list of Rust crates and licences |
| Signing | Ad-hoc; `codesign --verify --strict` passes; Gatekeeper "rejected" as expected without Developer ID |
| DMG | App + Applications shortcut + "Read me first.txt" with the one-time approval steps |
| Automated builds | `.github/workflows/macos.yml`: test + package on every push, release on `v*` tags. **Written, not yet run** (no GitHub repo yet). |

Not verified: the **Intel half** (this Mac has no Rosetta, so x86_64 can't run here), and the
**first-launch approval** of a copy downloaded from the internet (only quarantined downloads
show the warning).

**FFmpeg 9.0.2 has no Dolby AC-4 decoder**, so AC-4 files (rare; some newer streaming
releases) still won't play. E-AC-3, AC-3 and TrueHD do.

**Renaming the app:** `APP_NAME` in `src/main.rs`; `APP_NAME` and `BUNDLE_ID` at the top of
`scripts/package-macos.sh`; the icon letter (`scripts/make-icon.py X`); README/docs. Keep
`APP_ID` in `src/main.rs` unless you also move users' settings folder.

## Goal

One release, three downloads that install like any other app:

| Platform | Users download | Installs as |
|---|---|---|
| macOS 12+ (Apple Silicon and Intel) | `NeatLibrary.dmg` | `NeatLibrary.app` in Applications, signed and notarised |
| Windows 10/11 (x64) | `NeatLibrary-setup.exe` | Start-menu app, signed if possible (see §4) |
| Linux (x86_64) | `NeatLibrary.AppImage` and `neatlibrary.deb` | AppImage runs anywhere; .deb for Debian/Ubuntu |

All three are built by one automated workflow when a version tag is pushed, and published as a
GitHub Release.

---

## 1. Code changes needed first

An audit of the current code found these gaps. The app has only ever run on macOS. Testers:
[TESTING §3](TESTING.md#3-known-gaps-found-by-reading-the-code-not-yet-seen-on-a-real-machine)
has what each looks like, with workarounds. Already done: tests no longer assume `/` paths; the
card step no longer says "not connected" where free space can't be read.

| # | Issue | Affects | Fix | Effort |
|---|---|---|---|---|
| 1 | **No drives found.** The card picker only looks in `/Volumes`, `/media/$USER`, `/run/media/$USER`. | Windows | List drive letters (`GetLogicalDriveStringsW`), skip the system drive, and read label + filesystem (`GetVolumeInformationW`) and free space (`GetDiskFreeSpaceExW`) via the `windows-sys` crate. | M |
| 2 | **No free space or filesystem type.** `card::space` uses `statvfs`, which is Unix-only. | Windows | Same Win32 calls as #1. | (in #1) |
| 3 | **Filesystem type unknown**, so the "not FAT32/exFAT" warning never shows. | Linux | `statfs` gives `f_type`: `0x4d44` = FAT, `0x2011BAB0` = exFAT. | S |
| 4 | **A console window opens behind the app.** | Windows | `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`; for `--plan`, attach to the parent console (`AttachConsole`). | S |
| 5 | **Reserved names.** Windows can't open files or folders named `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`9`, `LPT1`–`9` (with any extension). An artist called "Aux" would break the card write. | Windows (and cards later read on Windows) | `sanitize_component` appends `_` to those names. Add a test. | S |
| 6 | **Long paths.** Card paths can reach ~305 characters (80 + 100 + 120 per part), above Windows' traditional 260. Rust's file functions already handle long paths, but Explorer and other tools may not. | Windows | Make the per-write name-length cap from PROGRESS.md §2 #2 the default (e.g. 60/60/80). | S |
| 7 | **Fonts differ per OS.** The look depends on Palatino (macOS), Palatino Linotype (Windows) or DejaVu Serif (Linux). | All | Embed one serif font in the binary (`include_bytes!`): ET Book (check its licence) or an OFL face such as EB Garamond or Source Serif 4. Keep system fonts only as fallbacks for Chinese/Japanese/Korean, adding Windows (Yu Gothic, Malgun Gothic) and more Linux Noto paths, or evaluate eframe's `system_fonts` feature. | S |
| 8 | **Folder shortcuts assume macOS.** "Drives" and the card-folder browser start at `/Volumes`. | Windows, Linux | Per-OS start points: drive list on Windows, `/media`/`/run/media` on Linux. | S |
| 9 | **Unused microphone code.** rodio's default `recording` feature links audio-input code that is never used. | All | `rodio = { default-features = false, features = ["playback", "symphonia-all", …] }`. | S |
| 10 | **Install hints for ffmpeg** mention only `brew`. | Windows, Linux | Per-OS hint: `winget install Gyan.FFmpeg`, `sudo apt install ffmpeg`, etc. | S |

macOS-specific additions to the app bundle (no code changes):

- `Info.plist` usage descriptions, shown when macOS asks for permission:
  `NSRemovableVolumesUsageDescription` ("to write your music to the SD card"),
  `NSDocumentsFolderUsageDescription`, `NSDownloadsFolderUsageDescription`,
  `NSDesktopFolderUsageDescription` (if the music folder is there).
- `LSApplicationCategoryType = public.app-category.music`, `NSHighResolutionCapable = true`.

Release build settings (smaller, faster binary; it's 26 MB today):

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

---

## 2. Packaging

**Tool: [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)** (by CrabNebula,
from the Tauri ecosystem, but it works with any Rust binary). One configuration produces macOS
.app/.dmg, Windows NSIS .exe and MSI, and Linux .deb, AppImage and pacman packages
([docs](https://docs.crabnebula.dev/packager/)). It is marked *public preview*. If it gets in
the way, the fallbacks are `cargo-bundle` (macOS .app/.deb), `create-dmg`, NSIS or WiX run
directly, and `linuxdeploy` for AppImage.

Configuration sketch for `Cargo.toml`. Key names must be checked against the cargo-packager docs
before use:

```toml
[package.metadata.packager]
product-name = "NeatLibrary"
identifier = "io.github.<github-user>.neatlibrary"
version = "0.1.0"
icons = ["assets/icon/32x32.png", "assets/icon/128x128.png", "assets/icon/icon.icns", "assets/icon/icon.ico"]
before-packaging-command = "cargo build --release"
out-dir = "dist"
binaries = [{ path = "neatlibrary", main = true }]

[package.metadata.packager.macos]
minimum-system-version = "12.0"
info-plist-path = "assets/macos/Info.plist"     # the usage descriptions above

[package.metadata.packager.deb]
depends = ["libasound2"]                         # audio; ffmpeg as "recommends"
```

Per platform:

- **macOS.** Build `aarch64-apple-darwin` and `x86_64-apple-darwin`, merge them with `lipo` into
  one universal binary, then package. A DMG with the usual "drag to Applications" window.
- **Windows.** NSIS installer: per-user install (no admin rights), Start-menu shortcut,
  uninstaller. Icon and version info embedded in the .exe (`winresource` or
  `embed-resource`). MSI only if someone asks for it.
- **Linux.** Build on Ubuntu 22.04 so the binary runs on older distributions too (glibc
  2.35). The AppImage bundles everything except system audio (ALSA/PipeWire) and graphics
  drivers. The .deb declares its dependencies. A `.desktop` file and icons go in both. The
  build machine needs `libasound2-dev`, `pkg-config`, `libxkbcommon-dev`, `libwayland-dev`.
  The file dialog uses the desktop's portal service (rfd's default `xdg-portal`), so no GTK
  dependency. **Flatpak/Flathub later:** its sandbox needs explicit access to `/media` and
  `/run/media` for SD cards, and ffmpeg via the `org.freedesktop.Platform.ffmpeg-full` extension.

**App icon:** one 1024×1024 master turned into `.icns`, `.ico` and PNG sizes. In keeping with
the look: a serif "N" or a small folder on warm paper.

---

## 3. ffmpeg

**Bundled** (decided). Instead of a downloaded 80–100 MB GPL build, the app ships a minimal
build made from FFmpeg's source by `scripts/build-ffmpeg-macos.sh`. It contains only the
demuxers and decoders the app needs plus a WAV writer. It's LGPL v2.1 (no `--enable-gpl`),
8.3 MB universal, with source URL, checksum and configure flags in `BUILD-INFO.txt` inside the
app. ffmpeg runs as a separate program the app calls, not linked into it, so the app's MIT
licence is unaffected. The LGPL obligations are the licence text and where to get the source,
both shipped.

If "convert while writing to the card" (PROGRESS.md §2 #1) is built, add the FLAC encoder
(LGPL) and LAME for MP3 (LGPL) to the build. Windows and Linux need the same script adapted.

---

## 4. Signing

Unsigned apps work, but each OS warns first. On macOS 15, the user has to approve the app in
System Settings › Privacy & Security; there's no right-click shortcut any more. On Windows,
SmartScreen shows "Windows protected your PC".

| Platform | Option | Cost | Notes |
|---|---|---|---|
| macOS | **Apple Developer Program**: Developer ID certificate + notarisation (`notarytool`) | US$99/year | The only way to avoid Gatekeeper warnings. Fully scriptable in CI with an App Store Connect API key. |
| Windows | **Azure Artifact Signing** (formerly Trusted Signing) | US$9.99/month (Basic) | **Individuals: only in the US and Canada.** Organisations also in the EU and UK ([Microsoft's options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options), [pricing](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/)). Identity check takes a few business days. |
| Windows | **SignPath Foundation** | Free for open-source projects | Requires a public repository and an OSS licence; they approve the project. |
| Windows | Traditional OV certificate (e.g. Certum's open-source certificate) | roughly tens to a few hundred US$/year | Works from most countries; the key lives in a cloud HSM or token, so CI integration is more work. |
| Linux | None needed | – | Publish SHA-256 checksums with the release; optionally sign them. |

Even signed, a new Windows app starts with no SmartScreen reputation. Warnings fade as downloads
accumulate.

---

## 5. Automated builds

GitHub Actions, triggered by pushing a tag like `v0.1.0`:

| Job | Runner | Produces |
|---|---|---|
| macOS | `macos-14` (Apple Silicon), adding the x86_64 target | universal `.dmg`, signed + notarised when secrets exist |
| Windows | `windows-latest` | NSIS `.exe`, signed when secrets exist |
| Linux | `ubuntu-22.04` | `.AppImage`, `.deb` |
| Release | after all three | GitHub Release with the files, SHA-256 checksums, changelog excerpt |

Every push (not just tags) also runs `cargo test` and a release build on all three platforms,
so portability breaks are caught early. **This part exists:** `.github/workflows/ci.yml`
(build + test on macOS, Windows, Linux, uploading each binary) and
`.github/workflows/macos.yml` (DMG, attached to `v*` releases). Neither has run yet.

Sketch (to be fleshed out in Phase 3):

```yaml
on:
  push:
    tags: ["v*"]
jobs:
  build:
    strategy:
      matrix:
        include:
          - { os: macos-14,       formats: dmg }
          - { os: windows-latest, formats: nsis }
          - { os: ubuntu-22.04,   formats: "deb,appimage" }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - if: runner.os == 'Linux'
        run: sudo apt-get install -y libasound2-dev pkg-config libxkbcommon-dev libwayland-dev
      - run: cargo test
      - run: cargo install cargo-packager --locked
      - run: cargo packager --release --formats ${{ matrix.formats }}
      # + signing/notarisation steps, then upload dist/* as artifacts
```

Secrets needed: Apple certificate (.p12, base64) and password, App Store Connect API key (for
notarisation), and Windows signing credentials for whichever option is chosen.

---

## 6. Getting it to people, and updates

- **First:** GitHub Releases, linked from the README with a short "which file do I need?" table.
- **Then, per platform:** Homebrew cask (macOS, requires notarisation), winget and Scoop
  (Windows), AUR (Arch). Flathub once the sandbox questions above are solved.
- **Updates, v1:** an optional check against the GitHub "latest release" API at startup,
  showing a quiet "Version x.y available" link in Settings. No auto-install.
- **Updates, later:** in-app updating (Sparkle on macOS, or a cross-platform updater) once
  signing is in place, since updates must be signed.
- **Versioning:** semantic versions, `CHANGELOG.md`, and the version shown in Settings.

Where user data lives (already handled by eframe, worth documenting for users):
macOS `~/Library/Application Support/NeatLibrary`, Windows `%APPDATA%\NeatLibrary\data`,
Linux `~/.local/share/neatlibrary`.

---

## 7. Release checklist (manual, each version)

On real machines or VMs: macOS (Apple Silicon + Intel), Windows 10 and 11, Ubuntu 24.04,
Fedora (Wayland).

- [ ] Installs and launches from the downloaded file; no console window; icon shows correctly.
- [ ] Pick a music folder; scan; rescan uses the database.
- [ ] Playback, seeking, waveform, cover art; ffmpeg found or install hint shown.
- [ ] A real SD card appears in the card picker with the right name, free space and filesystem.
- [ ] Write to the card; check folder names on the Echo Mini itself; write again (nothing
      re-copied); deselect an album and write (it's removed).
- [ ] Text in Chinese/Japanese/Korean renders; window works at 720 px and on HiDPI screens.
- [ ] Uninstall leaves no app files behind (user data may stay, documented).

---

## 8. Licensing

- Choose the app's licence (MIT/Apache-2.0 matches most dependencies; required anyway for
  SignPath's free signing).
- Dependencies are permissive (MIT/Apache) except **Symphonia (MPL-2.0)**, which allows use in a
  closed or open app as long as changes to its own files are shared. SQLite is public domain.
- Generate a third-party notices file automatically (`cargo-about`) and include it in each
  package and in Settings › About.

---

## 9. Phases

| Phase | Work | Effort | Result |
|---|---|---|---|
| 0. Prepare | git repository on GitHub; licence; version 0.1.0; icon; release profile | S | A home for releases |
| 1. Portability | Fixes 1–10 from §1, with tests where possible (reserved names, `f_type` mapping) | M | App behaves correctly on all three |
| 2. Package locally | cargo-packager config; build each format once by hand on each OS | M | Installable files |
| 3. Automate | GitHub Actions on every push + tagged releases (unsigned) | M | Public beta downloads |
| 4. Sign | Apple Developer + notarisation; Windows route chosen in §4 | S–M (+ days of identity checks) | No security warnings |
| 5. Reach | Homebrew, winget, Scoop, AUR; update check | S each | Easy installs and updates |

A sensible first milestone is Phases 0–3: unsigned beta builds for all three platforms, with
the macOS build signed as soon as the Apple account exists.

---

## 10. Still open

1. **Final name** and bundle identifier (placeholder: `NeatLibrary`, `io.github.neatlibrary.app`).
2. **Public repository?** Needed for the free SignPath route on Windows, and the natural home
   for releases and the macOS workflow.
3. **Apple Developer Program** when it makes sense (removes the first-launch approval; enables
   Homebrew cask and auto-updates).
