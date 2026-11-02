#!/usr/bin/env python3
"""The website's screenshots, from a made-up music folder and card rather than anyone's real ones.

    python3 scripts/site-shots.py        ->  site/img/<name>-light.webp, <name>-dark.webp

1. Builds a small library of 90s albums in /Users/Shared/Pausetape Demo/Music: real titles and
   tags, but the audio is generated tones and the covers plain gradients, so nothing copyrighted
   is shown. It's outside the repository so that the paths in the shots name nobody.
2. Makes an empty 32 GB FAT32 disk image (sparse: a few MB on disk) and mounts it as
   /Volumes/ECHO MINI, the card. Nothing is ever written to it.
3. Runs the debug build's screenshot demo (src/ui/screenshots.rs) once per entry in RUNS and
   per theme, with HOME pointed at target/site-demo/home so that the app's database and
   settings stay apart from yours, and keeps the shots named in RUNS.

It looks albums and artists up on MusicBrainz, so it needs the internet. Needs ffmpeg and
Pillow, on macOS; each run opens the app's window for about half a minute. How the site uses
the shots, and what to check after a change of look: docs/WEBSITE.md.
"""

import os
import random
import subprocess
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
DEMO = ROOT / "target/site-demo"
MUSIC = Path("/Users/Shared/Pausetape Demo/Music")
CARD = Path("/Volumes/ECHO MINI")
OUT = ROOT / "site/img"
SIZE = "1240x900"
WIDTH = 1600   # kept at about twice the size the site shows them

# Each run of the demo: its settings, and {demo shot: name on the site}.
RUNS = [
    # Radiohead, with what MusicBrainz says of them: the picture at the top of the page.
    ({"PAUSETAPE_SHOT_ARTIST": "Radiohead", "PAUSETAPE_SHOT_FILTER": "radiohead"}, {"library": "library"}),
    # The step-by-step tour follows Björk, whose albums are tagged two ways ("Bjork").
    ({"PAUSETAPE_SHOT_ARTIST": "Björk", "PAUSETAPE_SHOT_FILTER": "björk"},
     {"picker": "step-music", "folders": "step-card", "library": "step-tidy", "card": "step-choose",
      "job": "step-record"}),
]

# (artist tag, album, year, format, cover: gradient colours, and whether it's in the songs' tags
# or only a cover.jpg beside them; tracks). Tags are a little untidy on purpose ("Bjork",
# "Cranberries, The"), as real ones are; Pixies have none at all, only folder and file names.
ALBUMS = [
    ("Radiohead", "OK Computer", 1997, "flac", ("#3b5b7a", "#d9dde0", True),
     ["Airbag", "Paranoid Android", "Subterranean Homesick Alien", "Exit Music (For a Film)", "Let Down", "Karma Police"]),
    ("Portishead", "Dummy", 1994, "flac", ("#2b3a55", "#b04a3a", False),
     ["Mysterons", "Sour Times", "Strangers", "It Could Be Sweet", "Wandering Star"]),
    ("Massive Attack", "Mezzanine", 1998, "mp3", ("#1d1d1f", "#7a6a55", True),
     ["Angel", "Risingson", "Teardrop", "Inertia Creeps"]),
    ("Björk", "Homogenic", 1997, "flac", ("#6e3b5a", "#e4c3a8", True),
     ["Hunter", "Jóga", "Unravel", "Bachelorette"]),
    ("Bjork", "Post", 1995, "mp3", ("#c94f3d", "#f2d16b", True),
     ["Army of Me", "Hyperballad", "Isobel"]),
    ("Daft Punk", "Homework", 1997, "mp3", ("#111111", "#c8b37a", True),
     ["Revolution 909", "Da Funk", "Around the World"]),
    ("Air", "Moon Safari", 1998, "flac", None,
     ["La Femme d'Argent", "Sexy Boy", "Kelly Watch the Stars", "All I Need"]),
    ("Cranberries, The", "Everybody Else Is Doing It, So Why Can't We?", 1993, "mp3", ("#4d6b5a", "#e8e0c8", True),
     ["Dreams", "Linger"]),
    ("The Cranberries", "No Need to Argue", 1994, "mp3", ("#5a5046", "#d8c8a8", True),
     ["Ode to My Family", "Zombie", "Ridiculous Thoughts"]),
    ("Lauryn Hill", "The Miseducation of Lauryn Hill", 1998, "mp3", ("#5b4630", "#d9b98a", False),
     ["Lost Ones", "Ex-Factor", "Doo Wop (That Thing)"]),
    ("Pixies", "Doolittle", 1989, "untagged", ("#a33a2c", "#efe3c8", False),
     ["Debaser", "Here Comes Your Man", "Monkey Gone to Heaven"]),
]


def run(*args, **kw):
    subprocess.run(args, check=True, **kw)


def song(path: Path, seed: int, fmt: str, tags: dict, cover: Path | None):
    """A few minutes of tones whose loudness rises and falls like a song's, so the player's
    waveform has something to draw."""
    if path.exists():
        return
    rnd = random.Random(seed)
    secs = rnd.randint(170, 330)
    f, k = rnd.choice([110, 147, 165, 196, 220]), rnd.uniform(0, 6)
    env = f"(0.32+0.22*sin(t/{rnd.uniform(14, 30):.1f}+{k:.2f})+0.12*sin(t*{rnd.uniform(0.6, 1.4):.2f})^2)*min(t/4,1)*min(({secs}-t)/6,1)"
    expr = f"{env}*(0.6*sin(2*PI*{f}*t)+0.3*sin(2*PI*{f * 1.5}*t))"
    src = ["-f", "lavfi", "-i", f"aevalsrc='{expr}|{expr}':s=44100:d={secs}"]
    art = ["-i", str(cover), "-map", "0:a", "-map", "1:v", "-c:v", "copy", "-disposition:v", "attached_pic"] if cover else []
    meta = [x for key, v in tags.items() for x in ("-metadata", f"{key}={v}")]
    codec = ["-c:a", "flac", "-sample_fmt", "s16"] if fmt == "flac" else ["-c:a", "libmp3lame", "-b:a", "128k", "-id3v2_version", "3"]
    run("ffmpeg", "-loglevel", "error", "-y", *src, *art, *meta, *codec, str(path))


def library():
    for n, (artist, album, year, fmt, cover, titles) in enumerate(ALBUMS):
        folder = MUSIC / (f"{artist} - {album} ({year})" if fmt == "untagged" else f"{artist}/{album}")
        folder.mkdir(parents=True, exist_ok=True)
        jpg = folder / "cover.jpg"
        if cover and not jpg.exists():
            run("ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
                f"gradients=s=600x600:c0={cover[0]}:c1={cover[1]}:x0=0:y0=0:x1=600:y1=600:d=1", "-frames:v", "1", str(jpg))
        ext = "flac" if fmt == "flac" else "mp3"
        for i, title in enumerate(titles, 1):
            tags = {} if fmt == "untagged" else {"artist": artist, "album_artist": artist, "album": album, "title": title, "track": i, "date": year}
            song(folder / f"{i:02} - {title}.{ext}", n * 100 + i, fmt, tags, jpg if cover and cover[2] else None)


def mount_card():
    image = DEMO / "echo-mini.sparseimage"
    if not image.exists():
        run("hdiutil", "create", "-quiet", "-size", "32g", "-fs", "MS-DOS FAT32", "-volname", "ECHO MINI", "-type", "SPARSE", str(image.with_suffix("")))
    if not CARD.exists():
        run("hdiutil", "attach", "-quiet", "-nobrowse", "-mountpoint", str(CARD), str(image))


def shoot(n: int, settings: dict, keep: dict, theme: str):
    shots = DEMO / f"shots-{n}-{theme}"
    shots.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, HOME=str(DEMO / "home"), PAUSETAPE_SHOT=str(shots), PAUSETAPE_SHOT_SOURCE=str(MUSIC),
               PAUSETAPE_SHOT_CARD=str(CARD), PAUSETAPE_SHOT_SIZE=SIZE, PAUSETAPE_SHOT_THEME=theme,
               PAUSETAPE_SHOT_QUIET="1", **settings)
    run(str(ROOT / "target/debug/pausetape"), env=env, timeout=600)
    for shot, name in keep.items():
        img = Image.open(shots / f"{shot}.ppm").convert("RGB")
        if img.width > WIDTH:
            img = img.resize((WIDTH, round(img.height * WIDTH / img.width)), Image.LANCZOS)
        img.save(OUT / f"{name}-{theme}.webp", quality=86, method=6)


if __name__ == "__main__":
    library()
    run("cargo", "build", cwd=ROOT)
    mount_card()
    OUT.mkdir(parents=True, exist_ok=True)
    try:
        for n, (settings, keep) in enumerate(RUNS):
            for theme in ("light", "dark"):
                shoot(n, settings, keep, theme)
    finally:
        run("hdiutil", "detach", "-quiet", str(CARD))
    print(f"screenshots in {OUT.relative_to(ROOT)}")
