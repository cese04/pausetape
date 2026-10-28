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

### 13. Planned fixes documented (2026-10-28)
- `docs/NEXT.md`: why MusicBrainz suggestions can come from the wrong album and how to make
  lookups safer; folder and file names that are misread (an artist called "1979", nested
  collection folders, several editions of one album); support for albums stored as one file
  plus a `.cue` sheet (library, playback, splitting on the card). Not implemented yet.

### 14. Safer MusicBrainz lookups and better folder-name reading (2026-10-28)
- Folder and file names:
  - `1979 - Album.flac` gives a year, not an artist "1979"; `1-03 - Title` gives disc 1, track 3.
  - Album folders like `1979. Album {edition}`, `Album (1997) [FLAC]` and `Artist - Album (1997)`
    lose the year and edition details; the year is kept as the year.
  - The artist is taken from the nearest folder above that isn't itself an album, without
    endings like " - коллекция" or " [restored]"; `CD 1` folders give the disc number.
  - Measured on the test library: Joy Division, The Cure, Cigarettes After Sex and Ritchie
    Kotzen & Greg Howe albums are no longer filed under "1979"/"1980"/"1988" or as singles.
  - Albums named only by their folder may get a new identity (e.g. "Album (2017) [LP]" is now
    "Album"), so a write's exclusions or card names for those albums reset once.
- MusicBrainz:
  - Folder names are cleaned before searching ("The Beatles - Love (2006) [Dolby Atmos]" → "Love").
  - Never searches for an artist called "Unknown Artist".
  - Results must have the same artist and title; the exact title beats a similar one.
  - Up to 3 releases are checked against the songs. Songs are paired by track number or length
    only after enough titles agree, or every length lines up, so a wrong album can't hand out its
    titles. Otherwise: "no confident match", naming the releases that were checked.
  - Titles with a track number or band name in front ("01 Joy Division - Disorder") still match.
  - The result line shows the release's country, label and track count; long messages wrap.
  - Single songs without an album: an unknown artist is replaced by the song's length in the search.
- 24 tests (21 offline), new ones for names, folders, ranking and the album check.

### 15. Albums stored as one file plus a CUE sheet (2026-10-28)
- **Library:**
  - An album file with a `.cue` sheet shows as its songs, with the sheet's titles, artists,
    album and year; the file's own tags only fill gaps.
  - Sheets in Windows-1251/1252 and other legacy encodings are read correctly.
  - Sheet file names that differ in case or extension (`.FLAC`, `.wav` for a `.flac`) still
    match.
  - Sheets for already-split albums fill missing tags without splitting anything.
  - On the test library: 9 album files became 104 songs.
- **Player:** plays just the song's part of the file, sample-exact; seeking and the waveform
  are per song.
- **Card:**
  - Each song is cut into its own FLAC with ffmpeg and tagged. This is lossless; FLAC, WAV,
    APE, WavPack, AIFF and ALAC album files all become FLAC.
  - Rewrites skip songs already cut.
  - Songs in lossy album files, or with no ffmpeg available, are left out with a reason.
- **Reorganise** leaves album files and their sheets in place. Corrections to these songs stay
  in the app and reach the card copies.
- **Fixes:**
  - Cover art prefers conventional names, then an image named after the audio file, then
    `Covers/Front.jpg`-style scans, and never a spectrogram or back cover.
  - Duplicate card names are numbered "(2)", "(3)" instead of "(2) (3)".
  - Mixed-case artist spellings beat all-caps ones on ties.
  - The status line says "files unchanged".
- **ffmpeg:** the bundled build adds the FLAC encoder, FLAC writer and `atrim` filter (8.4 MB);
  `scripts/build-ffmpeg-macos.sh` rebuilds when its flags change.
- Debug demo: `NEATLIBRARY_SHOT_ARTIST` chooses the album played and looked up.
- 33 tests (28 offline); new opt-in checks against a real album: `section_matches_cut`,
  `cue_album_write`.

### 16. Editions, same-name albums and cover art in tags (2026-10-28)
- **Editions.** Copies of the same album in different folders (two rips, a reissue, a vinyl
  rip) are now editions instead of one album with every song twice:
  - The library lists them as "edition 1 of 3", with the edition details on hover.
  - The card gets one, by default the most complete lossless copy RECHO plays; tick another
    in the card tab to switch.
  - On the test library this caught 10 duplicated albums and made a full write about 1.5 GB smaller.
- **Same-name albums.** Different albums with the same name (American Football's three
  self-titled records) stay apart, by year. On the card they're "1999 - …", or
  "American Football (2016)" when albums aren't ordered by year.
- Album IDs change only for albums that were split. Exclusions and card names saved before
  still apply to all their editions.
- **Cover art in tags.** When the cover the player shows comes from an image file and not from
  the songs' tags, it suggests adding it.
  - "add it" embeds the image (resized to a 600 px baseline JPEG, ~100 KB) into the album's
    songs as a correction. It goes into card copies at the next write (the Echo Mini shows
    embedded covers), and into the music files with *write corrected tags*. Songs inside an
    album file get it on the card only.
  - A 1416 px scan became a 600×592 JPEG of 106 KB in a real FLAC.
- 36 tests (30 offline); new opt-in check `embeds_cover`.

### 17. Copies of the same or higher quality (2026-10-28)
- **Quality.** The scan now reads each file's bitrate, sample rate and bit depth. This is a
  database schema bump: the first scan after updating re-reads every file once.
- **Edition rows** show their format ("FLAC 24/96", "MP3 320"). Among equally complete
  lossless copies, the higher resolution goes on the card by default.
- **Covered copies.** When another edition has all of a copy's songs, at least as much music,
  and the same or higher quality, the copy says so: "Edition 1 has all these songs in the same
  quality (MP3 320); this copy could go (130 MB)". The header counts such copies and their size,
  and links to a compact view listing just those albums.
  - Nothing is deleted; other cases are left for the choice at write time.
  - On the test library: 11 copies, 1.9 GB.
- `--plan` also lists every album copy with its format and status.
- Debug demo: `NEATLIBRARY_SHOT_DUPLICATES=1` opens that view.
- 37 tests (31 offline).
