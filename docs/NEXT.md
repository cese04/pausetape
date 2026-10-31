# Next changes (agreed and in progress)

*Written 2026-10-28.* Three related problems, found on a real library, and how to fix them.
Each part says what happens today, why, and what to change. Part D gives the order of work, what
is done, and the tests.

**Done (2026-10-28):** steps 1–2 of part D (A1–A4, B1, B2), the folder walk-up from B3, and
part C (steps 5–7: CUE sheets in the library, playback and on the card), and B5 (step 8:
editions and same-name albums). The "What happens
today" sections below describe the behaviour *before* these steps; part C ends with what was
built and measured.

**Added 2026-10-30:** part E, other players (device profiles, as calibre has). E1 and E2 are done.

Related: [PROGRESS](PROGRESS.md) (status and roadmap) · [ARCHITECTURE](ARCHITECTURE.md) (where
the code lives).

| Part | Problem | Where in the code |
|---|---|---|
| A | MusicBrainz suggestions come from the wrong album, often by another artist | `src/musicbrainz.rs`, `src/app/music.rs` (`lookup_album`), `src/ui/library.rs` |
| B | Folder and file names are misread ("1979" becomes an artist) | `src/library/scan.rs` (`infer_from_path`) |
| C | Albums stored as one FLAC plus a `.cue` sheet appear as one long song | new `src/library/cue.rs`; `scan.rs`, `track.rs`, `db.rs`, `audio/`, `card/` |
| E | Only one player is known: everything assumes the Echo Mini running RECHO *(added 2026-10-30)* | new `src/card/device.rs`; `card/plan.rs`, `library/track.rs`, `ui/` |

---

## A. MusicBrainz lookups pick the wrong release

### What happens today

*look up* on an album sends **one search** with only the album and artist names (song titles
are not sent):

```
GET https://musicbrainz.org/ws/2/release?fmt=json&limit=10
    &query=release:"<album>" AND artist:"<artist>"
```

- `<album>` is the album tag, or the folder name if there's no tag.
- `<artist>` is the artist's name as shown in the library, or the literal text `Unknown Artist`
  when the files have no artist.

The app then picks one result:

1. It keeps results with a score of 80 or more.
2. It sorts them by **closeness of track count to the number of local files**, then by
   "Official", and only then by score.
3. It fetches the winner's track list (a second request).

Each local file is then matched to a track of that release locally:

1. by similar title (Jaro-Winkler ≥ 0.88 on `title_key`);
2. else by **disc and track number**;
3. else by length (within 2 s).

Every match becomes a suggested change.

### Why it goes wrong

These live queries ran on 2026-10-28:

| Query | Results |
|---|---|
| `release:"Love" AND artist:"The Beatles"` | All *Love* by The Beatles: the search itself works when tags are good |
| `release:"Love" AND artist:"Unknown Artist"` | "Faithless Love", "No Good / Your Love", "Im Sorry, I love You EP"… at scores 88–100 |
| `release:"The Beatles - Love (2006) [Dolby Atmos]" AND artist:"The Beatles"` | Nothing |

1. **Untagged artist.** `Unknown Artist` is sent as a real name. MusicBrainz has thousands of
   releases credited to it.
2. **A quoted phrase only has to appear somewhere** in the title. "Love" matches "Faithless
   Love" with a top score.
3. **Track count outranks everything.** With 4 local files, a 4-track EP of a different name
   beats the right 26-track album.
4. **The number fallback hides the mistake.** If no titles match (because it's the wrong
   album), files are matched by track number anyway. File 3 receives the title of track 3 of the
   wrong album, and every song gets a confident-looking wrong title.
5. **Band names inside song titles.** "The Beatles - Something", or a title taken from a
   filename like "01 The Beatles - Something", is not similar enough to "Something". Even the
   correct album then falls back to track numbers.
6. **Raw folder names are sent unchanged.** Year, edition, label and `[Dolby Atmos]` make the
   phrase match nothing.

### What to change

**A1. Clean what is sent and compared.** Add one shared cleaner (also used by part B) in
`library/names.rs`:

- `clean_album(s)`:
  - strip a leading year (`1979 - `, `1979. `, `(1979) `);
  - strip bracketed extras `(…)` `[…]` `{…}` holding years, formats, labels, catalogue numbers or
    "Remaster", "Deluxe", "Dolby Atmos", "Japan Edition", "v.1";
  - strip a leading `<artist> - ` when it matches the album artist's key.
- `clean_title(s, artist)`:
  - strip a leading track number (`01 `, `01. `, `1-01 - `);
  - strip a leading `<artist> - ` when it matches the track or album artist's key.
- Keep the stripped parts: years feed `year`, and edition text feeds the edition label (B3).

**A2. Never search for a placeholder artist.** If the artist is missing or is `Unknown Artist`
(`Various Artists` is kept: MusicBrainz credits compilations to an artist of that name):

- Search by **release title alone** and rely on A3 to check the result.
- Or, for loose tracks, run a recording search by title plus **duration**:
  `recording:"<title>" AND dur:[<ms−3000> TO <ms+3000>]`.
- Never put `artist:"Unknown Artist"` in a query.

**A3. Check a candidate before trusting it.** Rank candidates by:

1. **Artist agrees:** `artist_key(credit)` equals the cluster key, or is "similar" under the same
   rules the artist merging uses (`names::similar_keys`). Skip this when A2 applies.
2. **Title agrees:** Jaro-Winkler of `title_key(clean_album(local))` vs
   `title_key(clean_album(remote))` ≥ 0.92, or one contains the other as whole words.
3. Then track count, then Official, then score.

Reject any candidate that fails 1 or 2. When nothing passes, say "no confident match" and
suggest nothing.

**A4. No blind fallback.** Use track-number and length matching only once the release is
confirmed. Confirmed means at least half of the local files matched by cleaned title, or every
local file's length is within 3 s of the track at that position. Otherwise reject the release
and try the next candidate (at most 3, to stay polite to the API at 1 request/second).

**A5. Show what was found and offer alternatives.** In the album's suggestion panel:

- First, the release found: **artist, title, year, country, label, track count**, and a link
  to it on musicbrainz.org.
- Next, a line *other releases:* with up to 3 more that passed A3. Choosing one fetches its
  tracks and replaces the suggestions.
- Then, per track, the suggested changes as today.
- Make tracks matched only by number or length visibly weaker (muted, "matched by position").
- Data needed: `Reply::Album` carries a `Vec<ReleaseSummary>` alongside the chosen release.

**A6 (later). Exact lookups when the disc layout is known.** A CUE sheet (part C) or an EAC
`.log`/`.toc` gives the CD's track offsets. From those, compute the **MusicBrainz disc ID** and
call `GET /ws/2/discid/<id>?inc=recordings+artist-credits`. That gives an exact release with no
name guessing. Already-split rips with a `.toc` file next to them qualify too.

**A7 (later). AcoustID fingerprinting** for files with no usable names at all (pending
suggestion 4 in PROGRESS).

---

## B. Folder and file names are misread

### Example

```
Joy Division - коллекция/
  1979. Unknown Pleasures {3 издания}/                       ← album with 3 editions
    1979. Unknown Pleasures {1986, UK, Factory Records, FACD 10}/      01 - Disorder.flac …
    1979. Unknown Pleasures {1989, US, Qwest Records, 9 25840-2} v.1/  1979 - Unknown Pleasures.flac + .cue
    1979. Unknown Pleasures {1989, US, Qwest Records, 9 25840-2} v.2/  Joy Division - Unknown Pleasures.flac + .cue
```

`--plan` on this folder today (2026-10-28):

```
1979/1979. Unknown Pleasures {1989, US, Qwest Records, 9 25840-2} v.1/Unknown Pleasures.flac
1980/1980. Closer {1989, France, Factory Records, 30216}/Closer.flac
1988/1988. Substance {1988, Brazil, Stiletto, 498.004}/Substance.flac
Joy Division/1979 - Unknown Pleasures/02 - Day of the Lords (2).flac      ← a whole album, named after one song
Joy Division/1985 - Still/02 - Ice Age.flac                               ← same
The Cure/Singles/The Cure - Disintegration.flac                           ← whole album as a "single"
Unknown Artist/Singles/Dream Theater.2005.Octavarium (Side 1).flac
```

### Why

1. **"1979 - Unknown Pleasures.flac" has no tags.** A leading number is treated as a track
   number only up to 3 digits, so `1979 - Unknown Pleasures` is split on " - " into **artist
   "1979"** and title "Unknown Pleasures".
2. **Only the `YYYY - Album` folder form is understood.** `1979. Album {…}` is kept whole as the
   album name.
3. **The artist is assumed to be the folder just above the album.** Here that's
   `1979. Unknown Pleasures {3 издания}` (an album folder). The real artist is two levels up,
   and that folder has a suffix (` - коллекция`, "collection").
4. **Editions of the same album merge.** Albums are grouped by artist + album name, so the
   1986 and 1989 editions become one album with duplicate track numbers (`02 … (2).flac`).
5. **Album images carry one track's tags.** The v.2 image and the *Still* image are tagged as a
   single song (title "Day of the Lords", track 02). Part C fixes this: the CUE sheet overrides
   them.

### What to change (in `infer_from_path`, using A1's cleaner)

**B1. Years are not artists.** In a filename, a leading 4-digit number from 1900 to 2099
followed by a separator is a **year**, not a track number or an artist. Set `year` if empty and
take the rest as the title, or as the album when the file is an album image (part C). The same
applies to `Dream Theater.2005.Octavarium`: `Artist.YYYY.Album` splits into artist, year and
album.

**B2. Understand common album-folder forms.** `YYYY - Album`, `YYYY. Album`, `(YYYY) Album`,
`Album (YYYY)`, `Artist - Album (YYYY) [extras]`, `Artist - 'Album' [YYYY, …]`. Extract the
year and the clean album name; keep the bracketed extras for B3.

**B3. Walk up to the artist folder.**

1. Classify each folder between the root and the file:
   - **album-like:** starts or ends with a year, contains `CD1`/`Disc 2`, or matches the album
     name;
   - **edition-like:** a child of an album-like folder whose name repeats it with extras;
   - **disc folder:** `CD 1`, `Disc 2`, `CD2 - Chet Plays` (also sets `disc`);
   - **neutral:** anything else.
2. The artist is the nearest folder above that isn't album-like, edition-like or a disc folder.
3. Strip collection suffixes from it: ` - коллекция`, ` - дискография`, ` - discography`,
   ` - collection`, ` - complete`, ` [restored]`, ` FLAC`.
4. Tags still win over all of this. This only fills gaps.

**B4. Learn from neighbours.** When some files in an artist folder are tagged (e.g. the 1986
edition says "Joy Division"), untagged files in the same folder take that artist instead of a
folder-derived guess. The artist folder's name, stripped, must match that tag's key.

**B5. Keep editions apart.** When two folders yield the same artist + album **and both have a
track 1**, treat them as editions:

- **Library:** group them under the album, labelled by their extras ("1986, UK, Factory
  Records" / "1989, US, Qwest Records · v.1").
- **Writes:** choose which edition goes to the card (default: the one with the most tracks, then
  the oldest folder). The others are excluded rather than merged into `(2)` duplicates.
- **Data:** `AlbumNode` gets `editions: Vec<Edition { label, folder, tracks }>`. The album ID
  stays `cluster|title`, and the chosen edition is stored per write in `Custom`.
- **Same name, different years:** American Football's three self-titled albums (1999, 2016,
  2019) all carry the album tag "American Football" and merge into one today. Same artist +
  album name with **different years** should be different albums, not editions.

**What was built for B5 (2026-10-28)**, differing from the sketch above where testing on the
real library showed a better rule:

- **Copies.** Tracks sharing an album name are split by album folder (a `CD 1` folder counts as
  the folder above it). Folders that complete each other, with no shared track positions or
  titles (discs kept in separate folders), form one copy.
- **Editions or different albums.** Copies sharing at least half their song titles are
  *editions* of one album. Copies sharing few are *different albums with the same name*.
  Titles are a better signal than "both have a track 1" or years, which differ between a
  release and its reissue.
- **Data:** every edition is its own `AlbumNode` with an `Edition { key, label, rank, count }`
  (one row each, so lookups, play, names and order work per edition). The label is the
  folder's edition details ("1986, UK, Factory Records, FACD 10"). Same-name albums get a
  `variant` (their year).
  - IDs: `cluster|title` for unique names (unchanged), `cluster|title|2016` for same-name
    albums, `…#<folder key>` per edition.
- **Writes:** one edition per album goes on the card. The default is the one with the most
  lossless songs RECHO plays, then the most songs RECHO plays, then the most songs, then one
  without a CUE cut, then folder order: so a FLAC vinyl rip beats an MP3 copy with bonus tracks.
  Ticking another edition in the card tab chooses it (`Custom::editions`). Exclusions and card
  names saved under the old album ID still apply.
- **Card folders:** all editions share one folder name (only one is written). Same-name albums
  are told apart by the year prefix, or "Album (2016)" when albums aren't ordered by year.
- **Numbering:** custom album numbering gives all editions one number.
- **Reorganise** moves only the default edition; other editions stay where they are.
- **Measured on the test library:** besides Joy Division (*Unknown Pleasures* ×3, *Closer* ×2)
  and American Football (3 albums instead of one with 26 songs), it found duplicate copies of
  five Between The Buried And Me albums, *Images And Words*, *Rust In Peace* (FLAC vinyl rip
  vs MP3 320) and *The Dark Side of the Moon* (original vs 50th-anniversary remaster), all of
  which went onto the card twice before. A full write is about 1.5 GB smaller.
- **Copies that could go (2026-10-28).**
  - A copy is *covered* when another edition has all its songs, at least as much music (not
    over 30 s or 3% shorter), and the same or higher quality.
  - Quality: lossless over lossy; then bit depth and sample rate; lossy copies by median
    bitrate, within 10% counting as the same.
  - Songs are compared by title without bracketed notes or anything after " - " or " / ", with
    small spelling slips tolerated ("The Need For Repitition").
  - The library says so under the copy ("Edition 1 has all these songs in the same quality
    (MP3 320); this copy could go (130 MB)"). The header counts covered copies and their size,
    and links to a view of just those albums. `--plan` lists every album copy and its status.
  - Nothing is deleted by the app. Other cases (bonus songs, a hidden track, different
    quality trade-offs) are left alone and chosen per write.
  - On the test library: 11 copies, 1.9 GB.
  - *The Silent Circus* stays unflagged: its second copy runs 8 minutes longer (a hidden
    track).
- **Not caught:** editions tagged with different album names (*Substance* vs *Substance
  (1977-1980)*). Grouping by the cleaned name would catch them, but would also risk treating
  "Live (1998)" and "Live (2004)" as editions when they share songs; left for a rule that also
  compares years or lengths.

---

## C. Albums stored as one file plus a CUE sheet

### What players like foobar2000 and AIMP do

A `.cue` sheet next to an audio file lists the tracks inside it: title, performer and start time
(`INDEX 01 mm:ss:ff`, 75 frames per second). The player shows each as its own song and plays
from that start to the next track's start.

### What's in this library (surveyed 2026-10-28)

- **86 `.cue` sheets** on `/Volumes/Elements/Music`.
- **9 album files hold several songs** (The Cure *Disintegration*, Cigarettes After Sex, Joy
  Division ×6, Dream Theater *Octavarium*'s sides). The rest describe already-split files;
  those are only useful for A6. (A first count said 23: it counted `TRACK` lines, which in
  per-track rips can come before their own file's `FILE` line. That's the trap C1 describes.)
- **One sheet covers several files:** *Octavarium* has 4 files (vinyl sides), each holding 1–3
  tracks.
- **25 aren't UTF-8.** They're Windows-1251 (Cyrillic) or Windows-1252.
- **File names don't always match.** A sheet may name `….FLAC` for a `….flac` file, or `.wav`
  for files later converted to `.flac`.
- **Some album images carry one track's tags**, which are wrong for the whole file (B5 above).

### What to change

**C1. Parse CUE sheets** (new `library/cue.rs`, no egui, unit-tested):

- **Encoding:**
  - UTF-8 with or without a BOM;
  - else decode with `encoding_rs`, choosing Windows-1251 when Cyrillic letters result, else
    Windows-1252. `chardetng` can do the guessing.
- **Commands:**
  - Album level: `PERFORMER`, `TITLE`, `REM DATE`, `REM GENRE`, `REM DISCID`, `REM DISCNUMBER`.
  - Per `FILE`: its `TRACK nn AUDIO` entries with `TITLE`, `PERFORMER`, `INDEX 01`. Ignore
    `INDEX 00`: pregaps stay with the previous track, as foobar2000 does.
  - Accept CRLF and quoted or unquoted values.
- **Resolving `FILE`:**
  1. the exact name;
  2. else a case-insensitive match (Linux);
  3. else the same stem with any audio extension (`.wav` → `.flac`).
- **Which files to split:** count `INDEX 01` **per file**, not `TRACK` lines. In per-track
  sheets a `TRACK` line can appear before its `FILE` line.
  - Split only files with **two or more** `INDEX 01` entries.
  - Files with one keep their own tags; the sheet can still supply missing ones.
- **Several sheets in one folder:** use the one that resolves the most files. Ignore sheets
  whose files are missing.

**C2. Virtual tracks in the library.**

- `Track` gets `span: Option<Span { start: Duration, end: Option<Duration>, cue_track: u32 }>`.
  The last track's `end` is the file's end. The image file itself is replaced by its virtual
  tracks.
- Tags come from the sheet:
  - track `TITLE` and number;
  - track `PERFORMER` (else the sheet's) as artist;
  - sheet `PERFORMER` as album artist;
  - sheet `TITLE` as album, cleaned with A1;
  - `REM DATE` as year;
  - multi-sheet folders (`CD1`/`CD2`) as disc numbers.
  - The image's own embedded tags fill only what the sheet lacks. They never override it,
    because they're often a single track's tags.
- Capitals: an all-caps `PERFORMER "JOY DIVISION"` already merges with "Joy Division"
  (`artist_key` ignores case). Extend `spelling_quality` to prefer mixed case over all caps.
- **Database:** key rows by `(path, cue_track)` with `cue_track = 0` for normal files. Store the
  sheet's path, size and mtime so editing the sheet triggers a re-read. Bump `SCHEMA_VERSION`.
- **Identity:** anything that stores a track path (playlists, manifests) needs the span too.
  Serialise it as `path#3`; this needs a small migration for playlists.
- **Formats:** a FLAC image plays on RECHO only once split (C4). Mark virtual tracks with a
  badge, e.g. "from CUE".

**C3. Playback.**

- In `audio/player.rs`, open the file, seek to `start` and stop at `end` with rodio's
  `skip_duration` / `take_duration`, or Symphonia seek plus a sample counter for accuracy.
- The seek bar, time and waveform (`envelope`) cover only the span: compute the envelope for
  the span, or once per image and then slice it.
- Optional: when the next queued track is the next span of the same file, keep decoding without
  reopening, for gapless live albums.
- ffmpeg-converted sources (C2 + Dolby, etc.) pass `-ss`/`-to` to the conversion.

**C4. Writing to the card.** RECHO has no known CUE support, so the card gets **one file per
track**:

- **FLAC images → per-track FLAC**, split with ffmpeg:
  `ffmpeg -ss <start> -to <end> -i image.flac -map 0:a -c:a flac -map_metadata -1 out.flac`,
  then write tags with `library::write_tags`.
  - FLAC → FLAC is lossless; the decoded audio is identical.
  - Splitting at the right point needs the decode-side `-ss`, so put it after `-i`, or verify
    with a test that the split points land on the CD frames.
  - Splitting in-process is possible later: Symphonia decode plus a FLAC encoder crate.
- **WAV images** split to WAV the same way.
- **APE/WavPack images** need conversion to FLAC anyway (pending suggestion 1).
- **Manifest:** a split file's size can't be compared to its source. Store a source key per
  written file: `image path + size + mtime + cue track + span`. A rewrite then skips
  unchanged splits and prunes only files whose size still matches the manifest.
- **Missing ffmpeg:** without it (Windows/Linux until bundled), split tracks are listed as
  "needs ffmpeg" and skipped, like other formats that need conversion. The alternative of
  copying the whole image as one long file is offered per write.
- **Card names:** `Artist/1979 - Unknown Pleasures/01 - Disorder.flac`, same as split albums.

**C5 (later).** Some FLAC files carry the sheet *inside* them: a `CUESHEET` metadata block or a
`CUESHEET` Vorbis comment. Treat them like an external sheet.

### What was built (2026-10-28)

- **Parser** (`library/cue.rs`): encodings via `encoding_rs` + `chardetng`; songs assigned to
  the file current at `INDEX 01`; `FILE` names resolved exactly, ignoring case, or by stem
  with another audio extension.
- **Library:**
  - A file holding 2+ songs becomes one track per song; a file holding one song keeps its tags,
    and the sheet fills the gaps (*Octavarium* side 4 became "08 - Octavarium").
  - Sheets are read on every scan (milliseconds), so the database is unchanged: it caches the
    album file's own tags, and the songs are derived from it.
  - Track identity is `Track::id()`: the path, plus `#03` for a song inside an album file. Used
    for playlists, corrections, the waveform and the `--plan` output.
  - All-caps spellings from sheets ("JOY DIVISION") lose ties to mixed case.
  - The album's folder line says "one file + CUE sheet".
- **Playback:** `audio::Section` seeks the album file to the song's first sample frame and
  stops after its last. Times, seeking and the waveform are relative to the song. Checked
  against ffmpeg's cut of *Unknown Pleasures* track 2: the same 25,321,632 samples, difference 0.
- **Card:**
  - Songs are cut to FLAC with ffmpeg (`-ss` before `-i`: sample-exact, checked against the
    whole decoded album), then tagged.
  - Lossless album files of any format (FLAC, WAV, APE, WavPack, AIFF, ALAC) become FLAC. Songs
    in lossy album files, or without ffmpeg, are left out with a reason.
  - The manifest records what each song was cut from (`Written::source`), so a rewrite cuts
    nothing unless the album file, the sheet's times or the tags changed. *Unknown Pleasures*:
    10 songs in 5.4 s; a second write cut 0.
- **Not moved:** *Reorganise* leaves album files and their sheets where they are.
- **Corrections stay in the app:** they can't be written back into the album file; they reach
  the card copies.
- **Bundled ffmpeg** now includes the FLAC encoder, the FLAC writer and `atrim` (8.4 MB).
- **Left for later:**
  - Gapless playback between songs of the same file.
  - Hidden audio before track 1's `INDEX 01` (skipped, as foobar2000 does).
  - Sheets embedded in FLAC files (C5).
  - Disc ID lookups (A6).
  - Editions (B5): done since, see B5 above.

---

## D. Order of work and tests

| Step | Work | Effort | Depends on | Status |
|---|---|---|---|---|
| 1 | A1 cleaners + B1/B2 filename and folder forms | S | | **Done** |
| 2 | A2–A4 safer lookup (no placeholder artist, checked candidates, no blind fallback) | M | 1 | **Done** |
| 3 | A5 show the release found + alternatives | M | 2 | Partly: the found release's country, label and track count are shown |
| 4 | B3/B4 artist folder walk-up and learning from neighbours | M | 1 | B3 walk-up **done**; B4 to do |
| 5 | C1 CUE parser + C2 virtual tracks in scan/db/library | M | 1 | **Done** |
| 6 | C3 playback of spans | M | 5 | **Done** (gapless joins not yet) |
| 7 | C4 splitting on the card + manifest source keys | M | 5 | **Done** |
| 8 | B5 editions | M | 4 | **Done** (differently named editions not yet) |
| 9 | A6 disc ID lookups, C5 embedded sheets | M | 5 | |

**Tests to add** (offline, small fixtures in temporary folders, as the existing tests do):

- **Names:**
  - `infer_from_path` on the Joy Division tree above gives artist "Joy Division", album
    "Unknown Pleasures", year 1979 and three editions.
  - `Dream Theater.2005.Octavarium (Side 1)` gives artist, year and album.
  - `01 The Beatles - Something` gives title "Something".
- **Cleaners:** `clean_album` / `clean_title` on the folder names listed in this document.
- **CUE parser:**
  - the v.1 sheet (one file, 10 tracks);
  - a per-track sheet whose `TRACK` lines precede `FILE` lines (no split);
  - *Octavarium* (4 files, split per file);
  - a Windows-1251 sheet;
  - `.FLAC` vs `.flac` and `.wav` → `.flac` resolution.
- **Spans:** start and end times from `INDEX 01` (frames → seconds); the last track ends at the
  file's end.
- **Lookup ranking (pure functions over saved JSON):** with recorded MusicBrainz responses for
  `Love`/`Unknown Artist`, the wrong releases are rejected. A local album whose titles don't
  match yields no suggestions rather than position matches.
- **Card:** a fake image split into 3 files by a stub splitter; a rewrite skips them; deselecting
  prunes them.
- Keep the live checks as ignored tests (`musicbrainz`, a new `cue_library` over
  `NEATLIBRARY_FORMATS`).

---

## E. Other players: device profiles, as calibre has

*Added 2026-10-30.* calibre keeps the library apart from the reader: each device has a driver
that says what it reads, and "send to device" converts to suit it. NeatLibrary hard-codes one
player. "RECHO" is in the format check (`Track::on_recho`, `RECHO_FORMATS`), in how editions are
ranked, in the folder naming, in how playlists are written, and in dozens of on-screen strings.

The Echo Mini's own (stock) firmware is the natural second player, because it differs from
RECHO in nearly every way that matters here.

### What's known of the two firmwares

| | RECHO | Stock firmware |
|---|---|---|
| Browsing | folders, sorted by name | a library built from tags (artist, album), plus folders |
| Formats | FLAC, MP3, WAV (up to 24-bit/192 kHz; MP3 up to 320 kbps) | FLAC, MP3, WAV, APE, AAC in M4A, Ogg Vorbis, WMA; DSD (up to DSD256) |
| ALAC, Opus, AIFF, WavPack | no | not listed (ALAC inside M4A is unclear) |
| Playlists | none for the user (`.m3u` only internally) | none, per most reviews (one says "playlist navigation") |
| Covers | **none** | shown on the play screen |
| Limits | none stated | 8,192 songs; cards up to 256 GB; no NTFS; no gapless |
| SD card | read-only | read |

Sources: [RECHO's site](https://recho.atzkey.workers.dev/) for RECHO. For stock: reviews at
[Headfonics](https://headfonics.com/?p=75981),
[audioreviews.org](https://www.audioreviews.org/fiio-snowsky-echo-mini-review/),
[Mobile Audiophile](https://mobileaudiophile.com/daps/fiio-snowsky-echo-review/) and the
[Turbovs spec sheet](https://www.turbovs.com/review/fiio-snowsky-echo-mini-50291). The stock
column is unchecked on a player, and reviews disagree on playlists and on APE's top rate.

**One correction to the app as it stands:** many strings say "the Echo Mini shows embedded
covers". That's true of the stock firmware, but RECHO's site says it shows no cover art. Covers
still matter: for this app's player, the stock firmware, and most other players.

### What a profile changes

- **What plays, so what's converted.** On stock, AAC, Ogg, APE and WMA go to the card as they
  are; on RECHO they're converted.
- **Naming.** The year prefixes ("1997 - OK Computer") exist because RECHO sorts folders by
  name. A tag-browsing player ignores folder names, so album artist, track and disc numbers and
  the compilation flag decide how the music looks there; "write corrected tags" matters more.
- **Playlists.** RECHO's playlists are folders of numbered copies. A player that builds its
  library from tags would list each copy as a second song under its artist, so on stock the
  playlist copies come with a warning.
- **Limits.** Stock lists at most 8,192 songs, so what fits is counted in songs as well as bytes.
- **Covers.** Whether a missing cover shows on the player.

Edition ranking stays the same for every player. The edition a write picks first is the most
complete one in FLAC, MP3 or WAV (`Track::plays_anywhere`), the formats nearly every player
reads.

### Steps

| # | Step | Size | Status |
|---|---|---|---|
| E1 | **Profiles.** A `Device` on each card setup (`Options::device`), carried into `Rules`, so `target()` asks "does this player play it?". Built in: *Echo Mini · RECHO* (the default, so saved setups keep working) and *Echo Mini · stock firmware*. Chosen in the card tab's **player** row, next to the destination. Format warnings, badges, the scan notice and the playlist note name the chosen player; the card warns above its song limit. | M | **Done** (2026-10-30) |
| E2 | **A test card.** Write a folder of short samples (every format, FLAC at 96, 192 and 384 kHz, a cover, accented titles in ID3v2.3 and 2.4, a playlist) to the card; play through it on the player and tick what worked. The answers are kept with the card setup, per player, and win over the built-in profile: formats, the highest sample rate (songs above it go at CD quality) and covers. | M | **Done** (2026-10-31) |
| E2b | **Act on the other answers.** A playlist that shows → write playlists as `.m3u8` instead of numbered copies (no duplicate songs on a tag-browsing player). ID3v2.4 titles garbled but v2.3 fine → write ID3v2.3 into card copies. Both are recorded now, but don't change anything yet. | S | **On hold** (2026-10-31): until the player's developers confirm that playlists play |
| E3 | **Recognising a card.** Write a small hidden file (`.neatlibrary`, like calibre's `metadata.calibre`) naming the card setup it belongs to, so a card plugged in brings up its own setup and player. RECHO only reads the card, so the file is harmless there. | S | **Done** (2026-10-31) |
| E4 | **A side panel of facets** (format, quality, year, card status, genre) with counts, each a click to filter. The search terms behind it exist already (`library/search.rs`); genre needs reading from tags. | M | |
| E5 | **An editable naming pattern**, like calibre's save-to-disk template: `{albumartist}/{year} - {album}/{disc}-{track} - {title}`, with a default per profile. | M | |

**Tests for E1:** each player's format list (FLAC everywhere, AAC on stock only, ALAC on
neither); `target()` copies AAC for stock and converts it for RECHO; saved settings without a
player load as RECHO.

**Open questions, for E2 to settle:** ALAC in M4A on stock; whether stock reads `.m3u8`; tag
versions (ID3v2.3 or 2.4, and whether it reads Vorbis comments in Ogg); bit-depth and
sample-rate ceilings; whether stock's library counts songs on the internal storage towards the
8,192.
