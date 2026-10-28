# Changelog

Everything built so far, in the order it happened. All of it is unreleased work towards
**0.1.0**, developed on 2026-10-27 on macOS. Status and next steps: [docs/PROGRESS.md](docs/PROGRESS.md).

## [Unreleased]: 0.1.0

### 1. First version
- Rust + egui app chosen over Tauri (one language, one process, small binary).
- Scans a music folder with `lofty`; falls back to `Artist/Album/03 - Title` folder names.
- Artist matching: spelling variants merge automatically; near misses are suggested.
- MusicBrainz lookups for albums and artists, applied only when you accept them.
- Organise into `Artist/1997 - Album/01 - Title.flac` (multi-disc `1-01`, compilations under
  *Various Artists*), FAT-safe names, by copying to a card or moving in place with undo.
- Playlists exported as folders of numbered copies plus `.m3u8`, because FAT/exFAT have no
  symlinks.
- Player with a loudness waveform as the seek bar; Tufte-inspired look (serif, warm paper,
  red only for the playhead).
- `--plan` dry run; 8 tests; debug screenshot harness.

### 2. Folders and saved writes
- *Folders* tab: separate music folder (may stay messy) and SD card; in-app folder browser.
- *Card* tab: named **writes** (what goes on a card, card-only names, album order including a
  custom order) with a manifest, so rewrites copy only changes and remove only their own files.
- Reorganising the music folder became optional.

### 3. Clearer setup, library and player
- Folders tab redesigned into two numbered steps with dialogs for choosing; *Advanced* section
  for reorganising.
- Library: each album shows its folder(s) with *open folder*; A–Z rail with letter headings.
- Player: real round previous/play/next buttons.

### 4. Library database
- SQLite cache of tags keyed by path, size and modification time: rescans only read changed
  files (2,040 files: 280 ms → 25 ms). Unsaved corrections survive restarts.

### 5. Player polish
- Fixed misaligned transport buttons.
- Cover art (embedded, else `cover.jpg`/`folder.jpg`/… in the folder).

### 6. Formats RECHO can't play
- Playback of AAC/M4A, ALAC, AIFF, Ogg, MP2, MKA, CAF (all Symphonia codecs enabled).
- Clear warnings: banner, per-track badge, per-album count, player note; on the card these
  files are left out unless you choose *copy them anyway*.

### 7. Layout fixes
- List no longer runs under the A–Z rail; a crash with open MusicBrainz suggestions fixed; the
  library fills wide windows; long folder names and titles shorten with "…".

### 8. Dolby Atmos and other codecs
- The real codec inside M4A files is detected (e.g. Dolby E-AC-3 in Atmos releases).
- Files the built-in decoder can't read are converted with ffmpeg to a temporary stereo WAV
  (Dolby E-AC-3/AC-3, TrueHD, DTS, Opus, APE, WavPack, WMA). FFmpeg has no AC-4 decoder.

### 9. Documentation
- `docs/PROGRESS.md` (status, verification, roadmap, UI/UX ideas) and `docs/DISTRIBUTION.md`
  (packaging plan for macOS, Windows, Linux).

### 10. macOS app
- MIT licence. Display name in one constant (`APP_NAME`; "NeatLibrary" is a placeholder).
- `scripts/build-ffmpeg-macos.sh`: minimal LGPL ffmpeg 9.0.2 from source, universal, 8.3 MB.
- `scripts/package-macos.sh`: universal `.app` (47 MB) and `.dmg` (24 MB) with ffmpeg inside,
  icon (`scripts/make-icon.py`), licences, permission texts, ad-hoc signature, "Read me first".
- `--version` shows the version and which ffmpeg is used; the bundled one is preferred.
- `.github/workflows/macos.yml`: package and release on tags (not yet run).
- git repository initialised (nothing committed).

### 11. Preparing for Windows and Linux testing
- Two tests no longer assume `/` as the path separator.
- The card step no longer claims a connected card is "not connected" where free space can't be
  read (Windows).
- The screenshot demo never saves its sample playlist over real settings.
- `.github/workflows/ci.yml`: build and test on macOS, Windows and Linux on every push.
- Documentation reorganised into `docs/` with a Windows/Linux test guide
  (`docs/TESTING.md`), an architecture overview (`docs/ARCHITECTURE.md`), and issue templates
  for test reports and bugs.

### 12. Source restructured (before the first commit)
- `src/` reorganised from 22 flat files into areas: `library/`, `card/`, `audio/`, `app/`, `ui/`,
  plus `platform.rs` and `musicbrainz.rs`. The large files were split by job: `views.rs` into
  one file per tab, `theme.rs` into theme / widgets / text, `organize.rs` into plan / copy /
  journal, `app.rs` into topic files. Code moved verbatim; behaviour unchanged (tests, CLI and
  screenshots checked).
- Only `ui/` and `app/` use egui: cover art now comes out of `library/` as plain pixels.
- The data folder is computed in one place (`platform::data_dir`).
- Dolby AC-4 is now correctly reported as unplayable even with ffmpeg (FFmpeg has no decoder);
  texts no longer claim otherwise. New test for the format rules (16 tests).
