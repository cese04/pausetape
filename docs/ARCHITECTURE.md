# Architecture

How NeatLibrary's code fits together. For status and plans see [PROGRESS](PROGRESS.md); for
building and releases see [DISTRIBUTION](DISTRIBUTION.md) and [TESTING](TESTING.md).

## Shape of the app

One Rust binary using **egui/eframe** for the interface (immediate mode: the whole UI is
redrawn from state every frame). Slow work runs on background threads that report back through
channels; the UI thread drains them at the start of each frame (`App::poll`, in `app/events.rs`).

```
            ┌──────────── UI thread (egui) ─────────────┐
            │  app (state)  ──►  ui (tabs, player)       │
            │     ▲  poll() drains messages each frame   │
            └─────┼──────────────────────────────────────┘
   scan thread ───┤  library::scan  + db (SQLite cache)
   MusicBrainz ───┤  one worker, ≤ 1 request/second
   writes ────────┤  card::write / card::execute
   audio thread ──┘  rodio player and the play queue; status shared via Arc<Mutex<Status>>
   helpers: waveform + cover loading per track; ffmpeg conversions (serialised)
```

## Data flow

1. **Scan** (`library/scan.rs`, `library/db.rs`): walk the music folder; for each audio file reuse the
   database row if size and modification time match, else read tags with `lofty` (and, for
   MP4 files, the codec code). Then, every time: `.cue` sheets turn album files into one track
   per song (`library/cue.rs`), and folder-name inference ("Artist/Album/03 - Title") fills
   gaps (it depends on which folder is the root). Saved corrections are re-applied.
   Result: `Vec<Track>`.
2. **Group** (`library/names.rs`, `library/grouping.rs`): artist spellings collapse to a *key* ("The Beatles",
   "Beatles, The" → `beatles`); keys merge by user decisions; near-misses become suggestions.
   An album (same name, same folder) whose songs name different artists stays under one: its
   album artist, an artist with ¾ of the songs, or Various Artists (`keep_albums_together`).
   Albums with the same name are split by folder into editions (copies sharing their songs)
   or separate same-name albums. Songs an album holds twice (same number, size and length)
   are its `repeats`; the plan leaves them out.
   Result: `Index` = clusters, suggestions, and `artists → albums → track indices`.
3. **Plan** (`card/plan.rs`): `Options` (album order, what to do with formats RECHO can't play,
   …) + `Custom` (what the user chose: whole artists and single albums, `Chosen`; editions,
   names, order) → list of
   `PlanItem { track, rel path, skip reason, target }`, FAT-safe and collision-free
   (case-insensitive). `target` says how the song becomes a card file: copied, or made by
   ffmpeg as FLAC or MP3 320 (`card::target`).
4. **Write** (`card/write.rs`, using `card/copy.rs` and `card/playlist.rs`): prune songs the
   *previous* write created that the plan no longer wants (only if their size still matches
   the manifest; first, so the card has room), copy what differs (songs inside an album file
   are cut out, and formats RECHO can't play converted, with ffmpeg), export playlists, prune
   what's left over (old playlist files, songs that failed), clean macOS `._` files, and store
   the new manifest in the write profile.
5. **Play** (`audio/`, `library/cover.rs`): `audio::open` tries Symphonia, else converts with
   ffmpeg to a cached stereo WAV; `audio::Section` keeps to one song of an album file. The
   waveform (loudness every 100 ms) and the cover load in background threads.

## Modules

Grouped by what the code is about. Each folder's `mod.rs` re-exports what the rest of the app
uses, so call sites read `crate::card::build_plan`, `crate::library::Track`, `crate::audio::Audio`.

```
src/
├── main.rs            start-up, --version, --plan; APP_NAME / APP_ID
├── platform.rs        app data folder; open a folder in Finder/Explorer/file manager
├── musicbrainz.rs     rate-limited API worker; lookups → proposed field changes
├── library/           reading and understanding the music folder
│   ├── track.rs       Track; which formats play on RECHO / here / not at all
│   ├── scan.rs        incremental scan (database reuse), tag reading, folder-name inference
│   ├── mp4.rs         the codec inside .m4a/.mp4 files (AAC, ALAC, Dolby E-AC-3, …)
│   ├── cue.rs         CUE sheets: parse, decode legacy encodings, album file → songs
│   ├── tags.rs        writing corrected tags into files
│   ├── db.rs          SQLite library database: cached tags, unsaved corrections
│   ├── cover.rs       embedded or folder cover art (as plain RGBA pixels)
│   ├── names.rs       artist/title keys, fuzzy matching, file/folder name parsing, FAT-safe names
│   └── grouping.rs    clusters, merge suggestions, artist → album → track index
├── card/              putting music on the SD card (and tidying the music folder)
│   ├── plan.rs        Options + Custom → planned paths (what's chosen, album order, names)
│   ├── copy.rs        copy / move, prune a previous write, clean macOS ._ files
│   ├── journal.rs     undo journal for reorganising in place
│   ├── adopt.rs       a card's existing music: survey, take over (move into place, undo), copy into the library
│   ├── write.rs       saved writes (WriteProfile), compare-with-card, write
│   ├── volumes.rs     mounted drives, free space, filesystem type
│   └── playlist.rs    playlists, planned copies, export with .m3u8
├── audio/             playing music
│   ├── player.rs      playback thread (rodio); owns the queue, so songs advance without the UI
│   └── decode.rs      Symphonia or ffmpeg (conversion cache), loudness envelope
├── app/               state and the glue between background work and the UI
│   ├── mod.rs         App, Saved (persisted), Tab, messages, eframe::App
│   ├── music.rs       scanning, tag corrections, artist decisions, MusicBrainz
│   ├── writes.rs      card writes, reorganising, playlist edits
│   ├── playback.rs    play queue, waveform and cover loading
│   └── events.rs      per-frame message pump (poll)
└── ui/                everything drawn on screen
    ├── mod.rs         panels, header, tab dispatch
    ├── theme.rs       palette, fonts, frames
    ├── widgets.rs     links, buttons, tabs, transport buttons, fitted labels
    ├── text.rs        durations, sizes, plurals, shortening, format notes and badges
    ├── actions.rs     clicks collected while drawing, applied afterwards
    ├── library.rs  artists.rs  playlists.rs  settings.rs  folders.rs  card.rs   one per tab
    ├── player.rs      player bar
    ├── navigator.rs   in-app folder browser for the pickers
    └── screenshots.rs debug-only screenshot demo (NEATLIBRARY_SHOT)
```

### Layering

- `library/`, `card/`, `audio/`, `musicbrainz.rs` and `platform.rs` know nothing about the UI.
  None of them uses egui (cover art is handed over as plain pixels). They can be tested, and
  later reused, without a window.
- `app/` owns the state and background threads; it's the only place UI and logic meet.
- `ui/` only draws and reports clicks; it never touches files or the network directly.
- `impl App` is split across `app/*.rs` by topic. The submodules `use super::*` to share the
  state types, and methods called across them are `pub(super)` or `pub`.

## Identities that must stay stable

| Thing | Identified by | Why it matters |
|---|---|---|
| Artist | cluster *key* (from the most common spelling) | Write exclusions and card names survive renames |
| Album | `"<cluster key>|<title key>"` (`AlbumNode::id`), plus `|<year>` for same-name albums and `#<folder key>` per edition | Same, for albums; `base_id` (without the edition) keys a write's choice of edition |
| Write | its name (kept unique) | Background write/preview results find their write by name |
| Track | `Track::id()`: its path, plus `#03` for a song inside an album file | Playlists, corrections and the waveform find their track |
| Playlist | its name (in writes), track **ids** (inside) | Renaming a playlist updates writes; moving files outside the app drops them |
| Track in the database | absolute path + size + mtime (album files cached whole; songs derived each scan) | Any change means "read again" |
| Song made by ffmpeg for the card (cut or converted) | manifest `source`: id, target format, CUE times, source size/date, tags, added cover | A rewrite makes it again only when one of them changed |

## Where things are stored

| What | Where | Notes |
|---|---|---|
| Settings, writes (with manifests), playlists, artist decisions | `app.ron` in the app data folder | eframe persistence; serde defaults make new fields safe |
| Library database | `library.sqlite3` in the app data folder | `SCHEMA_VERSION` bump clears the cache (it's re-derivable) |
| Undo journals for *Reorganise* | `journals/` in the app data folder | |
| Undo records for taking over a card | `journals/card/` in the app data folder | One per take-over, named by write |
| ffmpeg conversions | `<temp>/neatlibrary-converted/` | Last 6 kept; cleared at start-up |

App data folder: macOS `~/Library/Application Support/NeatLibrary`, Windows
`%APPDATA%\NeatLibrary\data`, Linux `~/.local/share/neatlibrary`. `APP_ID` names it, so keep
`APP_ID` when renaming the app (or migrate the folder).

## Safety rules the code keeps

- The **music folder is read-only** unless the user runs *Reorganise* (with preview and undo)
  or *write corrected tags to source files*.
- A **card write never deletes a file it didn't create**, and not even those if the size
  changed since. *Take over the card* only moves files on the card (with an undo record), on
  the user's request.
- Copies are **plain byte copies** (`card::plain_copy`): `std::fs::copy` on macOS would
  copy extended attributes, which appear as `._` files on FAT cards.
- Copies go to a `.partial` file first and are renamed when complete.
- Album files with a CUE sheet are **never moved or retagged**: *Reorganise* skips them, and
  corrections to their songs stay in the app (and in card copies).

## Platform-specific code

`card/volumes.rs` (free space: `statvfs` on Unix, `statfs` filesystem name on macOS, none on
Windows yet; mount points), `platform.rs` (data folder; `open`/`explorer`/`xdg-open`),
`ui/theme.rs` (font paths), `audio/decode.rs` (ffmpeg locations, `ffmpeg.exe`),
`ui/navigator.rs` (drive shortcuts). The gaps are listed in
[TESTING §3](TESTING.md#3-known-gaps-found-by-reading-the-code-not-yet-seen-on-a-real-machine).

## Tests

`cargo test` runs 35 tests, each next to the code it checks: name rules, folder-name reading,
MusicBrainz release checks, CUE sheets (parsing, encodings, album file → songs), clustering,
editions and same-name albums (which edition a write uses, which copies another covers), which formats play where, layout plans, copy/move/undo,
custom layouts and pruning, card write → deselect → rewrite, database reuse and corrections,
playlist export, cover lookup, text shortening. They use temporary folders and fake audio files,
so they need no music and no network. Ignored tests (`cargo test -- --ignored <name>`): `musicbrainz`
(live API), `scan_speed` (`NEATLIBRARY_BENCH=<dir>`), `decodes_formats`
(`NEATLIBRARY_FORMATS=<dir>`), `section_matches_cut` (`NEATLIBRARY_SECTION=…`, see the test),
`cue_album_write` (`NEATLIBRARY_CUE_ALBUM=<folder>`), `embeds_cover`
(`NEATLIBRARY_EMBED=<song>:<image>`), `convert_write` (`NEATLIBRARY_CONVERT=<folder>`), `queue_advances_without_ui`
(`NEATLIBRARY_PLAY=<song>`), `survey_a_real_card` (`NEATLIBRARY_SURVEY=<card>:<music folder>`).

## Conventions

- Comments explain *why*, not what.
- Long lines (~160 characters); see PROGRESS §6 about `rustfmt.toml` before running `cargo fmt`.
- UI copy uses plain words ("Write to the card", "songs") and avoids file-system jargon where
  possible.
