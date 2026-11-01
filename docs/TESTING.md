# Testing on Windows and Linux

*Written 2026-10-27, before the app had ever been built on either system.*

Pausetape was developed and tested only on macOS. The code is meant to be portable, but
nobody has compiled or run it on Windows or Linux yet. This guide takes you from a fresh machine
to a full test pass, says what to expect (including the gaps already known), and how to report
results.

Related: [PROGRESS](PROGRESS.md) (overall status) · [DISTRIBUTION](DISTRIBUTION.md) (§1 lists the
portability fixes) · [REVIEW](REVIEW.md) (bugs found by a code review, 2026-10-31) ·
[ARCHITECTURE](ARCHITECTURE.md) (where things live in the code).

---

## 0. First signal: the CI run

Once the repository is on GitHub, every push runs `.github/workflows/ci.yml`. It builds and
tests on **macOS, Windows and Linux**. Check the **Actions** tab first:

- **Green on all three:** it compiles and the 35 tests pass everywhere. Go on to the hands-on
  test below.
- **Red on Windows or Linux:** open the failed step's log; that's the first thing to fix. A
  compile error there is likely, since this code has never met those compilers.

Each run also uploads the built program (*Artifacts* at the bottom of the run page:
`pausetape-Windows`, `pausetape-Linux`), so you can test on a machine without installing
Rust. On Linux, make it executable first: `chmod +x pausetape`.

---

## 1. Set up the build tools

Skip this if you use the CI artifacts above, except for ffmpeg, which you'll still want.

### Windows 10 / 11

1. **Git**: <https://git-scm.com/download/win>
2. **Rust**: run `rustup-init.exe` from <https://rustup.rs> and accept the defaults (MSVC
   toolchain). If it says the Visual C++ build tools are missing, let it install **Visual Studio
   Build Tools** with the **"Desktop development with C++"** workload. It's needed both to link
   the app and to compile the SQLite database built into it.
3. **ffmpeg** (optional; not bundled on Windows yet): in PowerShell,
   `winget install Gyan.FFmpeg`, then **open a new terminal** so the PATH update applies.
   Without it, Dolby (E-AC-3/AC-3), Opus, APE, WavPack and WMA won't play, which is expected.

### Linux

1. **Rust**: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`, then open a
   new terminal.
2. **Build dependencies** (audio and windowing headers, C compiler):

   | Distribution | Command |
   |---|---|
   | Ubuntu / Debian / Mint | `sudo apt install git build-essential pkg-config libasound2-dev libxkbcommon-dev libwayland-dev` |
   | Fedora | `sudo dnf install git gcc pkgconf-pkg-config alsa-lib-devel libxkbcommon-devel wayland-devel` |
   | Arch / Manjaro | `sudo pacman -S git base-devel alsa-lib libxkbcommon wayland` |

3. **ffmpeg** (optional): `sudo apt install ffmpeg` / `sudo dnf install ffmpeg` (Fedora needs
   RPM Fusion for the full build) / `sudo pacman -S ffmpeg`.

Wayland and X11 both work. The "system picker…" file dialog uses the desktop's portal service
(`xdg-desktop-portal`, standard on GNOME and KDE).

---

## 2. Build and run

```sh
git clone https://github.com/<you>/<repo>.git
cd <repo>
cargo test                          # 35 tests should pass (9 more are skipped on purpose)
cargo run --release -- --version    # e.g. "Pausetape 0.1.0 · ffmpeg: C:\…\ffmpeg.exe"
cargo run --release                 # the app
```

The first build takes several minutes (it compiles ~300 crates and SQLite). Later builds are
quick. Run the app **from a terminal** during testing: errors and panics are printed there.

A dry run that changes nothing, useful as a first check on real music:

```sh
cargo run --release -- --plan "D:\Music"        # Windows
cargo run --release -- --plan ~/Music           # Linux
```

---

## 3. Known gaps (found by reading the code; not yet seen on a real machine)

| # | System | What you'll see | Workaround for now |
|---|---|---|---|
| 1 | **Windows** | *Fixed 2026-10-31, not yet run on Windows:* drives are now listed by letter and label ("NO NAME (E:)"). | To check it: the card dialog lists the card; Windows' own drive isn't listed. If no drive shows, use **Use a folder on a card…** → **system picker…** and say so. |
| 2 | **Windows** | *Fixed 2026-10-31, not yet run on Windows:* free space, filesystem and the "not FAT" warning. | To check it: the card tab shows the size bar and "FAT32"/"exFAT". |
| 3 | **Windows** | A **console window** opens next to the app; closing it closes the app. | Leave it open (it also shows errors, which helps testing). |
| 4 | **Windows** | The in-app browser can't reach other drives except through **system picker…**; there's no "Drives" button. | Use **system picker…**. |
| 5 | **Windows** | An artist, album or title named exactly `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`9` or `LPT1`–`9` can't be written to the card. | Rename it for that write in the card tab. Rare. |
| 6 | **Windows** | Card paths can exceed 260 characters with very long names; the app copes, but Explorer may not open them. | Rename long albums for the card. |
| 7 | **Linux** | **Every** mounted drive under `/media/$USER` or `/run/media/$USER` is listed as a possible card, and the "not FAT32/exFAT" warning never appears (filesystem type isn't read on Linux yet). | Pick the right one by name and size. |
| 8 | **Linux** | Fonts: the serif look depends on DejaVu Serif at the Debian/Ubuntu path. On Fedora/Arch the app falls back to a sans font, and Chinese/Japanese/Korean names may show as boxes. | Cosmetic only. |
| 9 | Both | ffmpeg isn't bundled (only the Mac app includes it). | Install it as in §1. |
| 10 | All | *Fixed 2026-10-31:* starting with the music folder's drive unplugged used to empty the library database, and a Sync with an empty library removed the setup's songs from the card ([REVIEW](REVIEW.md) R1, R2). Now the library says it can't reach the folder and keeps everything. | To check it: start the app with the music drive unplugged, plug it in, and see it read again by itself. |
| 11 | **Windows** | *Fixed 2026-10-31, not yet run on Windows:* plugging in a labelled card brings up its setup, and converting songs no longer flashes console windows ([REVIEW](REVIEW.md) R12, R13). The card's `.pausetape` file is hidden (R23). | To check it: unplug and plug the card back in with another setup open; Sync a few songs that need converting and watch for console windows; the card's root in Explorer shows no `.pausetape` (unless hidden files are shown). |
| 12 | **Windows**, **Linux** | *Fixed on Windows 2026-10-31:* **Move to the Trash** leaves files on USB sticks, SD cards and network drives where they are, as they'd be deleted for good there. **Linux:** files should go to a `.Trash-<uid>` folder on the drive ([REVIEW](REVIEW.md) R10). | To check it on Linux: move one covered copy to the Trash on an external drive and find it in the file manager's Trash. |
| 13 | **Linux** | *Fixed 2026-10-31:* folders linked into the music folder are followed ([REVIEW](REVIEW.md) R20). | To check it: link a folder of music in and rescan. |

\#1–9 have planned fixes in [DISTRIBUTION §1](DISTRIBUTION.md#1-code-changes-needed-first); \#10–13
and the rest of what a full code review found are in [REVIEW](REVIEW.md).

---

## 4. Test checklist

**Prepare:** a small music folder (a few albums: FLAC and MP3, plus ideally one M4A, one with
cover art, one with a missing or misspelled artist), and a **spare** SD card or USB stick
formatted **exFAT or FAT32** standing in for the Echo Mini's card. Don't use a card with
anything you'd miss.

Mark each step ✅ / ❌ and note anything odd. Expected behaviour is what macOS does today.

### A. Start-up
1. `--version` prints version and an ffmpeg path (or "not found").
2. The app opens on the **folders** tab with two numbered steps; text uses a serif font
   (Windows: Palatino Linotype; Ubuntu: DejaVu Serif).
3. Settings › appearance: switching *paper* / *ink* works.

### B. Music folder and library
4. Folders › **Choose folder…** opens the in-app browser (Home, Music, Downloads buttons;
   breadcrumb; song counts per folder). Pick your test folder with **Use "…"**.
5. The status line reports songs read; the library tab lists artists with letter headings
   and the A–Z rail on the right.
6. Each album shows its folder and **open folder** opens Explorer / your file manager there.
7. **Rescan**: the status line says the songs came from the library database and "0 read
   from files". Reopen the app: the library is there at once, "as the folder was read …",
   without reading the folder; songs added meanwhile by another program show after **rescan**.
8. Narrow the window to its minimum: nothing overlaps the A–Z rail; long names end in "…".

### C. Playback and formats
9. Click a track: it plays; the waveform appears; cover art shows if the album has any.
10. Previous / play-pause / next buttons, seeking by clicking the waveform, space, ←/→, N/P.
11. An M4A (AAC) plays and shows a "M4A · not on RECHO" badge. With ffmpeg: an Opus or Dolby
    file plays after a short "converting with ffmpeg…".
12. Settings › ffmpeg shows the path found (or "not found" with an install hint).
12a. If you have an album stored as one file plus a `.cue` sheet: it shows as separate songs
    ("one file + CUE sheet" under the album), and each song plays from its own start with its
    own length.

### D. Artists and MusicBrainz
13. Library: a misspelled artist shows "≈ looks like …" under its name (the chip *artists that
    look alike* shows only those); **same artist** merges them, and Settings › Artist decisions
    can undo it. Open an artist: after a moment its genres, country and years show, with links
    to buy (Bandcamp, Qobuz, …), listen and discover; each opens in the browser. **rename**
    changes the artist in its songs' tags, as corrections.
14. Library › an album › **look up**: MusicBrainz suggestions appear; **apply selected** marks
    tracks as edited. Quit and reopen: the edits are still there (library database).
14a. If the same album is in two folders: the library shows "edition 1 of 2" and "2 of 2" with
    their formats; the card tab ticks one, and ticking the other switches. If one copy has the
    same songs in the same or lower quality, it says "this copy could go", and the library's
    "album copies covered by another" chip lists them.
14b. Play an album whose cover is a `folder.jpg`/`Front.jpg` (not embedded): the player says
    "cover from … · add it". After **add it** and a write, the card copies show the cover in
    Explorer / the file manager (and on the Echo Mini).
14c. Library › an album › **edit tags**: change the year and one song's title, **Save**: the
    album's year changes, those songs say *edited*, and the toolbar offers *write N corrected
    tags*. ⌘-click (Ctrl-click) two songs of different albums, then **edit their tags**: boxes
    whose values differ say "several values"; type an artist, save: both songs move under it.
    Right-click a song: the menu offers editing it, its album, or the selection. Esc clears the
    selection; a year like "nineties" keeps **Save** off.
14d. Library › **N albums without a cover** › **find covers for these** (try a few first, with
    **find cover** on one album): the line under the tabs counts albums; afterwards the message
    says how many came from folders and from the Cover Art Archive, and those not found are
    listed. Play one: the player shows the new cover and offers **don't**. Sync: the card copies
    have it embedded (the Echo Mini shows it).
14e. Search `format:flac year:1990..1999`, then `-format:flac`, `cover:no`, `card:on`; **filters**
    ticks and unticks the same terms; **save** keeps a search as a button beside the box.
14f. **albums**: a table of albums. Sort by size (largest first), by year, by format. Click,
    ⌘-click and shift-click to select; the bar's **send to card** marks them for the next Sync.
14g. Duplicates view › **move it to the Trash** on a covered copy: its folder is in the Trash
    (only its songs, if the folder holds other music), and the library rescans.
14h. **add music** › choose a folder with an album the library doesn't have and one it has: the
    dialog counts them apart; **Add them** copies the new one under `Added` and rescans.

### E. The card
15. Folders › **Choose card…**: Linux lists the stick by name; Windows uses the workaround
    (gap #1). Step 2 then shows the card.
16. Card tab › **Compare with the card**: counts of files to copy appear.
17. **Write to the card**. Then check the stick in Explorer / the file manager:
    `Artist/1997 - Album/01 - Title.ext`, no `._` files, playlists under `_Playlists`.
18. Write again: "0 copied", everything "unchanged".
18a. With a CUE album and ffmpeg: each song is its own FLAC on the stick, with the right
    length; writing again cuts nothing. Without ffmpeg, those songs are left out with
    "needs ffmpeg to cut the album file into songs".
18b. With ffmpeg, write an album in ALAC or AAC (`.m4a`): on the stick it's FLAC or MP3 with
    the same length, title and cover; writing again converts nothing. Settings › ffmpeg says
    whether this ffmpeg can make MP3s (the Windows `winget` and Linux distribution builds can).
18c. On a stick that already has music copied by hand, never written to: **Compare with the
    card** opens *Music already on the card* by itself (once; later, **go through them**);
    **Check them against the library** sorts them; **Take
    over the card** moves the copies into the layout (and, if ticked, the rest into
    `_Before Pausetape`; with **delete clutter** ticked, iTunes data, `desktop.ini`/`Thumbs.db`
    and rip logs are deleted); **undo taking over the card** puts everything back (except deleted
    clutter). **copy them into your music folder** puts the card's songs that aren't in the
    library under `From the card` in the music folder.
18d. Choose **hi-res songs at CD quality** (Saving space): 24-bit songs are 16-bit FLAC on the
    stick. Choose **all lossless songs as MP3 320**: FLAC songs are MP3s on the stick, and the
    space chosen drops to about a quarter. With more chosen than fits, the card tab lists the
    savings and the artists taking most room; **leave out** unticks one. After
    a write, the library shows "on card" on the albums written.
19. Untick one album, write again: that album's folder is removed; nothing else on the stick
    is touched (put an extra file there beforehand to check).
19a. In the library, **take off card** on an album that's on the card: its songs get a struck red
    ring and "taken off at the next sync"; **send to card** on another: rings and "added at the
    next sync". The strip above the list and the card tab's pinned panel count both. **Sync**
    (on either): the marks become dots, and the card has exactly that change. With the card
    unplugged, the marks and counts stay the same; *Sync* says "card not connected".
19b. **New** write: nothing is chosen. **send all to the card** on an artist: all its albums
    (one edition each) are chosen; a new album of that artist added to the music folder is
    chosen after a rescan.
19c. An album whose songs name different artists (a duet without an album-artist tag, a
    compilation flag on some songs) shows once, under one artist. Beside the library's filter,
    **N songs stored twice** opens the duplicates view; on such an album the extra copies say *stored
    twice*, and **move the extra copies to the Trash** puts them in the Trash (Finder shows
    them there) and rescans. A repeat already on the card gets a struck red ring, and the next
    Sync takes it off.
19d. The tabs sit on a tinted bar, each with an icon; the open one is filled. Narrow the window
    below ~1000 px: the other tabs show only their icon (hover names them). Start a scan: its
    progress shows on a line under the bar, and the result appears on the line below with a
    close button. Commands (play, look up, rename, Sync…) have icons and a background on hover;
    the card tab's and settings tab's options are named on the left with smaller explanations.
19e. Start a long write: the line under the tab bar counts files and, after a few seconds, the
    time left. **Stop**: it says "stopping…", then the message says what was copied. **Sync**
    again: it carries on (what's done is unchanged) and finishes with the playlists.
20. If you have the Echo Mini: put the card in and browse. Folders sort as intended.
20a. Card tab › **player** › *Echo Mini · stock firmware*: **Compare** again. Fewer songs are
    converted (AAC, APE, WMA and Ogg go as they are), and the library's format note and badges
    say "the Echo Mini" instead of "RECHO". Switch back to RECHO and the counts return.
20b. **test it on the player** › **Write the test folder to the card**: the card gets
    `Pausetape format test/` (17 samples, or 16 without an APE song in the library) and
    `Pausetape format test.m3u8`. The card tab doesn't count them as music the write didn't put
    there. Try them on the player and tick yes or no; the text at the bottom says what changes.
    Untick a format the player couldn't play and **Compare** converts it. **take it off the card**
    removes only the test's files.
20c. **Sync** to the card, then look at its top folder (⇧⌘. shows hidden files): `.pausetape`
    names the card setup. Make a second setup (**duplicate**), switch to it: the card tab says the
    card belongs to the first and offers **switch to** and **use it for this setup**; Sync is gone.
    Eject the card, switch back to the second setup, plug the card in: within a few seconds the
    app switches to the first setup and says so.
20d. Card tab › **Names on the card**: type `{albumartist} - {album} [({year})]/{track} {title}`.
    The example under it and the folder names in the list follow as you type; a broken pattern
    (`{album}/[{year}`) says what's wrong and changes nothing. The sync line says how many songs
    will be renamed on the card. **Sync**: they're moved, not copied (quick), and the old folders
    are gone. **back to the album folders above** and Sync again moves them back.

### F. Playlists
21. Create a playlist, add tracks with the **+** at the end of a track row, reorder, include
    it in the write (card tab), write: a numbered folder appears under `_Playlists`.

### G. Odds and ends
22. HiDPI / scaled display (125%, 150%): crisp text, nothing cut off.
23. Names with accents (Beyoncé) and, if you have any, Chinese/Japanese/Korean titles.
24. Quit and reopen: the music folder, card, writes and playlists are remembered.

---

## 5. Screenshots for a report

Debug builds can capture every tab and dialog automatically. Run from the project folder:

```powershell
# Windows (PowerShell)
$env:PAUSETAPE_SHOT = "C:\temp\shots"; cargo run
```

```sh
# Linux
PAUSETAPE_SHOT=/tmp/shots cargo run
```

It opens the app, steps through the tabs (with a short demo: plays a track, opens dialogs),
saves `folders.ppm`, `library.ppm`, … and quits. Add `PAUSETAPE_SHOT_SIZE=720x600` to check
a small window, and `PAUSETAPE_SHOT_FILTER=<text>` to filter the library first. PPM opens in
GIMP or IrfanView, or convert with `ffmpeg -i library.ppm library.png`. The demo plays a track,
shows a sample "Road Trip" playlist, and runs a MusicBrainz lookup if your library has a
"Radiohead" album. These changes aren't saved: your real playlists and settings stay as they were.

Ordinary screenshots of the real app are just as good.

---

## 6. Reporting results

Open an issue with the **Test report** template (`.github/ISSUE_TEMPLATE/test-report.md`). It
asks for:

- the OS and version (and on Linux, the distribution and Wayland or X11);
- the output of `pausetape --version`;
- the checklist with ✅/❌;
- any error text from the terminal, and screenshots of anything that looks wrong.

---

## 7. Where the app keeps its data (to inspect or reset)

| | Settings, library database, undo journals | ffmpeg conversions (temporary) |
|---|---|---|
| Windows | `%APPDATA%\Pausetape\data\` | `%TEMP%\pausetape-converted\` |
| Linux | `~/.local/share/pausetape/` (or `$XDG_DATA_HOME/pausetape/`) | `/tmp/pausetape-converted/` |
| macOS | `~/Library/Application Support/Pausetape/` | `$TMPDIR/pausetape-converted/` |

Files there: `app.ron` (settings, writes, playlists), `library.sqlite3` (cached tags and
unsaved corrections), `journals/` (undo for *Reorganise*). Deleting the folder resets the app.
Conversions are cleared each time the app starts.

---

## 8. Still to test on macOS

- **Intel Mac:** the DMG contains an Intel build that has never run (the development Mac has no
  Rosetta). Install from the DMG and run checklist A–C.
- **First launch of a downloaded copy:** download the DMG from a GitHub release (so macOS marks
  it as downloaded) and follow "Read me first.txt": open, then System Settings › Privacy &
  Security › **Open Anyway**.
