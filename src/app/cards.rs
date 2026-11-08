//! Noticing cards: every few seconds the mounted drives are listed, and a card plugged in with a
//! label (`card::label`) brings up the card setup it belongs to. Ejecting one after a Sync.
//!
//! The looking is done on a thread of its own, as a sleeping or failing drive can take seconds to
//! answer. What's drawn comes from its last answer; what acts on the card looks again first.

use std::path::Path;

use super::*;
use crate::card::label::{Found, same_card};
use crate::card::{Label, SpaceInfo, testcard};

const WATCH_EVERY: Duration = Duration::from_secs(3);

/// What the active setup's card was when a look was asked for.
#[derive(Clone, PartialEq)]
struct Asked {
    setup: String,
    dest: PathBuf,
    card: Option<String>,
}

/// What the app wants looked at.
pub(super) struct Ask {
    /// The drives seen so far, to read the labels of any others; `None` while work runs, when
    /// arrivals wait.
    seen: Option<HashSet<PathBuf>>,
    active: Option<Asked>,
    /// Every setup's destination.
    dests: Vec<PathBuf>,
    source: Option<PathBuf>,
}

/// What the watcher's thread found.
pub(super) struct Look {
    active: Option<Asked>,
    /// Asked for with no work running, so what it says about the record can be believed.
    seen_idle: bool,
    volumes: Vec<Volume>,
    /// New drives with a label, if looked for.
    arrived: Option<Vec<(PathBuf, Label)>>,
    card: Option<CardLook>,
    reachable: HashSet<PathBuf>,
    source: Option<SourceLook>,
}

/// A setup's destination, as last seen.
pub struct CardLook {
    pub dest: PathBuf,
    pub there: bool,
    found: Found,
    pub space: Option<SpaceInfo>,
    test_folder: bool,
}

/// The music folder, as last seen.
pub(super) struct SourceLook {
    path: PathBuf,
    there: bool,
    free: Option<u64>,
}

/// Read what's at `dest`, unless its drive has just been ejected.
fn look_at(dest: &Path) -> CardLook {
    let nothing = || CardLook { dest: dest.to_path_buf(), there: false, found: Found::NotThere, space: None, test_folder: false };
    if card::held(dest) {
        return nothing();
    }
    let found = Label::find(dest);
    // Looked for after reading: a card pulled out just now finds no label either.
    if !dest.is_dir() {
        return nothing();
    }
    let test_folder = dest.join(testcard::TEST_FOLDER).is_dir() || dest.join(testcard::TEST_PLAYLIST).is_file();
    CardLook { dest: dest.to_path_buf(), there: true, found, space: card::space(dest), test_folder }
}

/// The watcher's thread: one look for each thing asked, and the answer back to `App::poll`.
pub(super) fn watcher(asks: Receiver<Ask>, tx: Sender<Msg>, ctx: egui::Context) {
    for ask in asks {
        card::let_go_of_ejected();
        let volumes = card::volumes();
        let seen_idle = ask.seen.is_some();
        let arrived = ask.seen.map(|seen| volumes.iter().filter(|v| !seen.contains(&v.path)).filter_map(|v| Some((v.path.clone(), Label::read(&v.path)?))).collect());
        let reachable = ask.dests.into_iter().filter(|d| card::connected(d)).collect();
        let card = ask.active.as_ref().map(|a| look_at(&a.dest));
        let source = ask.source.map(|path| {
            let there = path.is_dir();
            let free = there.then(|| card::space(&path)).flatten().map(|s| s.free);
            SourceLook { path, there, free }
        });
        if tx.send(Msg::Looked(Box::new(Look { active: ask.active, seen_idle, volumes, arrived, card, reachable, source }))).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

impl App {
    /// After setup `id`'s Sync, let its notice offer to eject the card: when the card is a drive
    /// that comes out, and not the one the music folder is on (that would take the library away
    /// too).
    pub fn offer_eject(&mut self, id: &str) {
        let Some(dest) = self.saved.writes.iter().find(|w| w.id == id).and_then(|w| w.dest.clone()) else { return };
        let Some(v) = self.volumes.iter().filter(|v| dest.starts_with(&v.path)).max_by_key(|v| v.path.as_os_str().len()) else { return };
        if !crate::platform::ejectable(&v.path) || self.saved.source.as_ref().is_some_and(|s| s.starts_with(&v.path)) {
            return;
        }
        self.eject_notice = self.notices.last().map(|n| (n.clone(), v.path.clone()));
    }

    /// The card the latest notice offers to eject, while it's still there.
    pub fn notice_offers_eject(&self) -> Option<PathBuf> {
        let (notice, path) = self.eject_notice.as_ref()?;
        (self.notices.last() == Some(notice) && self.volumes.iter().any(|v| &v.path == path)).then(|| path.clone())
    }

    pub fn eject(&mut self, path: PathBuf) {
        if self.busy.is_some() {
            return;
        }
        let name = self.volumes.iter().find(|v| v.path == path).map(|v| v.name.clone()).unwrap_or_else(|| path.display().to_string());
        self.busy = Some(Busy::new(format!("ejecting “{name}”"), 0));
        self.spawn(move |tx| {
            // Looking at the drive while it's dismounted would mount it again.
            card::ejecting(&path);
            let done = crate::platform::eject(&path);
            card::eject_ended(&path, done.is_ok());
            let _ = tx.send(Msg::Ejected(name, done));
        });
    }

    /// Ask for a look at the cards every few seconds, one at a time: a drive that's slow to
    /// answer delays the next look, and nothing else.
    pub fn watch_cards(&mut self) {
        #[cfg(debug_assertions)]
        if self.shooter.as_ref().is_some_and(|s| !s.watch) {
            if self.volumes.is_empty() {
                self.volumes = card::volumes();
            }
            if let Some(dest) = self.write().dest.clone() {
                self.card_look = Some(look_at(&dest));
                self.reachable = std::iter::once(dest).filter(|d| card::connected(d)).collect();
            }
            return;
        }
        if self.looking {
            return;
        }
        if let Some(t) = self.last_watch
            && t.elapsed() < WATCH_EVERY
        {
            self.ctx.request_repaint_after(WATCH_EVERY - t.elapsed());
            return;
        }
        self.last_watch = Some(Instant::now());
        self.ctx.request_repaint_after(WATCH_EVERY);
        let ask = Ask {
            seen: self.busy.is_none().then(|| self.mounted.clone().unwrap_or_default()),
            active: self.asked(),
            dests: self.saved.writes.iter().filter_map(|w| w.dest.clone()).collect(),
            source: self.saved.source.clone(),
        };
        self.looking = self.watcher.send(ask).is_ok();
    }

    /// Have the cards looked at on the next frame, not at the next turn.
    pub fn look_soon(&mut self) {
        self.last_watch = None;
    }

    fn asked(&self) -> Option<Asked> {
        let w = self.write();
        Some(Asked { setup: w.id.clone(), dest: w.dest.clone()?, card: w.card_id.clone() })
    }

    /// The answer to a look. Cards that arrive during other work (as at every start, while the
    /// library opens) are dealt with once it's done: a setup's destination doesn't change under
    /// running work.
    pub(super) fn saw_cards(&mut self, look: Look) {
        self.looking = false;
        self.volumes = look.volumes;
        self.source_look = look.source;
        if let Some(arrived) = look.arrived
            && self.busy.is_none()
        {
            let first = self.mounted.is_none();
            self.mounted = Some(self.volumes.iter().map(|v| v.path.clone()).collect());
            let bring_up = bring_up(&self.saved.writes, self.saved.active_write, &arrived);
            for (path, label) in arrived {
                self.card_arrived(path, label, bring_up, first);
            }
        }
        if look.active == self.asked() {
            self.reachable = look.reachable;
            match look.card {
                Some(seen) => self.saw_card(seen, look.seen_idle),
                None => {
                    self.card_look = None;
                    self.dest_label = None;
                }
            }
            if self.busy.is_none() {
                self.forget_a_card_gone();
            }
        } else {
            // Another setup or card since it was asked: this says nothing about the one now.
            self.look_soon();
        }
        // The music folder's drive is back.
        if self.source_missing && self.busy.is_none() && self.source_seen().is_some_and(|s| s.there) {
            self.scan();
        }
    }

    /// What was last seen at the active setup's destination.
    pub fn card_seen(&self) -> Option<&CardLook> {
        self.card_look.as_ref().filter(|l| self.write().dest.as_ref() == Some(&l.dest))
    }

    /// The active setup's card was there at the last look.
    pub fn card_connected(&self) -> bool {
        self.card_seen().is_some_and(|l| l.there)
    }

    /// The setup's destination could be reached at the last look.
    pub fn dest_reachable(&self, w: &WriteProfile) -> bool {
        w.dest.as_ref().is_some_and(|d| self.reachable.contains(d))
    }

    pub(super) fn source_seen(&self) -> Option<&SourceLook> {
        self.source_look.as_ref().filter(|s| self.saved.source.as_ref() == Some(&s.path))
    }

    /// Free space on the music folder's drive, as of the last look.
    pub fn source_free(&self) -> Option<u64> {
        self.source_seen()?.free
    }

    /// The music folder could be reached at the last look (or there hasn't been one yet).
    pub(super) fn source_reachable(&self) -> bool {
        self.source_seen().is_none_or(|s| s.there)
    }

    /// The test folder was on the active setup's card at the last look.
    pub fn test_folder_seen(&self) -> bool {
        self.card_seen().is_some_and(|l| l.test_folder)
    }

    /// The active setup's card has gone: what was seen on it no longer stands, and its dialog
    /// closes. The results of the test folder stay: it's taken out to try them on the player.
    fn forget_a_card_gone(&mut self) {
        if self.card_connected() {
            return;
        }
        let looking = self.take_over_open && (self.write_preview.is_some() || self.card_survey.is_some());
        self.write_preview = None;
        self.card_survey = None;
        if looking {
            self.notice("the card was taken out, so its music can't be gone through now");
        }
    }

    /// Says so if the active setup's card isn't plugged in.
    pub(super) fn card_missing(&mut self, dest: &Path) -> bool {
        let missing = !card::connected(dest);
        if missing {
            self.notice(format!("the card for “{}” isn't connected", self.write().name));
        }
        missing
    }

    /// Point the card's setup at where it's mounted, and say so. At startup (`first`), a card
    /// already where its setup expects it goes unremarked.
    fn card_arrived(&mut self, path: PathBuf, label: Label, bring_up: Option<usize>, first: bool) {
        // By the name it's listed under: a drive letter's path has no last part to call it by.
        let shown = self.volumes.iter().find(|v| v.path == path).map(|v| v.name.clone()).unwrap_or_else(|| path.display().to_string());
        let Some(i) = self.saved.writes.iter().position(|w| w.id == label.id) else {
            self.notice(format!("“{shown}” plugged in: written by Pausetape for “{}”, a card setup not on this computer", label.name));
            return;
        };
        let name = self.saved.writes[i].name.clone();
        let moved = self.saved.writes[i].dest.as_ref() != Some(&path);
        self.saved.writes[i].dest = Some(path);
        let switched = bring_up == Some(i);
        if switched {
            self.select_write(i);
        } else if moved && i == self.saved.active_write {
            self.write_mut();
            self.refresh_on_card();
        }
        if switched || moved || !first {
            let player = self.saved.writes[i].options.device.name();
            self.notice(format!("card for “{name}” plugged in ({shown}) · {player}{}", if switched { " · switched to it" } else { "" }));
        }
    }

    /// Look at the active setup's card right now, on this thread: for what's about to act on it,
    /// or has just changed it. Everything else waits for the watcher.
    pub fn refresh_label(&mut self) {
        let Some(dest) = self.write().dest.clone() else {
            self.dest_label = None;
            self.card_look = None;
            return;
        };
        let idle = self.busy.is_none();
        self.saw_card(look_at(&dest), idle);
    }

    /// What's at the active setup's destination now known; a different card from the one its
    /// record describes is a new one (not while work runs, which the record is part of; `idle`
    /// says none ran when this was looked at).
    fn saw_card(&mut self, seen: CardLook, idle: bool) {
        let w = self.write();
        let has_record = !w.manifest.is_empty() || w.last_written.is_some() || w.card_id.is_some();
        let another_card = idle && self.busy.is_none() && has_record && seen.there && !same_card(w, &seen.found);
        self.dest_label = match &seen.found {
            Found::Label(l) => Some(l.clone()),
            _ => None,
        };
        if seen.there {
            self.reachable.insert(seen.dest.clone());
        } else {
            self.reachable.remove(&seen.dest);
        }
        self.card_look = Some(seen);
        if another_card {
            self.card_is_new();
        }
    }

    /// Set the active setup's record aside: it describes another card, and nothing on this one
    /// is its own until it has written there.
    fn card_is_new(&mut self) {
        let w = &mut self.saved.writes[self.saved.active_write];
        let had_files = !w.manifest.is_empty();
        w.manifest.clear();
        w.last_written = None;
        w.take_over_offered = false;
        w.card_id = None;
        let name = w.name.clone();
        self.write_preview = None;
        self.card_survey = None;
        self.card_found = None;
        self.refresh_on_card();
        if had_files {
            self.notice(format!("this isn't the card “{name}” last wrote to, so it's treated as a new one: the next Sync starts from what's on it"));
        }
    }

    /// The label of the active setup's card, when it names another setup: syncing or taking it
    /// over is refused then.
    pub fn card_owner(&self) -> Option<&Label> {
        self.dest_label.as_ref().filter(|l| l.id != self.write().id)
    }

    /// The setup the card's label names, if it's on this Mac.
    pub fn owner_setup(&self) -> Option<usize> {
        let l = self.card_owner()?;
        self.saved.writes.iter().position(|w| w.id == l.id)
    }

    /// Relabel the card for the active setup: the user chose to use another setup's card for it.
    pub fn claim_card(&mut self) {
        let Some(dest) = self.write().dest.clone() else { return };
        if self.busy.is_some() || self.card_missing(&dest) {
            return;
        }
        let label = Label::for_card(self.write(), &dest);
        match label.save(&dest) {
            Ok(()) => {
                self.saved.writes[self.saved.active_write].card_id = Some(label.card);
                self.notice(format!("the card is now “{}”'s", self.write().name));
            }
            Err(e) => self.notice(format!("{e:#}")),
        }
        self.refresh_label();
    }

    /// The notice for a refused sync or take-over, or `None` to go ahead. A card whose setup
    /// isn't on this Mac can be synced: only this setup's own files are ever removed.
    pub fn refused_by_label(&self) -> Option<String> {
        let l = self.card_owner()?;
        self.owner_setup()?;
        Some(format!("this card belongs to “{}”: switch to it, or choose “use it for this setup” in the card tab", l.name))
    }
}

/// Of the labelled cards just plugged in, the setup to bring up: the first that's on this Mac,
/// unless the active setup's own card is among them.
fn bring_up(writes: &[WriteProfile], active: usize, arrived: &[(PathBuf, Label)]) -> Option<usize> {
    let setups: Vec<usize> = arrived.iter().filter_map(|(_, l)| writes.iter().position(|w| w.id == l.id)).collect();
    if setups.contains(&active) { None } else { setups.first().copied() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_plugged_in_brings_up_its_setup() {
        let writes: Vec<WriteProfile> = ["Echo Mini", "Road trip", "Gym"].iter().map(|n| WriteProfile { name: n.to_string(), ..Default::default() }).collect();
        let card = |i: usize, at: &str| (PathBuf::from(at), Label::of(&writes[i]));
        let stranger = (PathBuf::from("/Volumes/Other"), Label { id: "0123456789abcdef".into(), name: "Elsewhere".into(), ..Default::default() });
        assert_eq!(bring_up(&writes, 0, &[card(1, "/Volumes/Untitled")]), Some(1));
        // The active setup's own card wins over another plugged in with it.
        assert_eq!(bring_up(&writes, 0, &[card(2, "/Volumes/Untitled"), card(0, "/Volumes/Untitled 1")]), None);
        // A card from a setup that isn't here brings up nothing; the next one does.
        assert_eq!(bring_up(&writes, 0, &[stranger.clone()]), None);
        assert_eq!(bring_up(&writes, 0, &[stranger, card(2, "/Volumes/GYM")]), Some(2));
        assert_eq!(bring_up(&writes, 0, &[]), None);
    }
}
