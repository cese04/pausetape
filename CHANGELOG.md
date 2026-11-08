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
- MIT licence. Display name in one constant (`APP_NAME`).
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
- Debug demo: `PAUSETAPE_SHOT_ARTIST` chooses the album played and looked up.
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
- Debug demo: `PAUSETAPE_SHOT_DUPLICATES=1` opens that view.
- 37 tests (31 offline).

### 18. Converting for the card (2026-10-28)
- **What converts.** Songs the Echo Mini can't play are converted when writing to the card,
  now the default (*convert them*, next to *leave them off* and *copy them anyway*):
  - lossless (ALAC, AIFF, APE, WavPack…) → FLAC, nothing lost;
  - lossy (AAC, Dolby E-AC-3/AC-3, Opus, Vorbis, WMA…) → MP3 320 kbps, stereo. Lossy audio is
    never made into FLAC, which would only take more space.
- **What carries over.** Converted files get the song's tags and the cover embedded in the
  source. Like CUE cuts, the write records what each was made from, so later writes convert
  nothing that hasn't changed.
- **Songs inside lossy album files (MP3 + CUE)** can now go on the card too, as MP3 320.
- **Card tab:** says what converting makes ("111 songs to FLAC, 226 to MP3 320") and what is
  still left out and why. Album rows say "N converted". The comparison counts conversions.
- **Settings › ffmpeg** says whether this ffmpeg can make MP3s.
- **On the test library:** all 337 M4A songs now reach the card (none left out).
  - *Speak Now* (14 ALAC songs) → FLAC in 15 s.
  - *Morning Glory* (12 Dolby songs) → MP3 in 22 s.
  - Lengths match the sources within 0.1 s.
- **Bundled ffmpeg** gains LAME 3.100 for MP3 (LGPL, built from source, licence shipped):
  8.9 MB.
- **Existing writes** switch to *convert them*: the old leave/copy setting isn't carried over.
- 39 tests (32 offline); new opt-in check `convert_write`.

### 19. Fix: playback stopped after a song while the window was hidden (2026-10-28)
- **Cause.** The next song was started from the window's frame update. macOS stops drawing
  windows that are covered (for example while working in another app), so the queue stalled at
  the end of a song until the window was looked at again.
- **Fix.** The audio thread now owns the play queue and moves from song to song by itself. It
  steps over songs that can't play and reports them. The window follows its position (song,
  cover, waveform) whenever it's drawn.
- New opt-in check `queue_advances_without_ui` (plays two one-second parts of a song with no
  window).

### 20. Taking over a card that already holds music (2026-10-28)
- First step of the "Calibre for music" roadmap (PROGRESS §2): the card as a device view.
- **Detecting.** *Compare with the card* now counts files the write didn't put there.
  **Check them against the library** (reads only) sorts them into:
  - exact copies of songs the write needs (same size, same first and last 64 KB);
  - exact copies of library songs it leaves out or converts;
  - other versions of library songs (same artist and title from tags or file name, a
    different file);
  - music that isn't in the library;
  - other files.
- **Take over the card.**
  - Exact copies are moved into the write's layout (a rename on the card: instant) and join
    its manifest, so they aren't copied again.
  - Optionally, everything else goes into one folder, `_Before Pausetape`. Nothing is
    deleted.
  - Folders emptied by the moves are removed.
  - **undo taking over the card** puts every file back (a record per take-over in the app's
    data folder).
- **On the test card:**
  - 936 songs (23.9 GB) can be moved into place;
  - 535 files are other versions of library songs;
  - 1,840 songs (29.8 GB) aren't in the library;
  - the survey takes about 80 s.
- Debug demo: `PAUSETAPE_SHOT_SURVEY=1` runs the check. New tests: a take-over and its undo
  on a fake card; opt-in `survey_a_real_card` (reads only).
- **Delete clutter** (opt-in, unticked by default, can't be undone).
  - What counts as clutter: iTunes library data (`.itc2` artwork cache, `.itl`/`.itdb`
    libraries, `.ipa` apps, the `iTunes` folder's XML), Windows and macOS folder files
    (`desktop.ini`, `Thumbs.db`, also when a backup tool renamed them
    `desktop (2015_05_12 …).ini`), and rip logs and checksums.
  - Covers, scans, booklets, playlists, CUE sheets and text files are never deleted.
  - On the test card: 2,781 files, 2.6 GB.

### 21. Card space, "on card" marks, and saving the card's own music (2026-10-28)
- **CD quality for hi-res** (per write, off by default): 24-bit and over-48 kHz FLAC go on the
  card as 16-bit FLAC at 48 or 44.1 kHz (whichever family the source is), dithered. Your music
  folder keeps the originals.
  - Measured: 24/192 → 17% of the size, 24/96 → 32%.
  - On the test library, about 27.5 GB saved, which brings a full write from 129 GB to about
    102 GB.
  - *AM* (12 songs, 24/192) took 42 s, with the same lengths.
  - When a write doesn't fit, the card tab says what this would save and offers "do that".
- **Covers in converted files:** files made by ffmpeg take the source's embedded cover, else
  the cover image beside it (the Echo Mini shows embedded covers only).
- **"On card" marks** (Calibre's "on device"): album rows say "on card" or "8 of 9 on card",
  and songs on the card get a small dot. They come from the write's manifest, so they work
  without the card connected.
- **Copy the card's own music into the library:** after checking the card, "copy them into
  your music folder" copies the songs that aren't in the library (with their covers, CUE sheets
  and booklets) into `<music folder>/From the card/`, keeping the card's folder names; the card
  isn't changed. The library is rescanned. Checking the card again then finds those songs as
  copies a take-over can move into place.
- **Space estimate:** once the card is checked, the songs a take-over would move into place
  count as already there.
- Debug demo: `PAUSETAPE_SHOT_ON_CARD=<album text>` fakes "on card" marks in memory.

### 22. The first real take-over, and what it taught (2026-10-29)
- The first take-over of the test card took 3½ minutes but moved only 659 files. The other
  songs were **locked**: macOS shows a FAT/exFAT read-only attribute as the "locked" flag and
  then refuses to rename or delete the file. Only the first error showed, in a notice.
  - The take-over now unlocks a locked file and tries again (the Echo Mini doesn't care), and
    undo does the same.
  - It shows progress (files done / total).
  - Every problem is listed in the card tab and saved to `journals/problems/<time> <what>.txt`;
    the same goes for writes, copying from the card and undo.
  - A move that fails no longer leaves behind the folders made for it.
  - macOS keeps extended attributes on exFAT as `._` files beside each file and folder it
    touches (here about 3,000, some named like songs). With the write's "delete macOS ._ files"
    option (on by default), the take-over deletes them afterwards, along with empty folders.
- **Folders that aren't artists:** music copied from the card sits in `From the card/`, and
  untagged albums in it were filed under an artist called "From the card". Folders like that
  (Music, iTunes' Música, Downloads) are now skipped when looking for the artist. A folder like
  "Guns N' Roses-Appetite For Destruction" (no spaces round the dash) is split when the library
  knows that artist from other songs.
- 46 tests (36 offline).

### 23. The card as "what you chose", with one Sync (2026-10-29)
Calibre's device view, for music: the library is where you choose what goes on the card.
- **What you chose:** a write now holds a chosen set: artists taken whole (albums added to the
  library later go too) and single albums. An album left out of a whole artist stays out. New
  writes start empty. Writes saved before this become "every artist that wasn't left out,
  whole", which puts the same music on the card as before.
- **Library rows:** each album has *send to card* / *take off card*, each artist *send all to
  the card* / *take all off the card*. A mark beside albums and songs says where they stand:
  a dot when on the card, a ring when the next Sync adds them, a struck red ring when it takes
  them off ("swapped for the other copy" when another edition replaces it). When the album on
  the card has a copy in higher quality, *use the FLAC copy* (or whichever format) swaps it.
- **One Sync:** clicks only change the choice; the card changes when you press **Sync** (the
  write, which copies, converts, removes and retags only what changed). The pending changes come
  from comparing the choice with the write's manifest, so they're right with the card unplugged.
- **The strip:** above the library list, while the write has a card: a space bar, "156 GB
  chosen, room for 67 GB · 89 GB over", the albums Sync adds, swaps and takes off, and *sync
  now*.
- **Card tab:** its header is pinned: the space bar, "doesn't fit" with the CD-quality offer,
  what the next Sync changes, and **Compare** / **Sync**. Everything else scrolls under it. The
  list's ticks are the same choice as the library's buttons.
- Debug demo: `PAUSETAPE_SHOT_TOGGLE=<album text>` sends matching albums to the card or takes
  them off, in memory.
- 47 tests (37 offline).

### 24. One album, one folder; songs stored twice (2026-10-29)
- **Albums split over artists:** an album's songs went wherever each song's artist pointed, so
  *Duality* landed half under "Lisa Gerrard & Pieter Bourke" and half under "Lisa Gerrard" (two
  songs had no album-artist tag), *Suspended Animation* half under Various Artists (a
  compilation flag on some songs only), and about 25 more, mostly soundtracks. An album (same
  name, same folder) now stays together: under its album artist when the songs that have one
  agree on it, else under an artist with three quarters of the songs, else under Various
  Artists (split releases, soundtracks). Artists merged by hand count as one.
- **Songs stored twice:** Windows' backup copies ("Black Dog (2015_05_12 05_18_04 UTC).mp3"
  beside "… (2015_06_05 …).mp3") came along with the card's music: 478 songs (3.8 GB) were in
  their albums twice, and 346 were on the card twice, as "Song" and "Song (2)". A song with the
  same disc, number, size and length as another in its album is now an extra copy:
  - the card gets it once, and the next Sync takes the extra copies off;
  - the library marks it *stored twice*, notes it under the album with *move the extra copies to
    the Trash*, and the header's "478 songs stored twice" opens the duplicates view, which can
    move all of them at once. Each copy goes only if it is still the same file as the one that
    stays (size, first and last 64 KB); the one with the shorter name stays.
- The library's Sync strip and the card tab's pinned row wrap instead of pushing *sync now* and
  the buttons off the edge.
- **"Doesn't fit" growing during a write:** the room is what's free plus the write's own files,
  but the files a running write adds count as its own only once it finishes, so the room shrank
  as it went and an "over" amount appeared and grew. The room now holds at what it was when the
  write started.
- **Removing first:** a write removed deselected albums (and the songs on the card twice) only
  after copying, so a choice that fits only once they're gone could fill the card halfway and
  fail its last songs. Songs no longer wanted now go before anything is copied; playlist files
  still wait for the end. A copy that fails (a full card) no longer leaves its `.partial` behind.
- **Back from the duplicates view:** its only way out was a grey "show all" that read like a
  hint. It now sits in a panel with a *back to the whole library* button; Esc and the library
  tab leave it too, and it closes itself once no duplicates are left.
- 49 tests (40 offline).

### 25. Clearer screens: a tab bar, icons, settings in sections (2026-10-30)
- **Tab bar:** the tabs sit on a tinted bar ruled off from the page, each with an icon, the open
  one in a filled pill, so they no longer look like the grey links below them. In a narrow
  window only the open tab keeps its name. What's running (writing, scanning) and its progress
  sit at the bar's right end.
- **Messages** ("6544 songs · …", "moved to the Trash") get their own line under the bar, with
  a close button, instead of sharing the header with the library's counts.
- **Icons on commands:** drawn line icons (`ui/icons.rs`, like the player's buttons, so no font
  is needed) mark play, look up, send to / take off the card, rename, open folder, rescan,
  Trash, Compare, Sync, move up and down… Commands get a soft background on hover.
- **Library:** the counts and the duplicates links moved from the header into the library tab,
  next to the music folder; the duplicates are outlined chips beside the filter. The card
  strip is a framed row with the card's icon, its room, "N over" in red, the changes for the
  next sync on a second line, and a **Sync** button.
- **Card tab:** the room and Sync summary is a panel; the sections have headings with icons;
  the options are a list of settings, each named on the left with its controls and a smaller
  explanation on the right (album folders, other folders, formats RECHO can't play, hi-res
  songs, housekeeping).
- **Settings tab:** the same layout, in sections (appearance, library, playing and converting,
  MusicBrainz), with explanations in smaller type than the settings.

### 26. Faster writes: conversions in parallel (2026-10-30)
- A write made one song at a time, so converting (hi-res to CD quality, M4A to MP3, songs cut
  from album files) kept one core busy and the card idle while ffmpeg ran. Now a pool of
  workers (one per spare core, up to 8) makes those songs in a work folder on the computer,
  while one writer puts files on the card: each finished song as soon as it's ready, plain
  copies in between. At most a few finished songs wait in the work folder, so it can't fill the
  disk. 6 hi-res FLAC at CD quality and 6 M4A to MP3: 18.0 s before, 3.8 s now (12 cores).

### 27. Stop and time left (2026-10-30)
- **What's running** has its own line under the tab bar: what it is, how far ("1204 of 2466"),
  the time left at the pace so far once there are a few seconds of it to go on, a progress
  line, and a **Stop** button.
- **Stop** finishes the file it's on and stops there. A stopped card write records what it
  copied plus the earlier files still on the card, so the next Sync carries on where it
  stopped and can still remove what's its own; playlists and the final clean-up wait for that
  Sync. Reorganising the music folder, copying music from the card, moving extra copies to the
  Trash and writing corrected tags can stop too (their undo journal and rescans work as before).

### 28. Editing tags by hand (2026-10-30)
- **edit tags** on an album opens a dialog with the album, album artist, artist, year and disc
  (one box each for all its songs) and each song's number and title. Where the songs differ,
  the box starts empty ("several values"); what's typed goes to all of them, and an empty box
  changes nothing.
- **Choosing songs:** ⌘-click (Ctrl-click) adds a song to a selection, shift-click a run of
  songs in an album; selected rows are shaded, and a bar above the list offers **edit their
  tags**, **play them** and **clear** (or Esc). Right-click a song for *edit tags of this song*,
  *of the album* or *of the selected songs*.
- Saved edits are corrections like MusicBrainz's: marked *edited*, kept in the library database
  across restarts, in card copies at the next Sync, and in the music files with *write
  corrected tags*. The library regroups at once (a new album name or artist moves the songs).

### 29. Making the choice fit (2026-10-30)
- **Saving space** (card tab › Folders and options) has three choices: lossless songs as they
  are, hi-res at CD quality, or **all lossless songs as MP3 320** (about a quarter of the space of
  CD-quality FLAC). Lossy songs are never re-encoded; without ffmpeg (or its MP3 encoder) songs
  go as they otherwise would. Choosing again re-makes the card's copies at the next Sync.
- When the choice doesn't fit, the card tab says by how much and lists the ways out with what
  each saves: CD quality, MP3 320 (each with **do that**), and the artists taking most room with
  **leave out** (at most five, enough to fit where possible).

### 30. Covers (2026-10-30)
- The scan notes whether each song has a cover embedded (the Echo Mini shows only those). The
  library database moves to version 4: files are read once more; corrections are kept.
- **N albums without a cover** beside the search box shows only them; **find covers for these**
  (or **find cover** on one album) takes an image in the album's folder if there is one, else
  looks the album up on MusicBrainz as *look up* does (song titles must match) and fetches the
  release's front cover from the Cover Art Archive (500 px). Found covers are corrections: the
  player shows them, card copies get them embedded at the next Sync, and the music files with
  *write corrected tags*; **don't** under the player drops one. Albums with nothing found are
  listed with the reason. Downloaded covers are kept in the app's data folder (`covers/`).
- MusicBrainz requests from lookups and cover searches share one request-a-second allowance.

### 31. Searching, and a table of albums (2026-10-30)
- The search box understands terms as well as words: `format:flac`, `format:alac`,
  `quality:lossless|lossy|hires`, `card:on|off|sync|better`, `cover:no|yes`, `year:1979`,
  `year:1979..1985`, `year:..1990`, `edited:yes`; `-` in front turns a term around
  (`-format:mp3`). Hovering over the box lists them; **filters** ticks the common ones (this is
  the card filter: on the card, not on it, changing at the next Sync, a better copy available).
  Words must each appear in the artist, the album or a song title.
- **save** keeps a search beside the box; click it to search again, right-click to forget it.
- **albums** (beside **artists**) shows a table: artist, album, year, format, songs, size, card,
  cover. Click a heading to sort, again to reverse. Click to select an album, ⌘-click to add
  one, shift-click a run, double-click to play; **select them all** after a search. The bar
  above offers **send to card**, **take off card**, **edit tags**, **find covers** and **play** for
  the selection; right-click a row for the same and **open folder**.

### 32. The card's own music, step by step (2026-10-30)
- Music on a card that the write didn't put there is now one line in the card tab (**go through
  them**) and a dialog: what it is in three steps, **Check them against the library**, then the
  same choices as before and **Take over the card**. It opens by itself once, the first time a
  card that hasn't been written to has such files.

### 33. Duplicates and new music (2026-10-30)
- An album copy that another copy covers has **move it to the Trash**, and the duplicates view
  **move all N covered copies to the Trash**: the copy's folder when everything musical in it is
  the copy's, else only its songs (and CUE sheets); only while the covering copy is still there.
- **add music** (library, and Folders › *add music from a folder…*) checks a folder (Downloads by
  default) against the library and shows what's new, what the library already has (exact
  copies, left out) and other versions of songs it has (optional); **Add them** copies the new
  ones, with covers and booklets, into `Added` in the music folder, then rescans. The folder
  itself isn't changed.

### 34. More than one player: RECHO or the stock firmware (2026-10-30)
- Each card setup now says which **player** it's for, next to its destination in the card tab:
  *Echo Mini · RECHO* (the default, so existing setups are unchanged) or *Echo Mini · stock
  firmware*. This is the first step of [NEXT](docs/NEXT.md) part E: device profiles, the way
  calibre has a driver per reader.
- The player decides what's converted. The stock firmware plays FLAC, MP3, WAV, APE, AAC (M4A),
  Ogg Vorbis and WMA, so those go to the card as they are; ALAC, Opus, AIFF, WavPack and Dolby
  are still converted. On this library: 701 songs converted instead of RECHO's 871.
- Warnings, format badges, album notes and the scan summary name the chosen player ("won't play
  on RECHO" / "on the Echo Mini").
- For the stock firmware the card tab warns above 8,192 songs (playlist copies included), and
  the playlists tab warns that a player listing songs by tags shows each playlist copy as a
  second song.
- Cover hints no longer say the Echo Mini shows covers under RECHO: RECHO's site says it shows
  none. Covers matter for this app's player, the stock firmware and most other players.
- The stock firmware's facts come from reviews and haven't been checked on a player yet.

### 35. Testing the player (2026-10-31)
- The card tab's player row has **test it on the player**. It writes `Pausetape format test/`
  to the card: 6-second tones in every format (FLAC 16/44.1, 24/96, 24/192, 24/384; MP3, WAV,
  AAC, ALAC, AIFF, Opus, Ogg Vorbis, WavPack, WMA), APE copied from the library's smallest APE
  song (ffmpeg can't make APE), a FLAC with an orange and blue cover, MP3s titled
  "Café ü 日本" in ID3v2.3 and ID3v2.4, and `Pausetape format test.m3u8` beside it.
- Try them on the player and tick **yes**, **no** or **not tried** for each. The answers are kept
  with the card setup, separately for each player, and win over what's known from reviews: a
  format that didn't play is converted, songs above the highest rate that played are written at
  CD quality, and the cover answer decides whether covers count.
- The playlist and ID3 answers are recorded but don't change anything yet ([NEXT](docs/NEXT.md)
  E2b).
- The test folder isn't counted as music the write didn't put on the card, and **take it off
  the card** removes only the files the test wrote.

### 36. Recognising a card (2026-10-31)
- Every write and take-over puts a small `.pausetape` file at the top of the card naming the
  card setup (and its player), as calibre's `metadata.calibre` does. Players skip it. Setups
  now have an id for this, which saved setups get when they load.
- A card plugged in brings up its setup: the app looks at the mounted drives every few seconds,
  points the setup at where the card is mounted (a renamed card, or "Untitled 1"), and says so.
  If the active setup's own card is among those plugged in, it stays; while something runs, it
  only says so.
- A card that belongs to another setup on this Mac isn't synced or taken over by this one. The
  card tab says whose it is, with **switch to** it or **use it for this setup** (relabels the
  card). A card labelled by a setup that isn't on this Mac can be synced: only the setup's own
  files are ever removed.

### 37. Names on the card as a pattern (2026-10-31)
- The card tab's **Names on the card** takes a pattern, as calibre's save-to-disk template does:
  `{albumartist}/[{year} - ]{album}/[[{disc}-]{track} - ][{trackartist} - ]{title}`. `/` makes a
  folder; a `[part]` is left out when a field in it is empty. Fields: albumartist, album, year,
  position (your own album order), disc, track, trackartist (on compilations and loose songs),
  title. An example from the library and the folder names in the list follow as you type; a
  broken pattern says what's wrong and changes nothing.
- The three album-folder choices are patterns too, and name everything exactly as before
  (checked on the whole library: 20,832 paths, identical), so cards already written keep their
  names.
- Songs already on the card under another name (a new pattern, a renamed artist or album,
  another order) are moved there at the next Sync instead of copied again. A converted song is
  matched by what it was made from; a plain copy by its size and its first and last 64 KB. The
  comparison and the sync line count them as renames.
- The test folder no longer leaves macOS "._" files beside its samples on FAT cards, where a
  player would list them as songs that don't play.

### 38. Code review, and no dropouts during card writes (2026-10-31)
- [docs/REVIEW.md](docs/REVIEW.md) lists 26 bugs and risks found by reading the whole code base,
  with what each does, where, and a fix; [TESTING](docs/TESTING.md) §3 has those Windows and
  Linux testers will meet. Two lose data and come first: starting with the music drive unplugged
  empties the library database (unsaved corrections included), and a Sync with an empty library
  removes the setup's songs from the card.
- Playback no longer drops out while a card write (or a scan) reads the same drive: songs are
  decoded up to 8 seconds ahead, on a thread of their own. Measured on the external music drive
  under load: dropouts of up to 1.9 s before, none after.

### 39. A music drive not connected loses nothing (2026-10-31)
- Starting with the music folder's drive unplugged no longer empties the library database (and
  with it the corrections not yet written into the files). The library keeps what it had, says
  "Can't reach the music folder: is its drive connected?", and reads it again by itself once the
  drive is back. An empty folder where the drive should be counts as not reachable too, and a
  folder inside that can't be read keeps the songs known there.
- Sync, taking over the card, copying music in, and tidying wait while the music folder can't be
  reached or the library is empty; the card tab says why in place of **Sync**.
- A Sync that would take most of the setup's songs off the card (at least 20) asks first.

### 40. Damaged files, late suggestions, renamed setups (2026-10-31)
- Files that start blank where their audio should be (as a failing card or disk leaves them) are
  marked damaged: the library says how many, `damaged:yes` finds their albums, writes leave them
  out saying why, and playback skips them quietly. The next start reads every file once more.
- MusicBrainz suggestions that arrive after a rescan go to the right songs (found by id), or are
  dropped if their lookup was.
- Renaming a card setup while it syncs or takes over the card no longer loses the result: setups
  are found by their id, and so are their take-over undo records.

### 41. Four more fixes from the review (2026-10-31)
- The music folder can't be changed while something runs (it could crash the app, or leave the
  library empty).
- Undoing a take-over works with the card mounted somewhere else: new undo records keep paths
  relative to the card.
- The card's label is read again right before a Sync or take-over, and as soon as a card is
  chosen, so a setup can't sync to another setup's card in the moments before the next look.
- Songs with corrections not yet in the music files are copied to the card once, not at every
  Sync.

### 42. Softer colours (2026-10-31)
- The light theme no longer glares: charcoal text on slightly dimmed paper (about 12:1 instead of
  19:1), so the Sync and play buttons and the progress bars are charcoal too, not black. The
  warning red is a brick red instead of a pure one.
- Grey secondary text is now readable on every surface it's drawn on (it fell just short of
  4.5:1 on the tab bar and in panels).
- The dark theme takes the same warm greys, so both themes feel like the same app.
- An accent colour to choose in settings, as in Calibre: brick (as before), ochre, moss, teal,
  blue, plum or graphite (no colour). It marks the open tab, the main button (Sync, play),
  progress bars, the playhead and what's playing. Warnings stay brick red whatever the accent.
  Every preset reads at 4.5:1 or more in both themes.

### 43. The rest of the review: R10–R26 (2026-10-31)
- **Playback:** if the audio output disappears with nothing to take over (a USB DAC unplugged,
  Bluetooth headphones gone), the app opens the system's output again and carries on with the
  song where it was. Nothing is printed on quitting any more. A rescan no longer empties the
  player bar while music plays. "Skipped … can't play" says why the last one failed.
- **Undo** (a tidy or a take-over) never writes over a file that has since taken a song's old
  place; it leaves the song and says so.
- **Tags:** emptying a field in the app empties it in the file; full dates stay full while
  their year is right; the compilation flag is written; an MP3's old ID3v1 tag (and an APE tag
  beside another) gets the same corrections.
- **DSD** (`.dsf`, `.dff`) joins the library: it plays here through ffmpeg, goes on the card as
  it is for the stock firmware, and becomes 24-bit/88.2 kHz FLAC for RECHO.
- **Linked folders** in the music folder are followed (often used on Linux). A link to a drive
  that isn't connected keeps its songs in the library.
- **Card:** a song folder named like the app's own (the playlists folder, the format test) gets
  a `_` added. The card tab no longer works out every song's size on every frame.
- The playback conversion cache is capped at 1 GB instead of six files (up to ~4 GB).
- When ffmpeg can't convert a song to play it, the skipped-song notice says why, and nothing
  is printed to the terminal any more (R26).
- **Windows** (type-checked here, not yet run there): cards are found by drive letter, so a
  plugged-in card brings up its setup, with free space and filesystem; ffmpeg runs without
  console windows; the card's label file is hidden; and "Move to the Trash" leaves files on USB
  sticks, SD cards and network drives alone, where Windows would delete them for good.

### 44. The library opens without reading the folder (2026-10-31)
- Opening the app no longer walks the music folder: the library comes from the database as the
  last scan left it, corrections included (0.2 s instead of 27 s for 7,100 songs on a USB
  drive). The status line says when the folder was read.
- The folder is read when it's chosen, on **rescan**, and after the app's own changes to it
  (tidy, take-over, Trash, undo, adding music, and a tidy that only retags). Saved tags are
  re-read into the database at once. Songs added or changed by other programs show after a
  rescan. A folder the database doesn't know yet, or after "forget stored tags", is read
  through as before.
- CUE sheets are kept in the database with their contents, so opening reads nothing from the
  music folder: reading the 113 sheets from a USB drive took 1.4–2 s of the opening. Sheets
  stored before are read once more, then kept.

### 45. Songs after one at another rate play at their own speed (2026-10-31)
- A 96 kHz song played after a 44.1 kHz one came out at under half speed (and lower in pitch):
  the output kept the first song's rate. The decoder-ahead buffer (`Prefetch`) said its rate
  and channels never change, so rodio never looked again; it now says how much is left of each
  chunk, as rodio's own decoders do. Chunks are cut on whole frames.

### 46. The artists tab gives way to an artist panel in the library (2026-10-31)
- **Opening an artist** in the library shows, from MusicBrainz, their genres, country and years,
  and links to **buy** (Bandcamp first, then Qobuz, Apple Music, Discogs, the artist's shop),
  **listen** (Spotify, Apple Music, Tidal, Qobuz, Deezer, YouTube Music, SoundCloud) and
  **discover** (last.fm, Rate Your Music, Songkick, their website). Where MusicBrainz has no
  Bandcamp, Qobuz or Discogs link, a search there stands in. Asked once, when the artist is
  first opened, and kept in the library database (`artists` table, by the key the library
  groups the artist under); **refresh** asks again, **not this artist?** chooses another
  MusicBrainz artist of that name, and several of one name are offered to choose from.
- **Look-alike names** ("Jimi Hendrix" / "Jimmy Hendrix") show under the artist, with *same
  artist* / *different artists*; the chip at the top shows only those artists.
- **Rename** changes the artist and album artist in the songs' tags, as corrections written
  with the others (a guest's "feat. …" is kept). *Use MusicBrainz's name* does the same.
- **Settings › Artist decisions** lists each merge and keep-apart with its own undo.
- The artists tab is gone: the library already lists every artist.
- The panel is a card of its own: genres as small tags, then a row each for **buy** (a bag),
  **listen** (headphones) and **discover** (a compass), each site a pill that opens in the
  browser. Buying links carry the accent; searches standing in for a missing link are grey,
  with a magnifier.

### 47. A clear button in the search boxes (2026-10-31)
- The library's search and the card tab's filter get a × inside their right end once something
  is typed; Esc clears them too while typing.

### 48. The app is called Pausetape (2026-11-01)
- After the tapes once recorded from the radio, pause button at the ready, to carry a selection
  of one's own: what the app does with a library and a card.
- What the app kept under its former name carries over: its data folder (settings, library
  database, undo journals, fetched covers) moves on the first start, and the covers chosen for
  unsaved corrections are found at their new place. Cards keep being recognised by their label,
  which is rewritten under the new name at the next write; the format test's files and a
  take-over's gather folder from before are still the app's own. Temporary conversions from
  before are removed. The former name lives in one constant (`FORMER_NAME`), used for nothing
  else.
- Test switches are now `PAUSETAPE_*`.

### 49. A cassette for an icon, and reels in the app (2026-11-01)
- **The icon** is a cassette seen from the front on the app's paper, "Pausetape" written by hand
  across its label (Caveat Bold) over a band of the brick accent, side A just begun in its
  window. `scripts/make-icon.py` draws it (Pillow; the font is fetched, not shipped).
- **Beside the app's name**, the same cassette in outline.
- **Work under way** (a card write, a MusicBrainz question, the test card) shows two reels
  turning instead of a spinner.
- **The player** has a little cassette whose reels turn while the song plays and stop when it's
  paused; the tape passes from the left reel to the right as the song goes on, and each reel
  turns faster the less tape it holds, as on a real deck.

### 50. Everything on an 8-point grid (2026-11-01)
- **Spacing:** every gap, padding and margin is a multiple of 8, from `ui/layout.rs` (`S1` 8,
  `S2` 16, `S3` 24 … `S6` 48): `style.spacing` (items 8 apart, controls a row of 24 tall,
  indents of 16), the frames (cards and panels 16 inside, dialogs 24, the page 40 at the
  sides), and every space between parts (thirteen sizes before, from 2 to 52).
- **Controls** have fixed heights on the grid, their contents centred in them: links, commands,
  chips, tags and fields 24, buttons and tabs 32; icons beside words are 16, 8 from them.
- **Rows** of controls and songs go left to right with everything centred on one line
  (`layout::row`, an explicit `Layout::left_to_right(Align::Center)` one row tall); text
  fields centre their words too. Settings keep their names level with the first control.
- **Panels:** the tab bar and the player are the app's frame, tinted and ruled off; work under
  way and the latest notice share one strip under the tabs; the library's A–Z rail is a panel
  of its own at the right edge. In the library, search and the chips of what's worth a look
  are on separate rows. The player's cover is exactly as tall as the title, sparkline and
  buttons beside it.

### 51. A website (2026-11-01)
- `site/`, published on GitHub Pages from `main`: what Pausetape is for, in the spirit of the
  pause-button tapes of the 90s; a five-step tour (choose the music, the card, tidy, pick,
  record) whose ring moves over real screenshots, closing in on a phone; downloads from the
  latest release, with how to open the app the first time on each system; questions.
  Light and dark like the app, no outside fonts or tracking; motion stops under *Reduce
  motion*. How it's made and kept up: [docs/WEBSITE.md](docs/WEBSITE.md).
- `scripts/site-shots.py` takes its screenshots from a made-up library and an empty FAT32
  disk image, in the app's own screenshot demo, which gains `PAUSETAPE_SHOT_CARD` and
  `PAUSETAPE_SHOT_QUIET`.

### 52. Search ignores accents (2026-11-01)
- "bjork" finds Björk, "sigur ros" Sigur Rós, "strasse" Straße, and "don't" a title spelt with
  a curly apostrophe: the library's search and the card tab's filter drop the accents of Latin
  letters, on both sides, before comparing. Other scripts are compared as they are, so "кино"
  still finds Кино.

### 53. Text centred on Windows too (2026-11-01)
- On Windows, words sat a few points above the icons beside them and above the middle of
  buttons. Palatino Linotype has a line gap of a third of its size, which egui puts below each
  line. The app now drops a font's line gap when it loads it, so text is centred and lines are
  as close as on macOS. Seen in the first run on Windows.

### 54. No console window on Windows (2026-11-01)
- Release builds on Windows are a windowed program, so no console opens behind the app.
  Started from a terminal, they attach to it: `--version`, `--plan` and errors print there.
  Terminals don't wait for a windowed program, so in PowerShell add `| Out-Host`; `cargo run`
  waits by itself. Debug builds keep their console. Run on Windows 11.

### 55. Track numbers clear of the card mark (2026-11-01)
- On albums of several discs, a number such as "2-10" ran into the ring at the left that says
  whether the song is on the card. The number column now fits the album's widest number, the
  same for all its songs, so their titles still line up. Seen on macOS and Windows.

### 56. The format note moves to the card tab (2026-11-01)
- The library no longer opens with a red box about songs the card's player can't play: the
  app converts them on the way, so it wasn't a problem to act on. Each such song keeps its
  badge ("M4A · not on RECHO"), and the card tab's *Formats RECHO can't play* says what plays
  in the app, what needs ffmpeg, and what each song becomes. Red stays for songs that would be
  left out.
- Messages that ask for ffmpeg say how to install it on the system in use (`winget install
  Gyan.FFmpeg` on Windows), not `brew install ffmpeg` everywhere.

### 57. First run on Windows, written down (2026-11-01)
- [TESTING](docs/TESTING.md) §3, [REVIEW](docs/REVIEW.md), [DISTRIBUTION](docs/DISTRIBUTION.md)
  and [PROGRESS](docs/PROGRESS.md) say what was seen on Windows 11 instead of "not yet run":
  drives and free space are listed, a FAT32 stick was written, rewritten ("0 copied") and had
  an album unticked without touching its other files, AAC was converted, `.pausetape` is
  hidden, and the Trash leaves files on cards alone. Reserved names and a 374-character path
  were written and read back. What still needs the app in hand is listed.

### 58. The cassette on Windows too (2026-11-02)
- The window and the taskbar show the cassette instead of egui's icon, and on Windows the
  .exe carries it too, for Explorer and the Start menu, with the app's name and version in
  its properties. `build.rs` makes them from the one drawing, `assets/icon/icon-1024.png`, so
  nothing derived is kept in the repository.

### 59. Drives in the in-app browser on Windows (2026-11-02)
- On Windows, **Drives** in the folder browser opens "Computer": every drive by letter and
  label with its free space, Windows' own and network drives too. "back up" from a drive's
  top folder and the breadcrumb lead there, and **Use a folder on a card…** starts there, so
  **system picker…** isn't needed to reach another drive. Seen on Windows 11.

### 60. MusicBrainz suggestions easier to read (2026-11-02)
- An album's suggestions sit in a frame headed *MusicBrainz suggests*, one change per row:
  a tick in the accent colour when it's chosen, an empty ring when it's left out. A click
  anywhere on the row switches it. What the tag will be is in ink and what goes is struck
  through, so a chosen change strikes the old value and a left-out one the suggestion.
- The header counts what's chosen; **apply N changes** is the main button (greyed out with
  none), beside **dismiss**, *choose all* and *none*.

### 61. The card tab says what's there and what's changing (2026-11-02)
- A tick only said what the card would have after Sync, so a ticked album could be on the card
  already or about to be copied, and an unticked one absent or about to be taken off. Each
  album now has the library's mark beside its tick, with words: ● *on card*, ○ *added at the
  next sync* (or *3 of 4 on card, the rest at the next sync*), and a struck red ring *taken off
  at the next sync*, its title struck through. Each artist counts its albums on the card, added
  and taken off; a legend above the list says what the marks mean.
- **what the next Sync changes (N)**, beside **everything**, lists only the albums Sync adds or
  takes off, their artists opened, to look over before it runs.
- **A card with music copied by hand**, before Pausetape has written to it: once *Check them
  against the library* has run, albums it found there show *on the card, copied by hand*, with
  their own mark (a ring with a dot), even unticked. The dialog offers **Tick them**, first, so
  a new setup starts from what the card holds; *Take over the card* then moves those songs into
  place instead of copying them again.
- The screenshot demo gets `PAUSETAPE_SHOT_EMPTY=1` (nothing chosen, as for a new card), and its
  take-over shot compares and checks the card again first, so the dialog shows.

### 62. A pixel room on the website (2026-11-02)
- Beside "Pause. Record. Pause.", in place of the little tape deck: an 8-bit room where,
  sitting beside a boombox, someone in a grey hoodie opens its deck, puts in the tape from
  their lap, closes it and presses play; the reels turn, the speaker pumps, notes float up
  while they listen, and then they turn the tape over to side B. A cat sleeps on the sill.
  Afternoon in a light theme, night in a dark one; one still picture under *Reduce motion*.
  Drawn in code (`site/pixel.js`), no image files.
- It draws only when its picture changes (15 times a second, not at every screen refresh),
  and sits centred in its column when whole pixels leave room beside it, as on a phone.
- The foot of the page is signed, as a tape's label would be: *made by Emiliano Solórzano*,
  the name by hand. The handwriting font now has accented letters (Latin-1), 9 KB more.

### 63. The website's screenshots taken again (2026-11-02)
- The tour and the top of the page show the app as it is now: the card tab's marks and views,
  MusicBrainz suggestions with their ticks, no format note above the library. The marks of
  steps 3 and 4 moved to fit, and step 4's note says what the marks beside the ticks mean.
- `scripts/site-shots.py` makes its demo folder first, as it failed once that was deleted.

### 64. Releases without a bare Mac binary (2026-11-02)
- A version's release gets the Mac disk image (macos.yml) and the Windows and Linux programs
  (ci.yml), no longer also the Mac program on its own, which only opens in Terminal and would
  confuse someone choosing what to download.

### 65. Keyboard focus you can see; controls screen readers can name (2026-11-05)
- Tab moves through the app's own controls (links, commands, buttons, tabs, chips, song and
  album rows, column headings, the A–Z rail, the folder browser), and the one with the focus
  now has a ring in the accent around it; Space or Enter presses it. Before, Tab worked but
  nothing showed where it was. A click doesn't give focus, so the ring only appears from the
  keyboard.
- Each of those controls now tells VoiceOver, Narrator and other screen readers its name and
  what it is (button, link, tab, row; selected or not). They were drawn by hand and said
  nothing. Icon-only buttons take the name they show on hover.
- While a control has the focus, Space presses it and ←/→ move to the next control, instead
  of also pausing or seeking the player.

### 66. Undo for playlists and the card's music; asking before what can't be undone (2026-11-05)
- **⌘Z** (Ctrl+Z on Windows and Linux) takes back the latest change to playlists (a playlist
  made, renamed or deleted, a song added, moved or taken out) or to what a card setup puts on
  its card (albums and artists sent or taken off, tick all and untick all, names and order on
  the card), up to 50 of them. The notice says what was undone. Changes to files keep their
  own undo commands, so a keystroke never moves files; text fields keep their own ⌘Z.
- **Deleting a playlist** no longer happens silently: the notice says so and offers *undo*, and
  card setups stop listing it.
- **Renaming a playlist** is taken when the field is left, and a name that's empty or another
  playlist's (card folders ignore case) is refused with a notice, keeping the old one. Before,
  each keystroke renamed it in every card setup, and two playlists could share a name. A pencil
  beside the name shows it can be renamed. New playlists get a number no other has.
- **Deleting a card setup** asks first and says what goes with it: its record of the files it
  put on the card, which no setup will then update or take off, and how to clear them first.
- **Forgetting the stored tags** (Settings) asks first and says what it costs: the next rescan
  opens every file again.

### 67. A click selects, a double click plays, in every view (2026-11-05)
- In the library by artist, a click on a song played it, and selecting needed ⌘-click: the
  opposite of the album table, Finder, Explorer and other players, so looking around with the
  mouse interrupted the music. Now a click selects the song (alone), ⌘-click adds or removes
  one, shift-click a run, and a double click plays from that song to the album's end.
- One selected song is only highlighted; the bar with what to do with a selection appears from
  two songs, so a click doesn't push the list down. Esc clears either.

### 68. Keyboard shortcuts for the library (2026-11-05)
- **↑ ↓** move the selection through the songs shown (library by artist) or the albums (album
  table), scrolling to keep it in view; with **Shift**, they add to it. **Enter** plays what's
  selected (one song plays on through its album, as a double click does).
- **⌘A** selects everything shown, **⌘I** edits the tags of what's selected, **⌘D** sends the
  selected albums (or the albums of the selected songs) to the card, or takes them off if
  they're all there.
- **⌘F** goes to the library's search box (in the card tab, to its filter); **⌘⇧S** syncs the
  card, saying why not if it isn't connected or something else is running.
- On Windows and Linux, Ctrl in place of ⌘. Settings › Keyboard lists them all, as each
  system writes them, and the Sync, *send to card* and *edit tags* commands show theirs on
  hover. Shortcuts wait while you type in a field or a dialog is open; the arrows and Enter
  also leave a control focused with Tab alone.

### 69. One right-click menu for music, everywhere (2026-11-05)
- Right-click an artist's name, an album's title (library by artist), an album's row (album
  table), a song, or a playlist's song: the same menu each time, in the same order. **Play**
  (a song plays on through its album) · **Add to playlist ▸** (any playlist, or a new one) ·
  **Send to the card** or **Take off the card** · **Edit tags** · **Find cover** (when there's
  none) · **Look up on MusicBrainz** (one album) · **Open folder**. A playlist's song adds
  **Remove from the playlist**, a library song adding to or taking out of the selection.
- On something in a selection of several, the menu acts on the whole selection ("Play 3 songs",
  "Send 2 albums to the card").
- Before, songs could only go into the open playlist, with a + that showed on hover; the + stays,
  and adding now says so in a notice that offers *undo*.

### 70. Drop a folder on the window to add its music (2026-11-05)
- Dragging a folder (an album, or a folder of albums) over the window covers the page with
  "Drop a folder to add its music to your library"; dropping it does what *add music* does
  after choosing a folder: checks it against the library, then asks what to copy.
- Single files are turned down with a note to drop their folder instead (checking the folder
  they're in could mean all of Downloads); of several folders, the first is checked. While a
  dialog is open or something is running, the drop waits and a notice says why.

### 71. One look and one name per action (2026-11-05)
- The same action looked and was called different things in different places: *Change
  folder…* was a button in the folders tab and a quiet link in the library; reading the folder
  again was *read again* there and *rescan* here; adding music was *add music from a folder…*
  and *add music*; the card was *Change card…* in the folders tab and a link in the card tab;
  Sync was filled in the card tab and outlined in the library. Now each has one name and one
  look: outlined buttons for **Change folder…**, **Rescan**, **Add music…** and **Change
  card…**, and Sync filled wherever it is, as the main thing to do.
- Leaving a dialog is a framed **Cancel** in each (the tag editor's had an icon), or **Not now**
  where the offer comes back (taking a card over), or **Done** where there's nothing to cancel.
- In the card tab the card's path gives way to *Change card…*, shortened with "…" and shown
  whole on hover, so the button is never pushed out of a narrow window.

### 72. Commands in sentence case; one word for each thing (2026-11-05)
- Every command now starts with a capital, as buttons did already: *Rescan*, *Edit tags*,
  *Find cover*, *Open folder*, *Tick all*, the tabs (*Library*, *Card*…), *Artists* and
  *Albums*. Before, buttons were capitalised and the links beside them weren't. Descriptions
  and states stay in lower case.
- The card's states are said one way everywhere: *on the card*, *added at the next Sync*,
  *taken off at the next Sync* (in the album table, shortened to *added at Sync* and *taken off
  at Sync*); before, the same state was also *on card*, *at next sync* or *coming off*. Sync is
  always written with a capital, as on its button. *Send to card* became **Send to the card**.
- What the app called a *write* (what a card gets, its names and order, its record of what's
  there) is a **card setup** wherever it's shown: the card tab's list is *Card setups*, and
  "this write" became "this setup". The code keeps its names.

### 73. Teal by default, warnings apart from it; edges and selections that show (2026-11-05)
- **The default accent is teal.** Brick, the accent until now, was also the colour of every
  warning, so the Sync button, the playhead and the open tab looked like "none", "taken off"
  and "isn't formatted FAT32". Warnings and what can't be undone stay brick red. An accent
  chosen in Settings is kept (an install that never chose one keeps the brick it saved).
- **Fields, checkboxes and dropdowns have an edge you can see**, 3:1 or more against the page
  in both themes (it was 1.3:1); the faint colour stays for rules and dividers.
- **A selected song or album** is tinted with the accent and has a bar of it at its left edge,
  instead of the grey chip the hover nearly matched; grey text on it stays at 4.5:1 or more
  with every accent. **A row under the pointer** (or with the keyboard's focus) is now a
  visible step darker; before, its background was 1.08:1 against the page.

### 74. A bar that shows how full the card will be (2026-11-05)
- The card's room was a hairline (1.3:1 against the page) with a thin line over it. It's now a
  flat bar, 6 points high, of the card as the next Sync leaves it: what stays on it in ink,
  what Sync adds in the accent, what it takes off as an outline in the warning colour (that
  space comes free), and what's left free; past the end, in the warning colour, what doesn't
  fit. Hovering says each in figures. The library's strip shows it a little longer.

### 75. A status bar at the foot of the window (2026-11-05)
- Work under way (the reels turning, what it is, how far, the time left, **Stop**) and the
  latest notice (with **Undo** when it offers one) used to appear in a strip under the tabs,
  which pushed the whole page down as they came and went: whatever was under the pointer
  moved. They now share one line at the foot of the window, always there, below the player.
  A notice too long for it ends in "…" and shows whole on hover.
- The same bar says how many songs, artists and albums the library has, and how many are
  selected; the library's heading no longer repeats the counts.

### 76. The queue, Play next, and a player that folds to one line (2026-11-05)
- **Play next** and **Add to the queue** in every right-click menu (once something is playing):
  songs go right after the one playing, or at the end, and the song playing goes on.
- **The queue opens from the player** (a click on "2 of 8", beside the cassette): what played,
  what's playing (in the accent) and what's next, each song with *Move up*, *Move down* and
  *Take out of the queue*; a double click plays one; *Clear what's next* empties the rest.
  It closes with a click elsewhere or Esc.
- The playback thread can now change its queue while it plays: it's told where each song went,
  so the one playing keeps playing and what follows it is what the queue says.
- **A click on the player's cover folds it to one line** (cover, title and artist, the buttons,
  the sparkline, the time, the queue, the volume), and back; the choice is kept. A window lower
  than 640 points always has the one-line player, so the page keeps most of it: at 760 × 540 the
  library now shows its list.

### 77. An artist's links folded away until asked for (2026-11-05)
- Opening an artist showed up to 17 buttons to stores, streaming services and sites (Buy,
  Listen, Discover) before its albums. They're now behind **Where to buy and listen (N)**, in
  the artist's line of commands beside *Rename*; **Hide the links** folds them again. Genres,
  country and years stay in view. Each artist is remembered open or folded until the app quits.

### 78. Flat panels (2026-11-05)
- Panels (the card's strip, the selection bars, an artist's facts, MusicBrainz suggestions, the
  folders tab's steps, the views of duplicates and look-alikes) were marked twice, with a
  background and an outline, at 8-point corners. Following the flat design, each is now set
  apart by its background alone, the tint of the tab bar, at 4-point corners. Warnings keep
  their outline, which is what marks them.

### 79. Nothing runs into the player's volume or an album's commands (2026-11-05)
- In the player, the note about a cover found as an image file ("cover from …, not in the
  songs' tags · Add it") ran on into the volume control in narrower windows. The volume now
  keeps its place, the note shortens with "…" to the room left (whole on hover), and where
  there's no room for words, a lone **Add cover** (or **Don't add**) link says it all.
- In the library by artist, an album's commands (*Play*, *Find cover*, *Look up*, *Edit tags*,
  *Send to the card*…) were drawn over its title and state in a narrow window. Where they
  don't fit beside them, they go on a line of their own under the title, in the same order.

### 80. Covers in BMP, GIF or WebP; a way to find one where none shows (2026-11-05)
- Some taggers embed the cover as a BMP (or GIF, or WebP); Pausetape read only JPEG and PNG.
  Such an album showed its initial in the player and, since its songs do have a picture, the
  library didn't count it as without a cover, so *Find cover* wasn't offered anywhere. Those
  formats are now read (shown, and made a JPEG when embedded in copies).
- An embedded picture that still can't be read (damaged) gives way to an image in the album's
  folder, as when there's none embedded.
- When the player has no cover to show, it says so and offers **Find cover** beside the queue,
  where the missing cover is noticed: it looks in the album's folder, then on the Cover Art
  Archive, as *Find cover* in the library does.

### 81. A long title no longer pushes the player past the window (2026-11-05)
- A song with a very long title (or artist and album) widened the player beyond the window: the
  time, the volume and the end of the cover note were pushed off its right edge. The time now
  keeps its place, and the title, then artist · album · year, shorten with "…" to the room
  there is (whole on hover).

### 82. A sidebar in place of the tabs (2026-11-05)
- The tabs across the top are gone. A sidebar on the left holds the whole app, as Calibre's
  tag browser does: the music folder (a menu to change it, rescan it or add music), the
  library's two views (**Albums**, **Artists**, with how many), **Review** (what's worth a look:
  albums without a cover, duplicates, artists that look alike, damaged files, corrections not
  yet in the files; only what there is, with how many), saved **Searches** (right-click to
  forget one), **Playlists** (and *New playlist*), the **Card** (with how many albums the next
  Sync changes), and **Settings** at the foot. What's shown is filled in with the accent's tint.
  In a window narrower than 900 points it keeps only its icons, their names on hover.
- **The search box is in the top bar**, beside the app's name, with *Filters* and *Save*: it
  searches the library from anywhere (typing opens the library).
- The library's page starts with what it shows ("Albums", "Without a cover", "Searching
  “format:flac”"), and *Collapse all* or *Write N corrected tags* where they apply. The chips
  that led to what's worth a look became the sidebar's Review.

### 83. The album table by default, and the details of what's selected (2026-11-05)
- **Albums**, the sortable table, is the library's first view (in a new install; the view chosen
  last is kept). **Artists**, the list with letters and every song, is the other, in the sidebar.
- **The details panel**, at the right of the library where there's room for it (about 1000
  points for the library), shows the album selected: its cover, the size of the panel; title,
  artist (a click opens them in the library by artist), year, format, songs, size; where it
  stands with the card; *Play*, *Send to the card*, *Edit tags*, *Look up*, *Find cover*, *Open
  folder*; and its songs, a double click playing from one. Several albums selected: how many,
  their songs and size, what to do with all of them. Nothing selected: what's playing.
- Where there's no room beside the library, right-click › **Details** shows the same as a sheet
  over the page.

### 84. The card's view: what's on it, and how it's set up (2026-11-05)
- **On the card** shows the albums on the card, or going on or off at the next Sync, in the
  same table as the library (sorting, selecting, ⌘D, the right-click menu), and the search box
  narrows them; **What the next Sync changes** keeps only those. The comparison with the card,
  and music on it that this setup didn't put there, are above it. With nothing chosen yet, it
  says how to choose.
- **Setup** holds the rest, once set: the card's name, where it is, the player and its test,
  playlists on it, folders and options, and, folded, the folder names and order artist by
  artist (the list with ticks that used to fill the tab).
- With one card setup, its list (and *New*, *Duplicate*) is gone: **Another card setup… ▾**
  beside the name makes a second, new or a copy. From two, the list is back, and each setup
  is an entry under *Card* in the sidebar.
- The search box takes `card:all`: albums on the card, going on or off.

### 85. A welcome the first time, in place of the folders tab (2026-11-05)
- The folders tab, which was first among the tabs for good after being needed once, is gone.
  The first time (no music folder yet), a **Welcome** over the page asks for the two things:
  your music folder, then the SD card, which can wait (**Start, and choose the card later**).
- After that, the music folder (with *Change folder…*, *Rescan*, *Add music…*) and reorganising
  it (*Advanced*) are in **Settings › Music folder**, and the folder's menu is at the top of the
  sidebar; the card is chosen in the card's *Setup*.
- Music already on a card is explained after the welcome, not over it.

### 86. Suggested tags where you'll see them (2026-11-05)
- MusicBrainz suggestions showed only under their album in the library by artist, so with the
  album table (now the first view) a *Look up* seemed to do nothing. They now show in three
  places: under the album in the library by artist, as before; in the album's details; and in
  **Review › Suggested tags**, which lists every album looked up, one after another, to apply
  or dismiss. A *Look up* from the table says where its suggestions will be.
- In a narrow space (the details panel) a suggestion's field takes less room, leaving it to the
  old and new values; its buttons wrap.

### 87. Drag music onto a playlist; drag a playlist's songs into order (2026-11-05)
- Songs (library by artist) and albums (the table) can be dragged onto a playlist in the
  sidebar, which is outlined while they're held over it; on **New playlist**, they make one.
  A selection goes whole when it's what's dragged. What's held follows the pointer, in words
  ("6 songs · drop on a playlist").
- In a playlist, each song has a handle at its left: drag it, and a line in the accent shows
  where it goes. *Move up* and *Move down* stay, for the keyboard. Each move can be undone.
- The playlist's page no longer repeats the list of playlists (it's in the sidebar); how
  playlists reach the card is one line, the rest on hover.

### 88. Genres: in the album's details, and a view of their own (2026-11-05)
- **Genres are read from the songs' tags**, several where a tag holds several ("Electronic;
  Trip Hop", or separate values); spellings of one genre ("Trip Hop", "trip-hop") count as
  one, shown as most songs spell it. CUE sheets' `REM GENRE` too. The library is read again
  once, for them.
- **The details panel shows an album's genres**, each leading to that genre. An album whose
  songs name none takes its artist's genres from MusicBrainz, where the artist has been looked
  up, and says so.
- **Genres in the sidebar**: every genre with how many albums and artists (by name, or most
  albums first), and *No genre*. A click shows the genre's artists and their albums, as the
  library by artist does; an album with several genres is under each, and one there only by
  its artist's genres is marked so. The search box takes `genre:trip-hop` and `genre:none`.
- **Look up on MusicBrainz suggests the album's genres** (its release group's, most votes
  first, up to three), with the other tags; applied, they're corrections like the rest:
  written into card copies at the next Sync, and into the files with *Write corrected tags*.

### 89. A folder dropped while work runs is kept, and notices show beside work (2026-11-06)
- A folder dropped on the window while other work ran (writing tags, a Sync, a scan) was turned
  away with a notice, and the status bar, showing the work, hid that notice: the drop seemed to
  do nothing. It's now kept, the notice says it will be checked once that work is done, and it
  is, by itself.
- The status bar shows the latest notice beside work under way, not only once it's done.
- *Write N corrected tags to source files* says on hover what takes long: a cover added makes
  the tags bigger than the room kept for them, so the whole file is written again (about a
  minute per gigabyte on a USB drive), and that it can be stopped and carried on.
- Songs whose tags couldn't be written are now listed, one line per file with why, at the top
  of the library (and the card's view), with *Open the full list* (a text file in the data
  folder); before, only how many failed was said. A cover that couldn't be made names the
  song as well as the image.

### 90. The A–Z rail stays beside the list (2026-11-06)
- In the library by artist, with the details panel showing, the A–Z rail could end up over the
  details. Something wider than the page above the list (the list of songs whose tags couldn't
  be written was up to 760 points wide) widened the page in egui's layout, and the rail, placed
  at the page's right edge, followed. The rail and the list now keep to the page's edge as it
  was before anything was laid out, what's too wide is cut there rather than drawn over the
  details, and the problems list fits the page.
- The screenshot demo gets `PAUSETAPE_SHOT_PROBLEMS=1`, a list of problems with long paths.

### 91. Lyrics, in time with the song (2026-11-06)
- **The details panel shows lyrics**: *Songs* | *Lyrics* above the album's songs. For the song
  playing, lyrics with times follow it: the line being sung in the accent, those sung in grey,
  kept in view; a click on a line goes to that moment. The arrows show the album's other songs'
  lyrics (*Play it* to have them in time).
- **In time or not, said first**: above the words, *In time · click a line to go to it*, *In
  time while it plays*, or *Words only: these aren't in time with the song*.
- **Where nothing is sung** (the intro, a solo, the end) a break shows as three dots; under way,
  they're in the accent and fill as it goes. A break of a minute or more also says how long it
  lasts, and under way counts down: *the words start in 5:12*, *the next line in 2:41*. A
  shorter one is only its dots (its length when pointed at), so songs with many short breaks
  stay quiet. A break is where an empty LRC line marks one, or 8 seconds into a line when the
  next is more than 15 away (a slow verse isn't one); under 4 seconds, none is shown.
- **The panel doesn't fight back**: once *Songs | Lyrics* is scrolled away, the album's title
  and the two tabs stay pinned at the top (the title, or ↑, goes back up). Scrolling the panel
  by hand stops it following the singing; *Follow the song*, at its foot, takes it back to the
  line being sung (as does a click on a line, or the next song).
- **Where they come from:** the song's own first (a `.lrc` file with its name beside it, or the
  lyrics in its tags), else **LRCLIB** (lrclib.net, an open lyrics database matched by artist,
  title, album and length within 2 seconds, then by artist and title within 3). The panel says
  which; *Not right?* asks LRCLIB again. The song's own are read each time, so a `.lrc` added
  later wins over what was kept.
- **When:** a song's are looked for when they're shown, one request; **Find lyrics** (in the
  panel, for the album, and in every right-click menu, for a selection) fetches them ahead,
  counted in the status bar, and can be stopped. Requests go one at a time, apart, and say who's
  asking, as LRCLIB asks.
- **Kept**: in the library database, found or not found, so they show offline after and nothing
  is asked twice. The music folder isn't touched, and nothing goes on the card for now.

### 92. The details panel's cover follows a found one (2026-11-06)
- A cover found (*Find cover*), added from the playing song's folder, dropped, or written into
  the files showed in the player but not in the details panel, which kept the cover it had
  first loaded for the album. Both are now looked up again whenever their songs' cover changes,
  the one shown kept until the new one is ready.

### 93. The app brings its fonts, on one type scale (2026-11-06)
UI/UX phase 4, first part (§4.3 of the design notes).
- **ET Book** (Tufte's, MIT) for the music, the same on every system; album titles in **its own
  italic**, not the roman slanted. What it lacks (ł, ş, ž…) comes from the system's serif, then
  its CJK faces, as before. Windows no longer depends on Palatino Linotype.
- **Courier Prime** (OFL), typed, for figures standing alone: songs' durations, the player's
  time, the album table's year, format, songs and size, the year beside an album. They line up
  in columns now (`4:32` over `3:06`). Figures within a sentence stay in the serif.
- **Kalam** (OFL), handwritten, for the names of cards and playlists: in the sidebar, the card
  bar, and where they're renamed, as on a tape's label.
- **One type scale** (`layout::font`): 12 for section labels in capitals, 13 for lesser facts,
  14 for the interface, 16 for rows and reading, 20 for titles, 24 for a page's, 28 for the
  album in the details panel. The 18 sizes used before map onto it.
- The fonts and their licences are in `assets/fonts`; the macOS app carries the licences in
  `Contents/Resources/Licenses/Fonts`. The binary grows by about 700 KB.

### 94. Text size and compact lists (2026-11-06)
UI/UX phase 4 (§4.3, §4.6 of the design notes).
- **Text size**, in Settings › Appearance: *Smaller* and *Larger* step by a tenth, from 80% to
  200%, with *As designed* to go back. Everything grows with the text (egui's zoom), so nothing
  is cut off. ⌘+, ⌘− and ⌘0 (Ctrl on Windows) do the same anywhere, and are listed under
  Keyboard. The size is kept for the next start.
- **Compact lists**, in Settings › Appearance › Lists: songs and albums packed 24 points apart,
  as in Calibre, rather than 32. It applies to the album table, an album's songs, the songs in
  the details panel, a playlist and the queue (`layout::list_rows`).
- The screenshot demo gets `PAUSETAPE_SHOT_COMPACT=1` and `PAUSETAPE_SHOT_TEXT=<size>`.

### 95. Motion, and less of it when asked (2026-11-06)
UI/UX phase 4, last part (§4.7 of the design notes): movement only to show what changed, quick,
never bouncing (`ui/motion.rs`).
- **Hover and focus** fade in and out over 120 ms, rather than flicking: rows, links, commands,
  buttons, chips, tabs, the player's buttons, the search's ×.
- **The player** folds and unfolds over 250 ms (a click on its cover, or the window getting too
  low), its height growing or shrinking to the new one rather than jumping.
- **The card's bar**: after a Sync, what was "added at the next Sync" slides into "on the card"
  over 300 ms; so does any change to what's chosen.
- **Reduce motion**, in Settings › Appearance, and followed on its own when the system asks for
  it (macOS's *Reduce motion*, Windows' *Animation effects* off, GNOME's animations off; read at
  start): every change happens at once, egui's own included (sections opening, scrolling to a
  row), and the cassette's reels stand still, in the player and in place of spinners.
- Not done: the details panel doesn't slide in, as it's always there when the window has room
  for it; "Done · Eject" after a Sync needs ejecting the card, a feature of its own.

### 96. Tags written into MP3s with two tags, or zeros before their audio (2026-11-06)
Writing corrected tags failed on 401 of this library's MP3s with "failed to write to file:
failed to parse file: no format could be determined from the provided file". Lofty tells a
file's kind from its extension when reading, but from its contents only when writing: past one
ID3v2 tag, it looks for the audio within 1 KB. Two shapes of file keep it from finding it.
- **Two ID3v2 tags, one after the other** (83 files: Cocteau Twins, Epitaph, Haken's
  *Aquarius*): a v2.3 tag, then a v2.4 one that some tagger added in place of replacing it.
  Letting lofty look further wouldn't do: it writes over the first and leaves the second, the one
  read afterwards, so the old tags come back. The file is now rewritten with one tag before its
  audio, holding both tags' frames as they were read (the later tag's winning, as players read
  them).
- **1044 zero bytes between the tag and the audio** (318 files: Rush, Dead Can Dance, Gojira, The
  Mars Volta, Guthrie Govan…): the zeros go too.
- The rewrite goes into a copy beside the file (`.<name>.pausetape`), which takes its place only
  once written, so a failure leaves the file as it was. Anything else lofty can't place now says
  "the audio isn't where its tags say it starts, so the tags weren't written".
- **More files called damaged** (`Track::damaged`): the ones that start blank but read (12
  Megadeth and Doors MP3s, a few seconds of audio after up to 835 KB of zeros), and the ones that
  end inside their own tag (*Spanish Caravan*: a 76 KB tag in a 36 KB file). With the 57 known
  before (R8), 70 in this library, all from a restored backup. They're left off the card, as
  before, and found with `damaged:yes`. The library database's version went up (7), so every file
  is read once more at the next start.
- Checked against the whole library: of the 471 files that failed, the 401 written into copies
  read back with their new tags, artist, album and length kept; the other 70 are the damaged ones.

### 97. rustfmt leaves the code alone (2026-11-06)
- An editor extension (Trunk) ran its own rustfmt, version 1.65, over the files changed since
  the last push, and rewrapped 24 of them: lines cut at 100 characters, `let … else` spread over
  three lines, imports in the old order. `rustfmt.toml` now says `disable_all_formatting`, which
  every rustfmt reads, old and new, run by `cargo fmt`, rust-analyzer or Trunk.

### 98. Windows: read-only songs, and keys by their names (2026-11-06)
- *Write corrected tags* refused songs marked read-only on Windows ("access denied", with no
  reason given); on the first music library tried there, 1,327 of them were. They're now
  unlocked and written, and stay writable, as was asked for. Taking over a card does the same
  with its read-only files on Windows, as it already did with locked ones on macOS
  (`platform::unlock`).
- Hints that said "⌘-click" and "⌘-Return" say "Ctrl+click" and "Ctrl+Enter" on Windows and
  Linux, as Settings › Keyboard already did.
- The list of what *Reorganise* would move showed `Artist\Album/01 - Title` on Windows: one
  separator now.

### 99. Narrow windows and long names keep things apart (2026-11-07)
- A long genre (`Post-Punk, Shoegaze, Drone, …`) as a chip in the details panel was wider than
  the panel, and egui then reserved too little of the page for it: the albums, and the A–Z rail,
  ran under the panel. Chips now end in "…" at their column's edge, with the whole name on hover.
- The library's title wraps in the room *Collapse all* and *Write corrected tags* leave it,
  rather than running under them; where that room is too little, they go on a line of their own.
- The one-line player gives what's left after its buttons to the time, then the queue, then the
  volume, and the sparkline only if there's room to scrub; what doesn't fit is left out rather
  than drawn over the buttons.
- The album table leaves out cover, size, format and year, in that order, where it's too narrow
  for every column, rather than cutting the last ones off at the edge.
- In *Up next*, a shortened artist ends a gap short of the duration rather than touching it.
- The top bar's search box gets shorter so *Filters* (and *Save*) stay whole with large text.
- Where the sidebar would need scrolling (a window 760 high), its items sit a little closer, so
  the card's section at the foot stays in view.

### 100. Songs whose audio breaks off are found, and say so (2026-11-07)
- An AAC song made Symphonia panic ("index out of bounds" in its AAC decoder): its audio had
  stretches of zeros where a copy, or a download, didn't finish. The song stopped short, its
  waveform never came, and the reason went only to a terminal. The decoder is now caught where it
  panics, while playing and while reading the waveform.
- Reading the waveform, which goes through the whole song as it starts, also tells when its audio
  is broken: the decoder gave up part way, or far less audio came out than its tags say it lasts
  (under half; none at all, in two of the songs above). The song then joins *Review › Damaged
  files*, with what's wrong ("its audio stops making sense at 0:01") on hover, and a notice says
  so. It's still played and copied to the card as it is: other players skip a bad stretch where
  Symphonia stops (one song here plays whole in ffmpeg, but only 16 s here). The library database
  keeps the note until the file changes, so a fresh copy is sound until found otherwise.
- Damaged songs show a *damaged* badge in the library, the player and playlists. Before, one that
  started blank showed no badge, or one about its format.
- On this library: Kendrick Lamar's *GNX* in M4A (all 12 songs) and most of John Petrucci's
  *Suspended Animation* (cut off, ffmpeg agrees) are damaged; no sound MP3 or M4A was flagged.

### 101. Done · Eject (2026-11-06)
The last piece of UI/UX phase 4 (§4.7 and §6.2 of the design notes): a Sync that finishes says
so, and offers the next step.
- The notice of a finished Sync starts with **Done**, and offers **Eject** beside it, as a
  change that can be undone offers *Undo*, for as long as it's the latest notice and the card is
  still there. Not after a stopped Sync, nor when the music folder is on the card's drive (that
  would take the library away too).
- Ejecting runs in the background (`platform::eject`): macOS `diskutil eject`; Windows what
  Explorer's *Eject* does (lock the volume, dismount it, allow and ask for the medium's
  removal), trying the lock for two seconds while a file on it closes; Linux `gio mount
  --eject`. The notice then says "“ECHO MINI” ejected: the card can come out", or why not: on
  macOS, which program still has something open there, from diskutil's answer.
- Checked on macOS with a FAT32 disk image: ejected, and refused while a program kept a file on
  it open. Windows and Linux not tried yet ([TESTING](docs/TESTING.md) step 18c).

### 102. The website’s screenshots in the new look (2026-11-06)
- The top of the page and the tour's five steps show the app after the UI/UX redesign: ET Book,
  Courier Prime and Kalam, the sidebar, the welcome dialog with its two steps, the card's view
  with its table, the job in the status bar.
- Every mark of the tour moved to fit (they still pointed at the old tabs), checked in the page
  itself, in both themes. The bottom ones stay inside the frame, which clips the ring.
- Step 4's note and the line above the tour say "choose" rather than "tick", as albums are sent
  to the card from the library now; two screenshots' `alt` texts say what they show today.

### 103. Eject, tried on Windows (2026-11-08)
- Tried on a real USB stick, without administrator rights: two Syncs, then Eject, refused while a
  file was open and done in under a second once it wasn't; the drive stayed gone with the app
  watching for cards.
- A refused Eject said "eject again" and took the button away. It stays now, on the notice that
  says why.
- On Windows, Eject is offered only for removable drives (`platform::ejectable`): it was offered
  for internal ones too, when a card setup wrote to a folder there. USB hard disks count as fixed,
  as a rule, so they don't get it either.
- While other work runs the button is greyed, and its hover says until when; a click did nothing
  and said nothing.
- Two messages said "not on this Mac" on every system: "not on this computer".
- [docs/CARD_STATES.md](docs/CARD_STATES.md): the card as state machines, what each operation
  checks first, and the paths the code doesn't handle yet.
