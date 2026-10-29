# Testing on Windows and Linux

*Written 2026-10-27, before the app had ever been built on either system.*

NeatLibrary was developed and tested only on macOS. The code is meant to be portable, but
nobody has compiled or run it on Windows or Linux yet. This guide takes you from a fresh machine
to a full test pass, says what to expect (including the gaps already known), and how to report
results.

Related: [PROGRESS](PROGRESS.md) (overall status) · [DISTRIBUTION](DISTRIBUTION.md) (§1 lists the
portability fixes) · [ARCHITECTURE](ARCHITECTURE.md) (where things live in the code).

---

## 0. First signal: the CI run

Once the repository is on GitHub, every push runs `.github/workflows/ci.yml`. It builds and
tests on **macOS, Windows and Linux**. Check the **Actions** tab first:

- **Green on all three:** it compiles and the 35 tests pass everywhere. Go on to the hands-on
  test below.
- **Red on Windows or Linux:** open the failed step's log; that's the first thing to fix. A
  compile error there is likely, since this code has never met those compilers.

Each run also uploads the built program (*Artifacts* at the bottom of the run page:
`neatlibrary-Windows`, `neatlibrary-Linux`), so you can test on a machine without installing
Rust. On Linux, make it executable first: `chmod +x neatlibrary`.

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
cargo run --release -- --version    # e.g. "NeatLibrary 0.1.0 · ffmpeg: C:\…\ffmpeg.exe"
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
| 1 | **Windows** | **No SD cards listed** in the card dialog ("No SD cards found"). Drive detection only knows macOS and Linux mount points. | In the dialog: **Use a folder on a card…** → **system picker…** → choose the card's drive (e.g. `E:\`) → **Use "E:\"**. |
| 2 | **Windows** | No free space, filesystem or "not FAT" warning for the card; the card tab shows no size bar. Step 2 says "Connected. Free space isn't shown on this system yet." | None needed; writing still works. |
| 3 | **Windows** | A **console window** opens next to the app; closing it closes the app. | Leave it open (it also shows errors, which helps testing). |
| 4 | **Windows** | The in-app browser can't reach other drives except through **system picker…**; there's no "Drives" button. | Use **system picker…**. |
| 5 | **Windows** | An artist, album or title named exactly `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`9` or `LPT1`–`9` can't be written to the card. | Rename it for that write in the card tab. Rare. |
| 6 | **Windows** | Card paths can exceed 260 characters with very long names; the app copes, but Explorer may not open them. | Rename long albums for the card. |
| 7 | **Linux** | **Every** mounted drive under `/media/$USER` or `/run/media/$USER` is listed as a possible card, and the "not FAT32/exFAT" warning never appears (filesystem type isn't read on Linux yet). | Pick the right one by name and size. |
| 8 | **Linux** | Fonts: the serif look depends on DejaVu Serif at the Debian/Ubuntu path. On Fedora/Arch the app falls back to a sans font, and Chinese/Japanese/Korean names may show as boxes. | Cosmetic only. |
| 9 | Both | ffmpeg isn't bundled (only the Mac app includes it). | Install it as in §1. |

All of these have planned fixes in [DISTRIBUTION §1](DISTRIBUTION.md#1-code-changes-needed-first).

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
7. **Rescan** (or reopen the app): the status line says the songs came from the library
   database and "0 read from files".
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
13. Artists tab: a misspelled artist appears under *possible duplicates*; **same artist**
    merges them.
14. Library › an album › **look up**: MusicBrainz suggestions appear; **apply selected** marks
    tracks as edited. Quit and reopen: the edits are still there (library database).
14a. If the same album is in two folders: the library shows "edition 1 of 2" and "2 of 2" with
    their formats; the card tab ticks one, and ticking the other switches. If one copy has the
    same songs in the same or lower quality, it says "this copy could go", and the header's
    "album copies with a same-or-better copy" link lists them.
14b. Play an album whose cover is a `folder.jpg`/`Front.jpg` (not embedded): the player says
    "cover from … · add it". After **add it** and a write, the card copies show the cover in
    Explorer / the file manager (and on the Echo Mini).

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
18c. On a stick that already has music copied by hand: **Compare with the card** says how many
    files the write didn't put there; **Check them against the library** sorts them; **Take
    over the card** moves the copies into the layout (and, if ticked, the rest into
    `_Before NeatLibrary`; with **delete clutter** ticked, iTunes data, `desktop.ini`/`Thumbs.db`
    and rip logs are deleted); **undo taking over the card** puts everything back (except deleted
    clutter). **copy them into your music folder** puts the card's songs that aren't in the
    library under `From the card` in the music folder.
18d. Tick **write hi-res FLAC at CD quality**: 24-bit songs are 16-bit FLAC on the stick. After
    a write, the library shows "on card" on the albums written.
19. Untick one album, write again: that album's folder is removed; nothing else on the stick
    is touched (put an extra file there beforehand to check).
20. If you have the Echo Mini: put the card in and browse. Folders sort as intended.

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
$env:NEATLIBRARY_SHOT = "C:\temp\shots"; cargo run
```

```sh
# Linux
NEATLIBRARY_SHOT=/tmp/shots cargo run
```

It opens the app, steps through the tabs (with a short demo: plays a track, opens dialogs),
saves `folders.ppm`, `library.ppm`, … and quits. Add `NEATLIBRARY_SHOT_SIZE=720x600` to check
a small window, and `NEATLIBRARY_SHOT_FILTER=<text>` to filter the library first. PPM opens in
GIMP or IrfanView, or convert with `ffmpeg -i library.ppm library.png`. The demo plays a track,
shows a sample "Road Trip" playlist, and runs a MusicBrainz lookup if your library has a
"Radiohead" album. These changes aren't saved: your real playlists and settings stay as they were.

Ordinary screenshots of the real app are just as good.

---

## 6. Reporting results

Open an issue with the **Test report** template (`.github/ISSUE_TEMPLATE/test-report.md`). It
asks for:

- the OS and version (and on Linux, the distribution and Wayland or X11);
- the output of `neatlibrary --version`;
- the checklist with ✅/❌;
- any error text from the terminal, and screenshots of anything that looks wrong.

---

## 7. Where the app keeps its data (to inspect or reset)

| | Settings, library database, undo journals | ffmpeg conversions (temporary) |
|---|---|---|
| Windows | `%APPDATA%\NeatLibrary\data\` | `%TEMP%\neatlibrary-converted\` |
| Linux | `~/.local/share/neatlibrary/` (or `$XDG_DATA_HOME/neatlibrary/`) | `/tmp/neatlibrary-converted/` |
| macOS | `~/Library/Application Support/NeatLibrary/` | `$TMPDIR/neatlibrary-converted/` |

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
