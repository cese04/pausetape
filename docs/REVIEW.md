# Code review: bugs and risks found (2026-10-31)

A read through the whole code base (about 15,000 lines) after part E of [NEXT](NEXT.md), looking
for bugs, and for what will behave differently on Windows and Linux. Each finding says when it
was fixed; all are (2026-10-31).

Related: [TESTING](TESTING.md) (the Windows and Linux test guide; §3 lists what testers will
see) · [DISTRIBUTION §1](DISTRIBUTION.md#1-code-changes-needed-first) (portability fixes known
before this review, not repeated here) · [ARCHITECTURE](ARCHITECTURE.md).

**How sure:** R1 and R2 were reproduced with throwaway tests (since removed). The rest come from
reading the code; each says what would happen and where.

| | Severity | Finding | Systems |
|---|---|---|---|
| [R1](#r1) | **High** | Starting the app with the music drive unplugged empties the library database, unsaved corrections included (**fixed**) | All |
| [R2](#r2) | **High** | A Sync with an empty library removes the setup's songs from the card (**fixed**) | All |
| [R3](#r3) | Medium | MusicBrainz suggestions that arrive after a rescan change the wrong songs (**fixed**) | All |
| [R4](#r4) | Medium | Renaming a card setup while it syncs or takes over the card loses what it did (**fixed**) | All |
| [R5](#r5) | Medium | Choosing another music folder while something runs leaves the library empty, and can crash (**fixed**) | All |
| [R6](#r6) | Medium | Undoing a take-over fails once the card is mounted somewhere else (**fixed**) | All |
| [R7](#r7) | Medium | For up to 3 seconds, Sync can write over another setup's card (**fixed**) | All |
| [R8](#r8) | Medium | Damaged audio files join the library as ordinary songs (**fixed**) | All |
| [R9](#r9) | Medium | Songs with unsaved corrections are copied again at every Sync (**fixed**) | All |
| [R10](#r10) | Medium | "Moved to the Trash" may really delete, on some drives (**fixed**) | Windows, Linux |
| [R11](#r11) | Medium | Playback stops for good when the audio device changes (**fixed**) | All |
| [R12](#r12) | Medium | Every ffmpeg run flashes a console window (**fixed**) | Windows |
| [R25](#r25) | Medium | Playback can stutter while the music drive is busy (**fixed**) | All |
| [R13](#r13) | Medium | Plugging in a card doesn't bring up its setup (**fixed**) | Windows |
| [R14](#r14) | Low | Writing tags can't clear a field, shortens dates to the year, and leaves old secondary tags (**fixed**) | All |
| [R15](#r15) | Low | Undoing a tidy can overwrite a file (**fixed**) | macOS, Linux |
| [R16](#r16) | Low | A rescan while music plays leaves the player bar without the song (**fixed**) | All |
| [R17](#r17) | Low | Why a song was skipped is often lost (**fixed**) | All |
| [R18](#r18) | Low | Playing album files needs up to ~4 GB of temporary space (**fixed**) | All |
| [R19](#r19) | Low | "Dropping DeviceSink…" is printed on quitting (**fixed**) | All |
| [R20](#r20) | Low | Folders linked into the music folder are skipped (**fixed**) | Linux, macOS |
| [R21](#r21) | Low | DSD files aren't scanned, though the stock firmware plays them (**fixed**) | All |
| [R22](#r22) | Low | A song folder named like the playlists folder confuses Sync (**fixed**) | All |
| [R23](#r23) | Low | The card's `.neatlibrary` label shows in Explorer (**fixed**) | Windows |
| [R24](#r24) | Low | The card tab recomputes sizes on every frame (**fixed**) | All |
| [R26](#r26) | Low | ffmpeg's errors during playback go to the terminal, not the app (**fixed**) | All |

R1 and R2 lost data in a situation that's easy to get into (an external music drive, as here);
both are fixed (2026-10-31).

---

## High

### R1
**Starting the app with the music drive unplugged empties the library database.**
[scan.rs:62](../src/library/scan.rs#L62), [scan.rs:101](../src/library/scan.rs#L101)

When the music folder isn't there, the walk finds no files, so every song in the database counts
as "gone since last time" and is deleted, **with its unsaved corrections** (MusicBrainz fixes,
hand edits, covers waiting to be embedded). Plugging the drive back in rescans everything from
the files, and the corrections are lost for good. Reproduced: a library of one song with one
correction scanned with its folder renamed away ended with `(0 tracks, 0 corrections)` in the
database.

**Fixed 2026-10-31.** A music folder that can't be read, or that holds no music where the
database knows some (an empty mount point), now changes nothing: the library keeps what it had,
says "Can't reach the music folder: is its drive connected?", and reads it again by itself as
soon as it's back. A folder inside that can't be read keeps the songs known there. Tested in
`rescans_reuse_unchanged_files_and_keep_corrections`.

### R2
**A Sync with an empty library removes the setup's songs from the card.**
[writes.rs:242](../src/app/writes.rs#L242), [write.rs](../src/card/write.rs) (`prune`)

With the library empty (R1, or a scan that failed), the plan is empty, so a Sync removes every
song the setup put on the card: removal only looks at the setup's own record of the card, and
none of those files are wanted any more. The card tab still offers **Sync** whenever the card is
connected. Reproduced: a card written with one song, synced again with an empty library, lost it.

**Fixed 2026-10-31.** Sync, taking over or comparing the card's music, copying from the card or
from another folder, and tidying all wait while the music folder can't be reached or the library
is empty, and the card tab says why in place of **Sync**. A Sync that would take at least 20
songs and most of the setup's songs off the card asks first ("Take 4100 of 4412 songs off the
card?"); songs it only renames don't count. Tested in `a_new_pattern_renames_on_the_card`.

## Medium

### R3
**MusicBrainz suggestions that arrive after a rescan change the wrong songs.**
[music.rs:367](../src/app/music.rs#L367)

A suggestion names songs by their position in the library's list (`FieldChange::track`). A
rescan (after adding music, moving copies to the Trash, tidying…) rebuilds that list. Lookups
waiting at that moment still arrive, and accepting one edits whichever songs now hold those
positions, or crashes if the list got shorter. The edits are saved as corrections and reach the
card and, with **write corrected tags**, the files.

**Fixed 2026-10-31.** A lookup names its songs by their place in the request, and remembers their
ids; the reply is turned into suggestions for the library's songs as they are now, found by id
(songs gone since lose theirs). A reply for a lookup a rescan dropped is ignored. Tested in
`suggestions_follow_their_songs_through_a_rescan`. Other work holding positions across a rescan
is R5.

### R4
**Renaming a card setup while it syncs or takes over the card loses what it did.**
[events.rs:77](../src/app/events.rs#L77) (and the `Previewed`, `Surveyed`, `TookOver` handlers);
the name field at [card.rs:243](../src/ui/card.rs#L243)

Results of background card work find their setup by name, and the name stays editable while the
work runs. Renamed in the meantime:
- a finished Sync's record of the card is dropped. The setup no longer knows its files: songs it
  later deselects are never removed, and converted songs are converted again;
- a take-over's moves aren't recorded, and its undo record is filed under the old name, so
  **undo taking over the card** disappears.

**Fixed 2026-10-31.** Results of card work (Sync, Compare, checking the card, take-over and its
undo) find their setup by `WriteProfile::id`, which never changes. New take-over records are
filed under the id; older ones, under the name, are still found.

### R5
**Choosing another music folder while something runs leaves the library empty, and can crash.**
[music.rs:36](../src/app/music.rs#L36)

`set_source` empties the library and starts a scan, but scans don't start while other work runs,
so the library stays empty until a manual rescan. Work that finishes afterwards (saving tags,
finding covers) then updates songs by position in a list that's now empty, which crashes the app.

**Fixed 2026-10-31.** The music folder can't be changed while something runs ("the music folder
can be changed once … is done"); nothing else replaces the library while work is running, since
scans wait for it too.

### R6
**Undoing a take-over fails once the card is mounted somewhere else.**
[adopt.rs:445](../src/card/adopt.rs#L445)

The undo record keeps full paths (`/Volumes/Untitled/…`). If the card comes back as
`/Volumes/Untitled 1`, or as another drive letter on Windows, every move fails ("no longer
there") and the setup's record isn't corrected. Card labels (E3) now follow a card to its new
mount point, which makes this more likely.

**Fixed 2026-10-31.** New records keep paths relative to the card (and are filed under the
setup's id, R4); older records with full paths are read as before. Tested by undoing with the
card moved to another folder.

### R7
**For up to 3 seconds, Sync can write over another setup's card.**
[cards.rs:92](../src/app/cards.rs#L92)

The "this card belongs to another setup" check uses the label as last read, which is refreshed
every 3 seconds. Choosing another setup's card in the card dialog and pressing **Sync** at once
isn't refused, and the write relabels the card as this setup's.

**Fixed 2026-10-31.** The label is read again right before a Sync, a take-over or a check of the
card's music, and as soon as a card is chosen in the card dialog.

### R8
**Damaged audio files join the library as ordinary songs.**
[scan.rs:153](../src/library/scan.rs#L153)

When a file's tags and audio header can't be read, it's still listed, with no length, and
nothing says so. It then fails when played ("skipped 1 file NeatLibrary can't play") and at every
Sync ("ffmpeg couldn't convert the song: Invalid data found"). This is what happened with 24 of
the 25 Emerson, Lake & Palmer `.ape` album files under `From the card/ELP`: they are 92–100% zero
bytes, already damaged on the old card they were imported from.

**Fixed 2026-10-31.** A file whose header can't be read and that starts blank (4 KB of zeros, or
nothing at all) is marked damaged when scanned. The library shows "N damaged files" (a click
lists their albums, as the new search term `damaged:yes` does); writes leave them out with that
reason instead of an ffmpeg error at every Sync; playback steps over them without ffmpeg. Files
that are merely odd (unreadable, not blank) aren't called damaged. The library database's version
went up, so every file is read once more at the next start. On this library: 24 of the 25 ELP
files, and only those.

### R9
**Songs with unsaved corrections are copied again at every Sync.**
[copy.rs:222](../src/card/copy.rs#L222) (`up_to_date` returns false for corrected songs)

A corrected song's card copy gets the corrected tags, but the next Sync can't tell it's already
current, so it copies and retags it again, every time, until the corrections are written into
the music files.

**Fixed 2026-10-31.** A corrected song's card copy is recorded with a key of the source and the
corrected tags, as converted songs are; it's copied again only when the corrections change.
Tested in `corrected_songs_are_copied_once`. One edge left: turning **write corrected tags**
on afterwards doesn't retag copies already made without them.

### R10
**"Moved to the Trash" may really delete, on some drives.**
[platform.rs:26](../src/platform.rs#L26)

Moving duplicate songs and covered album copies uses the system Trash. Windows has no Recycle Bin
on removable USB drives and SD cards (files are deleted outright), and on Linux, drives without a
`.Trash-<uid>` folder may refuse or delete. The message still says "moved to the Trash".

**Fixed 2026-10-31.** On Windows, files on removable and network drives (which have no Recycle
Bin) are left where they are, and the app says why. On Linux, the `trash` crate puts files in a
`.Trash-<uid>` folder at the top of the drive, creating it if it can, and fails rather than
deleting when it can't; still to be seen on a real machine (TESTING §3 #12).

### R11
**Playback stops for good when the audio device changes.**
[player.rs:104](../src/audio/player.rs#L104)

The audio output is opened once, at start-up. Unplugging headphones from a USB DAC, or Bluetooth
headphones disconnecting, leaves the app playing into nothing until it's restarted.

**Fixed 2026-10-31.** The audio library (cpal 0.18) already follows a change of the default
output on macOS, Windows and PipeWire. What was missing: when the output fails for good (the
device gone with nothing taking over), the app now opens the default output again, every 2
seconds until there is one, and carries on with the song where it was (paused if it was).
Rerouting notices and glitches no longer reach the terminal.

### R12
**Every ffmpeg run flashes a console window.** (Windows)
[decode.rs:209](../src/audio/decode.rs#L209), [copy.rs:115](../src/card/copy.rs#L115),
[testcard.rs:139](../src/card/testcard.rs#L139), [decode.rs:40](../src/audio/decode.rs#L40)

Once the app runs without its own console (DISTRIBUTION §1 #4), each ffmpeg it starts opens a
console window for as long as it runs: one per converted song on the card.

**Fixed 2026-10-31.** Every ffmpeg run goes through `platform::quiet_command`, which sets
`CREATE_NO_WINDOW` on Windows. Type-checked for Windows; not yet run there.

### R13
**Plugging in a card doesn't bring up its setup.** (Windows)
[volumes.rs:68](../src/card/volumes.rs#L68)

Card recognition (E3) looks for labelled cards among the mounted drives, and on Windows none are
listed yet (DISTRIBUTION §1 #1). The safety check still works: a setup won't sync to another
setup's card once its folder is chosen by hand. Fixed by the same work as #1.

**Fixed 2026-10-31** (with DISTRIBUTION §1 #1 and #2). On Windows, drive letters are listed
(not Windows' own drive, network or CD drives) with their label, filesystem and free space,
through `windows-sys`. Type-checked for Windows; not yet run there.

### R25
**Playback can stutter while the music drive is busy.**
[player.rs:74](../src/audio/player.rs#L74), [decode.rs](../src/audio/decode.rs) (`open`)

The song is read from disk and decoded inside the audio output's own callback, as the sound card
asks for samples: nothing is decoded ahead. When the music drive is slow to answer (an external
hard disk that's also being read by a Sync, a scan, a card comparison, or by the waveform, which
decodes the whole song at once as it starts), the callback waits and the sound drops out for a
moment. Reported 2026-10-31: "sometimes the audio sounded clipped", with the music on an external
USB drive.

**Fixed 2026-10-31** (`audio/prefetch.rs`): songs are decoded on a thread of their own, up to 8
seconds ahead of the output, which only takes what's ready. Measured with the `stutter` test, 20
seconds of a 24-bit song from the external drive while another thread read about 1.1 GB from it:
read as before, 2 and 6 slices of 10 ms came late (the longest wait 1.9 s and 0.8 s, each an
audible dropout); through the read-ahead, none (the longest wait under 0.5 ms).

## Low

### R14
**Writing tags can't clear a field, shortens dates to the year, and leaves old secondary tags.**
[tags.rs:24](../src/library/tags.rs#L24), [tags.rs:39](../src/library/tags.rs#L39)

Only fields that have a value are written, so a field emptied in the app (an album artist
removed) comes back from the file at the next scan. The date is replaced by the year alone, so
"1997-05-21" becomes "1997". The compilation flag isn't written. Only the main tag is updated:
an MP3's ID3v1 tag, or an APE tag on other files, keeps the old values for players that read it.

**Fixed 2026-10-31.** Emptied fields are removed from the file; a full date stays while its year
is unchanged; the compilation flag is written; and every tag in the file gets the same fields
(the cover goes in the main one only). Tested on a file carrying two tags.

### R15
**Undoing a tidy can overwrite a file.** (macOS, Linux)
[journal.rs:50](../src/card/journal.rs#L50)

Undo moves each file back with a plain rename, which on macOS and Linux replaces whatever now
has that name (a file added there since). (Rust's rename replaces files on Windows too.)

**Fixed 2026-10-31.** Undoing a tidy, and undoing a take-over, leave a file where it is, and
report it, when its old place is taken by another file. A change of case only (one file under
two spellings on most Mac and Windows disks) still goes back. Tested.

### R16
**A rescan while music plays leaves the player bar without the song.**
[events.rs:41](../src/app/events.rs#L41)

A rescan clears the play queue, but the music goes on. The bar says "Nothing playing", and
**next** stops the music.

**Fixed 2026-10-31.** The queue follows its songs into the new library by id. Should one of
them be gone, it's let go as before (the audio thread knows songs by their place in it).

### R17
**Why a song was skipped is often lost.**
[player.rs:74](../src/audio/player.rs#L74)

When a song fails and the next one plays, the error is overwritten; only "skipped 1 file
NeatLibrary can't play" remains.

**Fixed 2026-10-31.** The notice ends with the last reason: "skipped 1 file NeatLibrary can't
play (last: song.ape: …)".

### R18
**Playing album files needs up to ~4 GB of temporary space.**
[decode.rs:181](../src/audio/decode.rs#L181)

Formats the built-in decoder can't read (APE, WavPack…) are converted to WAV whole: about 600 MB
for a CD stored as one file, and the last 6 conversions are kept. Starting the app also clears
the folder another running copy of the app may be using.

**Fixed 2026-10-31.** The conversions kept are capped at 1 GB, newest first (the one playing is
always kept). Clearing at start-up is harmless to another running copy: a file it has open
stays readable on macOS and Linux, and can't be removed on Windows.

### R19
**"Dropping DeviceSink…" is printed on quitting.**
[player.rs:104](../src/audio/player.rs#L104)

A notice from the audio library, shown in the terminal (when the app was started from one) as it
closes the output. Harmless.

**Fixed 2026-10-31** with R11: `log_on_drop(false)`.

### R20
**Folders linked into the music folder are skipped.** (Linux, macOS)
[scan.rs:62](../src/library/scan.rs#L62)

The scan doesn't follow symbolic links, which Linux users often use to gather music from several
drives. Decide whether to follow them (walkdir guards against loops).

**Fixed 2026-10-31.** Links are followed; songs keep their paths inside the music folder. A
link back up is skipped. A link to a drive that isn't connected counts as an unreadable folder,
so the songs known there are kept rather than forgotten (as in R1). Tested.

### R21
**DSD files aren't scanned, though the stock firmware plays them.**
[scan.rs:16](../src/library/scan.rs#L16)

`.dsf` and `.dff` aren't in the list of audio files, so they don't appear at all. A missing
feature more than a bug: they'd need a profile entry and a decision on conversion.

**Fixed 2026-10-31.** `.dsf` and `.dff` are scanned: rate, channels and length from their
headers (`library/dsd.rs`), tags from their ID3v2 block. They play here through ffmpeg (at
88.2 kHz), go on the card as they are for the stock firmware (DSD64–256 per its
specifications, not yet checked on a player), and become 24-bit/88.2 kHz FLAC for RECHO.
Tested with a made-up DSD64 file, through to the card and a second, unchanged write.

### R22
**A song folder named like the playlists folder confuses Sync.**
[write.rs:74](../src/card/write.rs#L74)

An artist (or pattern) whose folder is `_Playlists`, or "NeatLibrary format test", collides with
the folders the app keeps for itself: renaming treats its songs as playlist copies, and the take-
over skips the test folder. Rare.

**Fixed 2026-10-31.** A song folder with one of those names gets a `_` added on the card.
Tested.

### R23
**The card's `.neatlibrary` label shows in Explorer.** (Windows)

A leading dot hides a file on macOS and Linux only.

**Fixed 2026-10-31.** On Windows it's written with the hidden attribute (which Windows also
needs given to write over it). Type-checked for Windows; not yet run there.

### R24
**The card tab recomputes sizes on every frame.**
[card.rs:27](../src/ui/card.rs#L27)

Each frame works out every song's card size and the card's free space. Fine on this Mac with 7,000
songs; worth watching on slower machines.

**Fixed 2026-10-31.** Measured at 0.2 s a frame in a debug build with 7,100 songs. The sizes are
now worked out again only when the library, the setup or its playlists change.

### R26
**ffmpeg's errors during playback go to the terminal, not the app.**
[decode.rs:209](../src/audio/decode.rs#L209)

Converting a song to play it (APE, WavPack, Dolby…) runs ffmpeg with its error output going to
wherever the app was started from. With `cargo run`, that's the terminal: playing the damaged ELP
files (R8) prints "Error opening input: Invalid data found when processing input" there, twice a
song (the player and the waveform each try). *Fix:* capture ffmpeg's output, as card conversions
already do, and put its last line in the error the player shows.

**Fixed 2026-10-31.** ffmpeg's error output is captured, and its first line, the cause, goes in
the error the player shows ("skipped 1 file NeatLibrary can't play (last: … ffmpeg couldn't
convert it: Error opening input: Invalid data found when processing input)"). Nothing reaches
the terminal any more.

---

## Checked, nothing found

- Names with accents and non-Latin scripts: every string cut lands on a character boundary.
- `unwrap`s outside tests: each is on something just checked or built.
- Copying: a temporary name, a sync to disk, then a rename, so a cut-short copy never looks done.
- Removing from the card: only files in the setup's record, and only while their size matches.
- Take-over and Trash: copies are confirmed by size and the first and last 64 KB.
- CUE sheets: encodings, Windows paths in `FILE` lines, data tracks.
- MusicBrainz: one request a second, backs off on 503, 20-second time-out.
- The library database: the folder-prefix range uses each system's path separator.
