# NeatLibrary

*(working name)*

A desktop app that organises your music for the **Snowsky Echo Mini** running the
[RECHO](https://recho.atzkey.workers.dev/) firmware, and plays it on your computer in a quiet,
Tufte-inspired player.

RECHO is fast, but it reads the SD card as it is: it browses folders sorted by name, has no
playlists, and plays only FLAC, MP3 and WAV. NeatLibrary turns a music folder (organised or not)
into exactly the layout RECHO browses well:

```
Artist/
  1997 - Album/
    01 - Title.flac
    1-01 - Title.flac        (multi-disc)
Various Artists/Compilation/03 - Track Artist - Title.mp3
_Playlists/Road Trip/01 - Artist - Title.flac   (+ Road Trip.m3u8)
```

## What it does

- **Reads your music where it is.** Any folder, tidy or not. Tags first, folder names as a
  fallback; a library database makes rescans near-instant.
- **Fixes artist names.** "The Beatles" / "Beatles, The", "AC/DC" / "ACDC", "Beyoncé" /
  "Beyonce" merge automatically; likely typos are suggested for you to confirm.
- **Suggests tags from MusicBrainz**, applied only when you accept them.
- **Writes to the card the way you want it.** Saved *writes* choose which artists, albums and
  playlists go on a card, what they're called there and in what order. Rewrites copy only what
  changed and remove only files they put there themselves.
- **Playlists for a player without playlists:** folders of numbered copies (FAT/exFAT cards
  can't hold symlinks).
- **Plays almost anything**, including Dolby Atmos (E-AC-3) releases via ffmpeg, and marks
  clearly what the Echo Mini can't play.
- **Albums stored as one file plus a `.cue` sheet** show as separate songs, as in foobar2000
  or AIMP. On the card, each song becomes its own FLAC file.
- **Several copies of an album** (rips, reissues) are recognised as editions; one goes on the
  card, and you choose which.
- **Cover art in the songs:** when a cover is only an image in the folder, the player offers to
  embed it, so the Echo Mini shows it.

## Status

Early, unreleased (0.1.0). Developed and tested on **macOS**; **Windows and Linux have not been
tried yet**. Details: [docs/PROGRESS.md](docs/PROGRESS.md).

| Platform | How to get it | State |
|---|---|---|
| macOS 12+ (Apple Silicon + Intel) | Build the DMG (below), or download from Releases once published | Works; not signed by a registered Apple developer yet |
| Windows 10/11 | Build from source | Untested; known gaps in [docs/TESTING.md](docs/TESTING.md) |
| Linux | Build from source | Untested; known gaps in [docs/TESTING.md](docs/TESTING.md) |

## Install on macOS

1. Open `NeatLibrary-<version>.dmg` and drag the app to Applications.
2. Open it once; macOS says it can't verify the developer. Click **Done**.
3. System Settings › Privacy & Security › **Open Anyway**. Only needed the first time.

To build the DMG yourself (needs Xcode Command Line Tools and
`rustup target add x86_64-apple-darwin`; the first run also compiles the bundled ffmpeg):

```sh
scripts/package-macos.sh      # → dist/NeatLibrary.app and dist/NeatLibrary-<version>.dmg
```

## Build from source (any platform)

Install Rust from <https://rustup.rs>, plus on Windows the Visual Studio C++ build tools, and on
Linux the audio/windowing headers (exact commands per distribution in
[docs/TESTING.md §1](docs/TESTING.md#1-set-up-the-build-tools)). Then:

```sh
cargo run --release                          # the app
cargo run --release -- --version             # version, and which ffmpeg it uses
cargo run --release -- --plan ~/Music        # dry run: print the planned layout, change nothing
cargo test                                   # 30 offline tests
```

ffmpeg is optional outside the Mac app (`winget install Gyan.FFmpeg`, `sudo apt install ffmpeg`,
`brew install ffmpeg`). Without it, everything except Dolby, Opus, APE, WavPack and WMA plays.

## Documentation

| Document | For |
|---|---|
| [docs/PROGRESS.md](docs/PROGRESS.md) | What's built, how it was verified, known issues, roadmap and UI/UX ideas |
| [docs/NEXT.md](docs/NEXT.md) | Agreed next changes: safer MusicBrainz lookups, folder-name reading, CUE sheets |
| [docs/TESTING.md](docs/TESTING.md) | Testing on Windows and Linux, step by step, and how to report results |
| [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md) | Packaging, signing and releases for macOS, Windows and Linux |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | How the code fits together, where data is stored |
| [CHANGELOG.md](CHANGELOG.md) | Everything done so far, in order |

## License

[MIT](LICENSE). The macOS app bundles a minimal build of [FFmpeg](https://ffmpeg.org) (LGPL v2.1),
whose licence and build details are inside the app under `Contents/Resources/Licenses`.
