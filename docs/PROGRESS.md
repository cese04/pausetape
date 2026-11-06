# Pausetape: progress and roadmap

*Last updated: 2026-10-30: the roadmap's steps 1–5 and 7 are done, 6 and 8 in a first form (see §2 and the [CHANGELOG](../CHANGELOG.md), sections 23–33). Section 1 below still describes 2026-10-28 in places; the CHANGELOG is the full record.*

Pausetape organises a music collection for the Snowsky Echo Mini running the
[RECHO](https://recho.atzkey.workers.dev/) firmware. RECHO reads the SD card without writing to
it, browses it folder by folder sorted by name, has no playlists, and plays only FLAC, MP3 and
WAV. Everything here is shaped by those limits.

Related documents: [README](../README.md) (install and run) · [CHANGELOG](../CHANGELOG.md)
(everything done so far, in order) · [ARCHITECTURE](ARCHITECTURE.md) (how the code fits
together) · [TESTING](TESTING.md) (Windows and Linux test guide) ·
[DISTRIBUTION](DISTRIBUTION.md) (packaging and releases) · [NEXT](NEXT.md) (agreed next
changes, designed in detail).

---

## 1. Where things stand

Rust + egui desktop app, ~6,300 lines in 41 files grouped into `library/`, `card/`, `audio/`,
`app/` and `ui/` ([ARCHITECTURE](ARCHITECTURE.md#modules)), MIT licensed. 16 automated tests pass
on macOS, and 3 more run on request (network, benchmark, format check). Release build has no warnings.
Developed and run only on macOS so far; Windows and Linux are next (see [TESTING](TESTING.md)).

### Built

| Area | What it does |
|---|---|
| **Folders tab** | Two numbered steps. *Your music*: any folder, organised or not, only ever read. *Your SD card*: picked from mounted drives, with free space, filesystem, and a warning for non-FAT drives. Choosing happens in dialogs with an in-app folder browser. "Reorganise the music folder itself" is optional, under *Advanced*, with preview and undo. |
| **Library** | Artist → album → track. Letter headings and an A–Z rail (shows only used letters when the window is short). Each album shows the folder(s) it lives in, with *open folder*. Filter box. Click a track to play. |
| **Library database** | SQLite `library.sqlite3` in the app's data folder (see [ARCHITECTURE](ARCHITECTURE.md#where-things-are-stored)). Rescans only open files whose size or modification date changed: 2,040 files in 25 ms vs 280 ms. Unsaved tag corrections survive restarts. |
| **Artist matching** | Spelling variants merge automatically ("The Beatles"/"Beatles, The", "AC/DC"/"ACDC", "Beyoncé"/"Beyonce", "feat." stripped). Near misses ("Radiohaed") are suggested for you to confirm or reject; decisions are remembered. Compilations without album-artist tags are detected. |
| **MusicBrainz** | *Look up* on an album proposes tag fixes as an opt-in list; identical changes across tracks are grouped. Artist search includes aliases ("Guns and Roses" → "Guns N' Roses"); picking a MusicBrainz name merges clusters that already use it. Limited to one request per second, as MusicBrainz asks. |
| **Card tab: saved writes** | A *write* is a named layout for a card: which artists, albums and playlists go on it; folder names just for that card; album order (`1997 - Album`, `Album`, or your own `01 - Album`). *Compare with the card* shows what will be copied and removed; *Write to the card* copies only what changed. Deselected music is removed, but only files that write put there and whose size hasn't changed. |
| **Layout on the card** | `Artist/1997 - Album/01 - Title.flac`; multi-disc `1-01`; compilations name the track artist. Names made safe for FAT (`AC/DC` → `AC-DC`), case-insensitive collisions avoided, plain byte copies so macOS leaves no `._` files, optional cleanup of existing `._` and `.DS_Store` files. |
| **Playlists** | Built in the app; written to the card as folders of numbered **copies** (`_Playlists/Road Trip/01 - Artist - Title.flac`) plus an `.m3u8` for future firmware. Symlinks aren't possible on FAT/exFAT (see §4). |
| **Formats** | Plays FLAC, MP3, WAV, AAC, ALAC, AIFF, Ogg, MP2, MKA, CAF directly. Through ffmpeg (bundled in the Mac app; installed separately elsewhere): Dolby E-AC-3 and AC-3 (e.g. Atmos releases), TrueHD, DTS, Opus, APE, WavPack, WMA, converted once to a temporary stereo WAV. **Not Dolby AC-4**: FFmpeg has no decoder for it. The real codec inside M4A files is detected. Anything RECHO can't play gets a badge ("Dolby E-AC-3 · not on RECHO"), is counted in the card tab, and is converted for the card unless you choose otherwise there. |
| **Player** | Cover art (embedded, else `cover.jpg`/`folder.jpg`…), a loudness waveform that doubles as the seek bar with a red playhead, round previous/play/next buttons, volume. Keys: space, ←/→ (±10 s), N/P. |
| **Mac app** | `scripts/package-macos.sh` makes a universal `.app` (47 MB) and `.dmg` (24 MB) for Apple Silicon + Intel, macOS 12+, with a minimal LGPL ffmpeg (8.3 MB) inside, icon, licences, and an ad-hoc signature (no Apple Developer Program yet). `--version` reports which ffmpeg is in use. See [DISTRIBUTION](DISTRIBUTION.md). |
| **Automated builds** | GitHub Actions: `ci.yml` builds and tests on macOS, Windows and Linux on every push; `macos.yml` packages the DMG and attaches it to releases for `v*` tags. Written, not yet run (no GitHub repository yet). |
| **Look** | Tufte-inspired: one serif face (Palatino), warm paper or ink theme, no boxes except where a task needs grouping, colour only for warnings (always brick red) and an accent of the user's choosing (seven presets) for what's open, playing or under way. Long names shorten with "…" to fit; the layout holds from 720 px to wide screens. |

### How it was checked

| Checked | How |
|---|---|
| Name normalisation, clustering, FAT names, text shortening | Unit tests |
| Copy (incremental), move + undo, custom layout + pruning | Tests on temporary folders |
| Card write → deselect → rewrite cleans up, never touches other files | Test (`card/write.rs`) |
| Database: reuse, changed/deleted files, corrections kept/dropped | Test (`library/db.rs`) |
| Playlist export numbering and pruning | Test (`card/playlist.rs`) |
| Cover lookup beside the track | Test (`library/cover.rs`) |
| Which formats play on RECHO / here / nowhere (AAC, E-AC-3, AC-4) | Test (`library/track.rs`) |
| MusicBrainz album + artist lookups | Live test, run manually (`--ignored musicbrainz`) |
| Decoding of each format; your *Love (Dolby Atmos)* tracks | Format test on real files (`--ignored decodes_formats`) |
| Every tab at 720 px and 1,040–1,600 px | Screenshots via the debug harness (§6) |
| Packaged Mac app: bundle layout, signature, DMG contents, `--version` finds the bundled ffmpeg, app starts | Run on this Mac after `scripts/package-macos.sh` |
| Bundled ffmpeg converts a *Love (Dolby Atmos)* track | Same 31.6 MB stereo output as Homebrew's ffmpeg, in 1 s |

### Not verified yet

- **A real write to the Echo Mini's card.** The write logic is tested on temporary folders, and
  the *Write to the card* button runs that same code, but nobody has pressed it against a FAT/exFAT
  card and then browsed the result on the player.
- **Writing tags into real audio files** (*write corrected tags*, *write … to source files*).
  The code uses `lofty`, but no test writes to a real FLAC/MP3 and reads it back.
- **Click-through of the dialogs.** The screenshots open them programmatically.
- **Windows: first run on Windows 11 (2026-11-01).** It built and passed its tests unchanged;
  drives, free space, a FAT32 write and rewrite, unticking an album, ffmpeg conversion and the
  Trash on cards worked. Text sat high and a console opened behind the app; both are fixed.
  Still to see in the app: a card bringing up its setup, Stop during a write, taking over a
  card, and scaled displays. [TESTING](TESTING.md) §3 has the details.
- **Linux: never built or run there.** The code is written to be portable, but an audit found
  gaps; [TESTING](TESTING.md) lists them with workarounds and is the step-by-step guide.
- **The Mac app on Intel** (no Rosetta on the build Mac), the **first-launch approval** of a
  downloaded copy, and both **GitHub Actions workflows** (no GitHub repository yet).

### Known issues and limits

- **MusicBrainz lookups** were fixed on 2026-10-28 so they no longer suggest another album's
  titles ([NEXT §A](NEXT.md#a-musicbrainz-lookups-pick-the-wrong-release)). Still to do: choosing
  among alternative releases, and marking songs matched only by position.
- **Editions** of one album are kept apart, and one goes on the card; same-name albums
  (American Football ×3) stay separate ([NEXT §B5](NEXT.md#b-folder-and-file-names-are-misread)).
  Copies that another copy covers in the same or higher quality are pointed out; removing them
  is up to you (the app never deletes music).
  Not yet caught: editions tagged with different album names (*Substance* vs *Substance
  (1977-1980)*).
- Untagged files still take the artist from the folder only, not from tagged neighbours
  ([NEXT](NEXT.md) B4).
- **Albums stored as one file plus a `.cue` sheet** now work in the library, the player and on
  the card ([NEXT §C](NEXT.md#c-albums-stored-as-one-file-plus-a-cue-sheet)). Limits:
  - no gapless join between songs of one file;
  - corrections to those songs can't be written back into the album file (they reach the card);
  - cutting needs ffmpeg, which only the Mac app bundles.
- The debug screenshot demo has twice produced no screenshots (empty log, no panic); every
  rerun worked. It only affects that debug tool, but is noted in case the app is ever seen to
  hang on start-up.
- RECHO remembers where it was by file path; after a write or a reorganise its *resume* may
  point at a moved file (the app warns).
- Writes are matched to background results by name; names are kept unique, but renaming a
  write while it's being written would orphan the result.
- Playlists list tracks by path. Paths are remapped after *Reorganise*, but files moved outside
  the app drop out of playlists (shown in red).
- Library-wide notices show only the latest message; earlier ones are lost.

---

## 2. Roadmap: Calibre for music

*Agreed 2026-10-28.* The direction: like Calibre for books, the library is the source of truth,
the card is a device view of it, metadata and covers are first-class and fixable in bulk, and
sending to the device converts as needed. Steps in order of importance. Effort: **S** = an hour
or two, **M** = about a day, **L** = several days.

| Calibre | Pausetape | Status |
|---|---|---|
| Convert on send | FLAC / MP3 320, CUE albums cut into songs | Done |
| Download metadata | MusicBrainz suggestions, checked against your songs | Done |
| Duplicate finder | Editions; copies covered by a same-or-better copy and songs stored twice, both of which can go to the Trash | Done |
| Device view | Card writes with a manifest; taking over a card's existing music (a guided dialog); send to / take off the card from the library with one Sync; on-card and pending marks; `card:` searches; space savers (CD quality, MP3 320) and what to leave out | Step 1: done |
| Edit metadata, bulk edit | Tag editor for a song, an album or any selection | Step 2: done |
| Download covers | Images beside the files, else the Cover Art Archive, for albums without a cover | Step 3: done (no choosing between several found covers yet) |
| Sortable, searchable library | Album table (sort, select, bulk actions) beside the artist list; `field:value` search | Step 4: done |
| Jobs panel | The running job with time left and Stop | Step 5: done (no history of finished jobs) |
| Add books | Copies new music from the card or any folder into the music folder (`From the card`, `Added`) | Step 6: first form (copies as they are; no sorting into the organised layout) |
| Virtual libraries | Saved searches; select all matches and send them to the card | Step 8: first form (not yet a live card selection) |

| Step | What | Why here | Effort |
|---|---|---|---|
| 1 | **The card as a device view.** Take over music already on a card (match exact copies of library songs, move them into the organised layout instead of copying again, never delete what doesn't match; undo). Then "on card" marks, the card's free space at hand, send/remove a selection. | The test card already holds 67 GB copied by hand: a first write would duplicate it and not fit. Every later device feature builds on knowing what's on the card. | M–L |
| 2 | **Edit metadata, also in bulk:** click a field to fix it; select songs or albums and set album artist, year, genre… for all. Same correction model as MusicBrainz suggestions (in-app, card copies, files on request). | Turns an organiser into a library manager; small, and reuses what exists. | M |
| 3 | **Covers:** fetch from the Cover Art Archive for albums with none; choose between found, embedded and downloaded; "add covers to all albums missing one". | The Echo Mini shows embedded art; missing covers are visible on the device. | S–M |
| 4 | **Browsable library:** a sortable album table (artist, album, year, format/quality, songs, size, on card, cover, edited), search fields (`format:m4a`, `quality:lossy`, `on:card`, `cover:no`, `year:1979..1985`), multiple selection for bulk actions. The artist list stays as a second view. | The Calibre feel itself; makes steps 2, 3 and 7 work on many albums at once. Largest piece, so done once 1–3 show which columns and actions matter. | M–L |
| 5 | **Jobs panel:** running and finished jobs (writes, conversions, lookups, scans) with progress, errors and what changed. | Work now runs for minutes; one busy line hides it. | S |
| 6 | **Add new music:** pick a folder (e.g. Downloads), see each album as new / edition / better copy / duplicate, copy or move it into the music folder in the organised layout. | Calibre's "Add books"; keeps the library growing without manual sorting. | M |
| 7 | **Duplicates, actions:** prefer this copy, these are different albums, move covered copies to the Trash with undo. | Frees disk space; search (step 4) makes it quick. | M |
| 8 | **Saved selections** (virtual libraries): saved searches usable as card selections and smart playlists ("lossless jazz", "gym"). | Builds on step 4. | M |
| 9 | **Windows and Linux builds:** drive detection, bundled ffmpeg, installers ([DISTRIBUTION](DISTRIBUTION.md)). | Distribution. | M–L |

**Route to the first real write on the test card** (2026-10-28). The card holds 67 GB copied by
hand: 936 songs are exact copies of what a write needs, 1,840 songs (29.8 GB) aren't in the
library, and 2.6 GB is iTunes and system clutter.

1. *Check them against the library* → **copy them into your music folder** (the 1,840 songs
   join the library under `From the card`; the card is unchanged).
2. Rescan, then decide what goes on the card. The library is then about 160 GB, so it won't
   all fit: tick **CD quality** (saves about 28 GB) and leave out some artists or albums.
3. *Check them against the library* again → **Take over the card** with *move everything else*
   ticked and *delete clutter* ticked. Undo is available.
4. **Write to the card**, then test on the Echo Mini: folder sorting, songs cut from CUE
   albums, converted MP3s, covers, names on its screen.
5. Delete `_Before Pausetape` from the card once happy.

Where it stands (2026-10-29): steps 1–2 done. The first take-over moved 659 files and stopped
on locked files (see CHANGELOG §22); the take-over now unlocks them, so check the card again and
take it over once more.

**Card UX, from the plan of 2026-10-29.** Done: (1) each album and artist row says where it
stands with the card and has one button; a better copy can replace the one on the card;
(2) clicks wait for one **Sync**, with a strip above the library and a pinned header in the
card tab; (3) the card is what you chose, not everything minus exclusions. Still to do:
(4) fold the card tab's settings (formats, CD quality, album order, tags, singles folder,
playlists) away by default; (5) the take-over as a one-time guided dialog behind a banner;
(6) a "Card" filter in the library (on card, not on card, better copy available), with step 4's
table.

Folded in along the way: choosing among alternative MusicBrainz releases, the artist from tagged
neighbours ([NEXT](NEXT.md) B4), shorter folder names if the device test shows cut-off names,
gapless playback between songs of one album file.

---

## 3. Other pending suggestions (offered in conversation)

Smaller items, ordered by value.

| # | Suggestion | Why | Effort |
|---|---|---|---|
| 1 | ~~**Convert while writing to the card.**~~ **Done 2026-10-28** (CHANGELOG §18): lossless → FLAC, lossy → MP3 320, the default for new and existing writes. | | |
| 2 | **Shorter folder names on the card**, as a setting per write (e.g. 40 characters; today 80 artist / 100 album / 120 file). | The Echo Mini's screen shows far fewer characters; long names are cut off on the device. | S |
| 3 | **Optional max width on very wide screens**, keeping the rail beside the list and centring the block. | Titles and durations drift far apart on large monitors. | S |
| 4 | **AcoustID fingerprinting** (needs `fpcalc` and a free API key) to identify files with no usable tags. | MusicBrainz lookup needs at least a rough artist/title. | M |
| 5 | **Turn off Spotlight on the card**: offer `mdutil -i off` for the selected card. | Stops macOS adding hidden `.Spotlight-V100`/`.fseventsd` folders. | S |

---

## 4. Further UI/UX ideas

### Card and device

- **Preview as the player sees it.** A small panel drawing the card's folders as the Echo
  Mini lists them: name order, cut to the screen's width. Catches bad sort order and unreadable
  names before writing. *M*
- **Progress with time left, and Cancel**, during long writes (a 30 GB first write takes a
  while). *S*
- **Eject the card** after a successful write (`diskutil eject`). *S*
- **"On the card" marks in the library**: a quiet dot on albums the active write has put on
  the card, so the library and the card tab tell the same story. *S*
- **Prefer the best copy.** When the same album exists twice (FLAC and MP3, or Atmos and
  stereo), choose which one goes to the card, defaulting to the one RECHO plays at the highest
  quality. *M*
- **Space-saving option.** Optionally downsample 24-bit/192 kHz FLAC to 16/44.1 when copying;
  RECHO plays hi-res, but a smaller card fits much more. *M* (needs #1's ffmpeg pipeline)

### Library

- ~~**Keyboard navigation.**~~ *Done 2026-11-05:* ↑/↓ through songs or albums, Enter to play,
  ⌘F to the search box, ⌘A, ⌘I, ⌘D, ⌘⇧S, ⌘Z; listed in Settings › Keyboard.
- **Cover thumbnails** next to album titles; optionally an album grid view. *M*
- **Remember expanded artists and scroll position** between launches. *S*
- **Look up every album** in one go, as a queue with progress, then review the suggestions
  album by album. *M*
- **Duplicate-track detection** (same recording in two places or formats). *M* Editions now
  catch whole duplicate albums, and exact repeats inside an album are found and can go to the
  Trash; the same song in two albums or formats is still unhandled.
- **Duplicate copies, next steps** (discussed 2026-10-28; for now copies are only pointed out):
  - a "prefer this copy" choice for all writes, folding the others in the library;
  - "these are different albums" to unlink a remaster or deluxe edition;
  - a keyboard review of all duplicates;
  - moving covered copies to the Trash or a `_Duplicates` folder, with preview and undo;
  - a quality-or-space preference per write;
  - flagging "FLAC made from MP3" from auCDtect logs. *M each*
- **Fetch missing covers** from the Cover Art Archive (MusicBrainz) for albums with no image at
  all, then offer to add them to the songs as for covers found beside the files. *M*

### Player

- **Up-next queue** with shuffle and repeat, and *play next* / *add to queue* on any track. *M*
- **Gapless playback** by queueing the next track in the audio engine ahead of time; matters
  for live albums and *Love* itself. *M*
- **macOS media keys and Now Playing** (via the `souvlaki` crate). *M*
- **Loudness levelling** (ReplayGain tags, or computed from the waveform the app already
  makes). *M*

### Artists and tags

- ~~**Undo per decision.**~~ *Done 2026-10-31:* each one has its undo in Settings › Artist
  decisions.
- **Edit a tag by hand** (click a title or year to correct it) in addition to MusicBrainz
  suggestions. *M*

### General

- **Message history.** Make the one-line notice expandable into a short log (writes, scans,
  errors). *S*
- **Text size setting** and a contrast check of the muted grey, especially in the paper theme,
  which has been reviewed less than ink. *S*
- **First-run guidance.** The folders tab already opens first; a one-line "Next: …" hint on
  each tab until the first write would finish the onboarding. *S*
- **Installable apps for macOS, Windows and Linux**: planned in
  [DISTRIBUTION.md](DISTRIBUTION.md) (packaging, signing, automated builds, and the
  portability fixes needed first).

---

## 5. Decisions and why

| Decision | Reason |
|---|---|
| **egui rather than Tauri** | One language and one process, so playback and the UI share state directly; a single small binary; immediate mode suits a dense, table-like library. Typography comes from loading system serif fonts. |
| **Playlists as copies, not symlinks** | FAT32/exFAT have no symbolic or hard links. macOS fakes symlinks there as small "XSym" text files the player would show as broken entries. A FAT "cross-linked file" hack is technically possible (RECHO never writes to the card) but any disk check on the computer would "repair" it, so it was rejected. |
| **Copy by default, reorganise in place only on request** | The music folder is the user's own; it may be organised for other players or tools. |
| **Card writes delete only their own files** | The manifest records what each write created and at what size; anything else on the card is never touched. |
| **Tags cached raw, folder guesses recomputed** | Guesses from folder names depend on which folder is the music root, so caching them would go stale. |
| **ffmpeg as a separate program, bundled on macOS** | Most libraries are FLAC/MP3/AAC, which play without it. For the rest (Dolby, Opus, APE…) the Mac app ships a minimal LGPL build made from source (8.3 MB, not a 70–100 MB GPL download). It runs as its own program, so the app stays MIT. On Windows/Linux it is still a separate install, for now. |
| **MIT licence** | Simple and compatible with every dependency (Symphonia is MPL-2.0; bundled ffmpeg LGPL as a separate program). Also qualifies for free open-source code signing on Windows (SignPath). |
| **Mac first, unsigned for now** | No Apple Developer Program yet: builds are ad-hoc signed and users approve the app once in System Settings. |

---

## 6. Technical housekeeping

- **Cross-platform release**: see [DISTRIBUTION](DISTRIBUTION.md). Its §1 lists ten code
  changes needed before Windows/Linux builds behave (drive detection on Windows is the big one).
- **Version control.** A git repository exists (`target/` and `dist/` ignored) with nothing
  committed yet; the first commit and a GitHub remote are the next steps. Once pushed, the CI
  workflow gives the first Windows/Linux build results.
- **Formatting.** The code uses lines up to ~160 characters; plain `cargo fmt` would rewrap every
  file. Adding a `rustfmt.toml` (`max_width = 160`) first keeps diffs readable.
- **Continuous integration**: `ci.yml` exists; watch its first runs on Windows and Linux.
- **A tag-writing test** on a real FLAC and MP3 (see *Not verified yet*).
- **Upgrade path for saved settings.** Saved state is versioned implicitly through serde
  defaults; a breaking change to `Saved` would need an explicit version.

---

## 7. Handy commands

```sh
cargo run --release                         # the app
cargo run --release -- --plan ~/Music       # dry run: print the planned layout
cargo test                                  # offline tests
cargo test -- --ignored musicbrainz         # live MusicBrainz test
PAUSETAPE_BENCH=~/Music      cargo test --release -- --ignored scan_speed --nocapture
PAUSETAPE_FORMATS=/some/dir  cargo test --release -- --ignored decodes_formats --nocapture
```

Debug builds include a screenshot harness that opens every tab and dialog, saves each as a PPM,
and quits:

```sh
PAUSETAPE_SHOT=/tmp/shots \
PAUSETAPE_SHOT_SIZE=720x600 \
PAUSETAPE_SHOT_FILTER=beatles \
cargo run
```

`PAUSETAPE_FFMPEG=/path/to/ffmpeg` points the app at a specific ffmpeg.
`pausetape --version` prints the version and which ffmpeg is in use.

Mac packaging: `scripts/package-macos.sh` (uses `scripts/build-ffmpeg-macos.sh` and the icon
from `scripts/make-icon.py`). Details in [DISTRIBUTION](DISTRIBUTION.md).
