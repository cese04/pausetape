---
name: Test report
about: Results of running the checklist in docs/TESTING.md on a machine
title: "Test report: <OS> <version>"
labels: testing
---

**System:** <!-- e.g. Windows 11 23H2 / Ubuntu 24.04 GNOME (Wayland) / macOS 14 Intel -->

**How it was built:** <!-- cargo run --release from commit abc1234, or CI artifact from run #N -->

**`neatlibrary --version`:**
```
<paste>
```

**`cargo test`:** <!-- all passed / failures (paste them) / not run -->

### Checklist (docs/TESTING.md §4)

| Step | Result | Notes |
|---|---|---|
| A1–3 Start-up | | |
| B4–8 Music folder and library | | |
| C9–12 Playback and formats | | |
| D13–14 Artists and MusicBrainz | | |
| E15–20 The card | | |
| F21 Playlists | | |
| G22–24 Odds and ends | | |

### Known gaps confirmed or not (TESTING.md §3)
<!-- e.g. "#1 confirmed, workaround works", "#7 not seen" -->

### Errors from the terminal
```
<paste anything printed while the app ran>
```

### Screenshots
<!-- drag images here -->
