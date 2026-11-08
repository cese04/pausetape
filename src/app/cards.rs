//! Noticing cards: every few seconds the mounted drives are listed, and a card plugged in with a
//! label (`card::label`) brings up the card setup it belongs to. Ejecting one after a Sync.

use std::path::Path;

use super::*;
use crate::card::Label;

const WATCH_EVERY: Duration = Duration::from_secs(3);

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
            let _ = tx.send(Msg::Ejected(name, crate::platform::eject(&path)));
        });
    }

    pub fn watch_cards(&mut self) {
        #[cfg(debug_assertions)]
        if self.shooter.as_ref().is_some_and(|s| !s.watch) {
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
        self.volumes = card::volumes();
        // Cards that arrive during other work (as at every start, while the library opens) are
        // looked at once it's done: a setup's destination doesn't change under running work.
        if self.busy.is_none() {
            let first = self.mounted.is_none();
            let seen = self.mounted.take().unwrap_or_default();
            let arrived: Vec<(PathBuf, Label)> = self.volumes.iter().filter(|v| !seen.contains(&v.path)).filter_map(|v| Some((v.path.clone(), Label::read(&v.path)?))).collect();
            self.mounted = Some(self.volumes.iter().map(|v| v.path.clone()).collect());
            let bring_up = bring_up(&self.saved.writes, self.saved.active_write, &arrived);
            for (path, label) in arrived {
                self.card_arrived(path, label, bring_up, first);
            }
        }
        self.refresh_label();
        // The music folder's drive is back.
        if self.source_missing && self.busy.is_none() && self.saved.source.as_deref().is_some_and(Path::is_dir) {
            self.scan();
        }
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

    /// Read the label of the active setup's card again.
    pub fn refresh_label(&mut self) {
        self.dest_label = self.write().dest.as_deref().and_then(Label::read);
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
        match Label::of(self.write()).save(&dest) {
            Ok(()) => self.notice(format!("the card is now “{}”'s", self.write().name)),
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
