# Working on Pausetape

The repository follows **git flow**: `main` holds released versions only, `develop` is where
work comes together, and each piece of work has a branch of its own.

## Branches

| Branch | Comes from | Goes back into | Holds |
|---|---|---|---|
| `main` | — | — | Released versions only, each one tagged `vX.Y.Z` |
| `develop` | `main` | `release/*` | Finished work, waiting for the next version |
| `feature/<name>` | `develop` | `develop` | One piece of work in progress |
| `release/X.Y.Z` | `develop` | `main` and `develop` | Getting a version ready: version number, changelog, last fixes |
| `hotfix/X.Y.Z` | `main` | `main` and `develop` | An urgent fix to a released version |

Nobody commits on `main` or `develop` directly: work happens on a `feature/` branch, and
merges keep the branch visible in the history (`git merge --no-ff`).

## A piece of work

```sh
git switch develop && git pull
git switch -c feature/eject-card          # or an existing feature branch
# … commits …
git switch develop && git merge --no-ff feature/eject-card
git branch -d feature/eject-card
```

Commit messages are [conventional](https://www.conventionalcommits.org): `type(scope): summary`,
for example `feat(card): eject the card after a write` or `fix(audio): …`. Every change of
behaviour gets its entry in [CHANGELOG.md](CHANGELOG.md), and the docs it touches are updated
with it.

## A version

```sh
git switch -c release/0.2.0 develop
# version in Cargo.toml, CHANGELOG heading, last fixes; `cargo test` passes
git switch main && git merge --no-ff release/0.2.0
git tag -a v0.2.0 -m "Pausetape 0.2.0"
git switch develop && git merge --no-ff release/0.2.0
git branch -d release/0.2.0
git push origin main develop v0.2.0
```

A hotfix goes the same way, from `main` (`hotfix/0.2.1`), back into both `main` and `develop`.

## What the CI keeps

Every push and pull request is built and tested on macOS, Windows and Linux
([ci.yml](.github/workflows/ci.yml)); `main`, version tags and pull requests also get the
packaged Mac app ([macos.yml](.github/workflows/macos.yml)). The website in `site/` is published from `main`
([pages.yml](.github/workflows/pages.yml); see [WEBSITE](docs/WEBSITE.md)).

The dependencies are checked with [cargo-deny](https://embarkstudios.github.io/cargo-deny/) on
every run, and every Monday on `main` (GitHub runs scheduled workflows from the default branch),
against the rules in [deny.toml](deny.toml):

- a known vulnerability ([RustSec](https://rustsec.org)) fails the build; so does a crate no
  longer maintained if it's one of ours, not one further down the tree;
- so does a licence outside the list (permissive ones, MPL-2.0, and the fonts' licences), or a
  crate from anywhere but crates.io;
- the same crate in two versions is only a warning.

To check locally: `cargo install cargo-deny`, then `cargo deny check`.

Every `unsafe` block (they're all system calls: free space, drive letters, unlocking a file on
the card) starts with a `// SAFETY:` comment saying why it's sound; the CI runs clippy's
`undocumented_unsafe_blocks` on each platform and fails without one. For a fuller picture of the
`unsafe` in the dependencies, `cargo install cargo-geiger` and `cargo geiger` now and then: it's
slow and has no pass or fail, so it isn't part of the CI (its closing "Found … warnings" is
about data files it can't read, not a problem).

| Pushed to | Kept | For |
|---|---|---|
| a version tag `v*` | **for good**, on its GitHub Release: the Mac disk image, and the Windows and Linux binaries | Versions people download |
| `main` | 90 days (GitHub's most): binaries and the disk image, as artifacts | The latest release build |
| `develop`, `feature/*` | 7 days: binaries, as artifacts | Trying a build out |
| a pull request | nothing; it's only checked | — |

## Open feature branches

Each starts from `develop`. The fuller lists of ideas are in
[PROGRESS](docs/PROGRESS.md) and [DISTRIBUTION §1](docs/DISTRIBUTION.md#1-code-changes-needed-first).

| Branch | What it's for |
|---|---|
| `feature/artist-wikipedia` | The artist panel's short description: Wikipedia's summary, found through the artist's Wikidata link, in the system's language when there is one, else English |
| `feature/windows-portability` | DISTRIBUTION §1 #4 (no console window), #5 (reserved names such as `CON`, `AUX`), #6 (long paths), #8 (drive shortcuts), #10 (ffmpeg install hint) |
| `feature/linux-portability` | DISTRIBUTION §1 #3 (FAT/exFAT detection), #7 (one serif font everywhere), #8 (mount-point shortcuts), #10 (ffmpeg install hint) |
| `feature/play-queue` | An up-next queue: *play next* and *add to queue* on any song, shuffle, repeat |
| `feature/media-keys` | macOS media keys and Now Playing |
| `feature/eject-card` | Eject the card after a successful write |
