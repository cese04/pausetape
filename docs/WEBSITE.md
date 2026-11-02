# The website

*Written 2026-11-01.*

`site/` is Pausetape's page for the people who'd use it rather than build it: what it's for,
a step-by-step tour, where to download it and how to open it the first time. It's in English,
published on GitHub Pages at <https://cese04.github.io/pausetape/>, and plain HTML, CSS and
JavaScript: nothing to install or build.

| File | What it is |
|---|---|
| `site/index.html` | The page, and the tour's steps: their marks and notes (below) |
| `site/style.css` | The app's palette and serif, an 8 px grid, the animations |
| `site/main.js` | Sections fading in, the tour, the download links from the latest release |
| `site/img/` | Screenshots (`*-light.webp`, `*-dark.webp`), icon, favicon |
| `site/fonts/` | Caveat Bold, the handwriting on the labels (latin subset), and its licence |
| `scripts/site-shots.py` | Takes the screenshots again |
| `.github/workflows/pages.yml` | Publishes `site/` |

---

## 1. Look at it locally

```sh
cd site && python3 -m http.server 8765      # then open http://localhost:8765/
```

Any static server works; opening `index.html` as a file mostly works too, but the download
links need `http://`. Check it in a light and a dark system theme (each has its own
screenshots), at a phone's width, and with *Reduce motion* on (System Settings › Accessibility
› Display), where nothing moves and the tour doesn't play by itself.

---

## 2. The screenshots

None of them shows a real library. `scripts/site-shots.py` makes everything they show:

1. **A music folder** of eleven 90s albums at `/Users/Shared/Pausetape Demo/Music`, with real
   titles and tags, generated tones for audio and gradients for covers, so nothing
   copyrighted is published. It's outside the repository so that the paths on screen name
   nobody. Some tags are untidy on purpose (`Bjork` beside `Björk`, `Cranberries, The`;
   Pixies have no tags at all), so there's something to tidy.
2. **A card**: an empty 32 GB FAT32 disk image, sparse (a few MB on disk), mounted as
   `/Volumes/ECHO MINI`. Nothing is written to it.
3. **The app** (debug build) runs its screenshot demo, `src/ui/screenshots.rs`, with `HOME`
   pointed at `target/site-demo/home`, so its database and settings are its own and yours are
   left alone. The demo's switches it uses:

   | Variable | For |
   |---|---|
   | `PAUSETAPE_SHOT=<dir>` | Where the shots go (as `.ppm`, one per tab or dialog) |
   | `PAUSETAPE_SHOT_SOURCE=<dir>` | The music folder |
   | `PAUSETAPE_SHOT_CARD=<dir>` | The card |
   | `PAUSETAPE_SHOT_ARTIST`, `PAUSETAPE_SHOT_FILTER` | Who plays and is looked up; what the library shows |
   | `PAUSETAPE_SHOT_THEME=light\|dark`, `PAUSETAPE_SHOT_SIZE=1240x900` | Theme and window size |
   | `PAUSETAPE_SHOT_QUIET=1` | No notice above the page |

   The script runs the demo twice per theme (`RUNS`): the whole look with Radiohead for the
   top of the page, and Björk for the tour. It keeps the shots it names there, as WebP 1600
   px wide.

To take them all again (macOS, about five minutes the first time, two after; it needs ffmpeg,
Pillow and the internet, for MusicBrainz):

```sh
python3 scripts/site-shots.py
```

The app's window opens and goes through its tabs by itself; leave it alone until it closes.

### After the app's look changes

1. Take the screenshots again, as above.
2. Check the tour's marks: open `http://localhost:8765/?step=1.1`, then `?step=1.2`,
   `?step=2.1` … (step, then mark; the tour stops there). Each ring should sit on what its note
   talks about.
3. Move the marks that are off (next section), and look again.
4. If a screen changed so much that a note no longer fits, change the note too, and its
   screenshot's `alt` text.

---

## 3. The tour

The tour is five screenshots, one per step. In each, a ring moves from one thing to the next
while a note under the screen says what it is; on a phone the screenshot closes in on the ring.
It plays by itself while it's on screen (3.6 s per mark), and stops once someone picks a step
or skips; its play button starts it again. With *Reduce motion* it doesn't play by itself and nothing slides.

Each step is an `<li>` in `.tour-steps`, in `site/index.html`:

```html
<li data-shot="step-tidy"
    data-marks="3.1 35.3 15 3.6 | 4 70.6 50.4 10.5"
    data-notes="“Bjork” and “Björk” were two artists. Now they're one. | MusicBrainz suggests…">
```

- `data-marks`: one box per mark, `left top width height` **in percent of the screenshot**,
  separated by `|`. The ring is drawn 6 px outside the box.
- `data-notes`: the note for each mark, in the same order, separated by `|`.
- The screenshot of each step is the `<picture>` in the same position in `.tour-frame`.

To measure a box: open the light screenshot (1600 × 1161) in any image viewer that shows the
pointer's position (Preview's *Tools › Show Inspector*, or GIMP), note the box's corners in
pixels, and divide: `left = x ÷ 1600 × 100`, `top = y ÷ 1161 × 100`, and the same for width and
height. Both themes share the marks, since only the colours differ.

A step more or less: add or remove the `<li>` and its `<picture>`, keeping both in the same
order; the code counts them.

---

## 4. Downloads

`main.js` asks GitHub for the latest release (`api.github.com/repos/cese04/pausetape/releases/latest`)
and points each platform's button at its file, found by the pattern in `data-asset`:

| Platform | Pattern | File the release workflows attach |
|---|---|---|
| macOS | `\.dmg$` | `Pausetape-<version>.dmg` (macos.yml) |
| Windows | `Windows\.exe$` | `pausetape-v<version>-Windows.exe` (ci.yml) |
| Linux | `Linux$` | `pausetape-v<version>-Linux` (ci.yml) |

If those names change in the workflows, change the patterns too. Until there's a release (or
if GitHub can't be reached) the page says the first release is on its way and the buttons go
to the Releases page. The visitor's own platform is outlined and named on the button at the
top.

---

## 5. Publishing

`.github/workflows/pages.yml` publishes `site/` when it changes on `main`, or by hand
(*Actions › Website › Run workflow*, from `main`). Following git flow, a change to the site goes
through `develop` and reaches `main` with the next release, as the app does.

**Once, before the first publish:** in the repository's *Settings › Pages*, set *Source* to
**GitHub Actions**.

---

## 6. Choices made

- **The app's look:** its paper and ink in both themes, its serif, red only for what to do
  next. Handwriting (Caveat) only where a tape would have it: the label, side A and B, the
  track numbers.
- **The 90s, lightly:** a cassette whose reels turn, a tape deck with REC blinking, a J-card
  listing what it does (side A) and won't do (side B), a tape running under the tour.
- **Motion that explains or welcomes,** nothing more: reels, sections fading up once, the
  tour's ring, its pictures fading into each other. All of it stops under *Reduce motion*.
- **Works without JavaScript:** every section is visible and the tour reads as a list beside
  its first screen; the download buttons go to the Releases page.
- **Nothing from elsewhere:** no web fonts from Google, no analytics, no cookies. The only
  request to another site is the one to GitHub for the latest release.
- **Honest about where it stands:** used most on macOS, Windows and Linux newer; not
  affiliated with Snowsky or RECHO.
