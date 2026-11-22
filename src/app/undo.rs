//! Undoing changes that live only in the app: playlists, and what a card setup puts on its card.
//! ⌘Z (or a notice's "undo") takes back the latest. Changes to files (reorganising the music
//! folder, taking a card over) keep undo commands of their own, so a keystroke never moves files.

use super::*;
use crate::card::Custom;

/// The most changes kept to undo.
const KEEP: usize = 50;

/// How things were before one change.
enum Step {
    /// The playlists, the one open, and each setup's playlists for the card, by setup id (setups
    /// refer to playlists by name, so renaming or deleting one changes them too).
    Playlists { playlists: Vec<Playlist>, open: Option<usize>, on_card: Vec<(String, BTreeSet<String>)> },
    /// What a setup puts on its card, its names and order there, by setup id.
    Chosen { setup: String, custom: Custom },
}

pub struct Undo {
    /// The change, as "undone: …" will say it ("deleting the playlist “Road trip”").
    pub what: String,
    step: Step,
}

impl Undo {
    fn playlists(what: String, saved: &Saved, open: Option<usize>) -> Undo {
        let on_card = saved.writes.iter().map(|w| (w.id.clone(), w.playlists.clone())).collect();
        Undo { what, step: Step::Playlists { playlists: saved.playlists.clone(), open, on_card } }
    }

    fn chosen(what: String, saved: &Saved) -> Undo {
        let w = &saved.writes[saved.active_write];
        Undo { what, step: Step::Chosen { setup: w.id.clone(), custom: w.custom.clone() } }
    }

    /// Put things back as they were. `Ok(true)` when the active setup's music changed; an error
    /// when what it belonged to is gone (a setup deleted since).
    fn restore(self, saved: &mut Saved, open: &mut Option<usize>) -> Result<bool, String> {
        match self.step {
            Step::Playlists { playlists, open: was_open, on_card } => {
                saved.playlists = playlists;
                *open = was_open.filter(|&i| i < saved.playlists.len());
                for (id, names) in on_card {
                    if let Some(w) = saved.writes.iter_mut().find(|w| w.id == id) {
                        w.playlists = names;
                    }
                }
                Ok(false)
            }
            Step::Chosen { setup, custom } => {
                let i = saved.writes.iter().position(|w| w.id == setup).ok_or("its card setup was deleted")?;
                saved.writes[i].custom = custom;
                Ok(i == saved.active_write)
            }
        }
    }
}

impl App {
    /// Call before changing playlists: `what` names the change for "undone: …".
    pub fn remember_playlists(&mut self, what: impl Into<String>) {
        let u = Undo::playlists(what.into(), &self.saved, self.active_playlist);
        self.push_undo(u);
    }

    /// Call before changing what the active setup puts on its card, or its names and order there.
    pub fn remember_chosen(&mut self, what: impl Into<String>) {
        let u = Undo::chosen(what.into(), &self.saved);
        self.push_undo(u);
    }

    fn push_undo(&mut self, u: Undo) {
        self.undo_stack.push(u);
        if self.undo_stack.len() > KEEP {
            self.undo_stack.remove(0);
        }
    }

    /// A notice about a change that can be undone: it offers "undo" while it's the latest.
    pub fn notice_undoable(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.undo_notice = Some(s.clone());
        self.notice(s);
    }

    /// Whether the latest notice offers to undo its change.
    pub fn notice_offers_undo(&self) -> bool {
        self.undo_notice.is_some() && self.undo_notice.as_ref() == self.notices.last() && !self.undo_stack.is_empty()
    }

    /// Take back the latest change to playlists or to what goes on the card.
    pub fn undo_last(&mut self) {
        self.undo_notice = None;
        let Some(u) = self.undo_stack.pop() else {
            self.notice("nothing to undo");
            return;
        };
        let what = u.what.clone();
        match u.restore(&mut self.saved, &mut self.active_playlist) {
            Ok(chosen) => {
                if chosen {
                    self.chosen_changed();
                }
                self.notice(format!("undone: {what}"));
            }
            Err(why) => self.notice(format!("can't undo {what}: {why}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved_with(names: &[&str]) -> Saved {
        let playlists = names.iter().map(|n| Playlist { name: n.to_string(), tracks: vec![PathBuf::from(format!("/m/{n}.flac"))] }).collect();
        let mut s = Saved { playlists, ..Saved::default() };
        s.writes[0].playlists = names.iter().map(|n| n.to_string()).collect();
        s
    }

    #[test]
    fn a_deleted_playlist_comes_back_with_its_place_on_the_card() {
        let mut saved = saved_with(&["Road trip", "Gym"]);
        let u = Undo::playlists("deleting".into(), &saved, Some(1));
        saved.playlists.remove(1);
        saved.writes[0].playlists.remove("Gym");
        let mut open = Some(0);
        assert_eq!(u.restore(&mut saved, &mut open), Ok(false));
        assert_eq!(saved.playlists.len(), 2);
        assert_eq!(saved.playlists[1].name, "Gym");
        assert!(saved.writes[0].playlists.contains("Gym"));
        assert_eq!(open, Some(1));
    }

    #[test]
    fn what_goes_on_the_card_comes_back_for_its_own_setup_only() {
        let mut saved = Saved::default();
        saved.writes.push(WriteProfile::default());
        saved.writes[0].custom.album_names.insert("a".into(), "Before".into());
        let u = Undo::chosen("renaming".into(), &saved);
        saved.writes[0].custom.album_names.insert("a".into(), "After".into());
        saved.active_write = 1;
        let mut open = None;
        assert_eq!(u.restore(&mut saved, &mut open), Ok(false), "another setup is open: nothing to refresh");
        assert_eq!(saved.writes[0].custom.album_names["a"], "Before");

        let u = Undo::chosen("x".into(), &saved);
        saved.writes.remove(1);
        saved.active_write = 0;
        assert!(u.restore(&mut saved, &mut open).is_err(), "its setup is gone");
    }
}
