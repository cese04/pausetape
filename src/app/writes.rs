//! Card writes, reorganising the music folder, and playlist edits.

use super::*;

/// A Sync removing at least this many songs, and most of the setup's songs on the card, asks first.
const MANY: usize = 20;

impl App {
    pub fn preview_tidy(&mut self) {
        self.tidy_plan = Some(card::tidy_plan(&self.tracks, &self.index, &self.saved.tidy));
    }

    pub fn run_tidy(&mut self) {
        let (Some(plan), Some(source)) = (self.tidy_plan.clone(), self.saved.source.clone()) else { return };
        if self.busy.is_some() {
            return;
        }
        if let Some(why) = self.library_unavailable() {
            self.notice(why);
            return;
        }
        let (busy, stop) = Busy::stoppable("tidying source", plan.len());
        self.busy = Some(busy);
        let tracks = self.tracks.clone();
        let opts = self.saved.tidy.clone();
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let report = card::execute(&tracks, &plan, &source, &source, Mode::MoveInPlace, &opts, &[], &stop, |p| {
                // Throttle UI updates to keep the channel small on large libraries.
                if let Progress::Step { done, .. } = &p
                    && done % 10 != 0
                {
                    return;
                }
                let _ = tx.send(Msg::Organize(p));
                ctx.request_repaint();
            });
            let _ = tx.send(Msg::Organize(Progress::Finished(report)));
        });
    }

    pub fn undo(&mut self) {
        match card::undo_latest() {
            Ok((n, errors)) => {
                self.notice(format!("restored {n} files{}", if errors.is_empty() { String::new() } else { format!(", {} errors", errors.len()) }));
                self.scan();
            }
            Err(e) => self.notice(format!("undo: {e:#}")),
        }
    }

    /// Why work that relies on the library can't go ahead now: the music folder can't be reached,
    /// or the library is empty. Against an empty library a Sync would take the setup's songs off
    /// the card, and a take-over or import would treat all of the card as new.
    pub fn library_unavailable(&self) -> Option<String> {
        let Some(src) = &self.saved.source else { return Some("choose your music folder first".into()) };
        if self.source_missing || !src.is_dir() {
            return Some(format!("can't reach the music folder ({}): is its drive connected?", src.display()));
        }
        self.tracks.is_empty().then(|| "the library is empty: rescan the music folder first".into())
    }

    /// A dialog is open over the tab: keys are its own.
    pub fn dialog_open(&self) -> bool {
        self.welcome_open || self.tag_edit.is_some() || self.confirm.is_some() || self.sync_confirm.is_some() || self.picker.is_some() || self.take_over_open || self.player_test_open || self.adding.is_some()
    }

    pub fn write(&self) -> &WriteProfile {
        &self.saved.writes[self.saved.active_write]
    }

    /// The player the active card setup is for, as tested.
    pub fn player(&self) -> crate::card::Player {
        self.write().options.player()
    }

    pub fn write_mut(&mut self) -> &mut WriteProfile {
        self.write_preview = None;
        self.card_survey = None;
        &mut self.saved.writes[self.saved.active_write]
    }

    pub fn select_write(&mut self, i: usize) {
        self.saved.active_write = i.min(self.saved.writes.len() - 1);
        self.write_preview = None;
        self.card_survey = None;
        self.refresh_on_card();
        self.refresh_label();
        self.card_renaming = None;
        self.pattern_draft = None;
    }

    /// Put an album on the active write's card, or take it off. Like everything chosen, it
    /// reaches the card at the next Sync.
    pub fn card_album(&mut self, artist: usize, album: usize, on: bool) {
        let title = self.index.artists[artist].albums[album].title.clone().unwrap_or_else(|| "loose tracks".into());
        self.remember_chosen(if on { format!("sending “{title}” to the card") } else { format!("taking “{title}” off the card") });
        let a = &self.index.artists[artist];
        let ck = self.index.clusters[a.cluster].key.clone();
        let al = &a.albums[album];
        self.saved.writes[self.saved.active_write].custom.set_album(&ck, al, on);
        self.chosen_changed();
    }

    /// Put a whole artist on the card (albums added later too), or take all of it off.
    pub fn card_artist(&mut self, artist: usize, on: bool) {
        let name = self.index.artists[artist].name.clone();
        self.remember_chosen(if on { format!("sending all of {name} to the card") } else { format!("taking all of {name} off the card") });
        let a = &self.index.artists[artist];
        let ck = self.index.clusters[a.cluster].key.clone();
        self.saved.writes[self.saved.active_write].custom.set_artist(&ck, &a.albums, on);
        self.chosen_changed();
    }

    /// After the choice of music changed: the old comparison with the card is stale.
    pub fn chosen_changed(&mut self) {
        self.write_mut();
        self.refresh_on_card();
    }

    pub fn new_write(&mut self, copy_current: bool) {
        let mut w = if copy_current { self.write().clone() } else { WriteProfile::default() };
        if !copy_current {
            // A new card starts empty: music goes on as it's chosen.
            w.custom.chosen = Some(Default::default());
        }
        w.name = format!("{} {}", if copy_current { w.name.as_str() } else { "Card" }, self.saved.writes.len() + 1);
        // A new layout hasn't written anything yet, even if it targets the same card.
        w.id = card::label::new_id();
        w.manifest.clear();
        w.last_written = None;
        self.saved.writes.push(w);
        self.select_write(self.saved.writes.len() - 1);
    }

    /// Delete the card setup with this id (asked first: see `Confirm::DeleteWrite`). The last
    /// one stays.
    pub fn delete_write(&mut self, id: &str) {
        let Some(i) = self.saved.writes.iter().position(|w| w.id == id) else { return };
        if self.saved.writes.len() > 1 {
            let name = self.saved.writes.remove(i).name;
            let active = self.saved.active_write;
            // The one open stays open; if it was this one, the one before it opens.
            self.select_write(if active > i || (active == i && i > 0) { active - 1 } else { active });
            self.notice(format!("deleted the card setup “{name}”"));
        }
    }

    pub fn preview_write(&mut self) {
        let w = self.write().clone();
        let Some(dest) = w.dest.clone() else { return };
        let tracks = self.tracks.clone();
        let prefs = self.saved.prefs.clone();
        let playlists = self.saved.playlists.clone();
        self.busy.get_or_insert(Busy::new("comparing with the card", 0));
        self.spawn(move |tx| {
            // Statting every file on a slow card can take a while, so this runs off the UI thread
            // with its own index (rebuilding one is much cheaper than the card I/O).
            let index = grouping::build(&tracks, &prefs);
            let p = card::preview(&w, &tracks, &index, &playlists, &dest);
            let _ = tx.send(Msg::Previewed(w.id, Box::new(p)));
        });
    }

    /// Look at what's on the card besides this write's files: which are copies of library songs.
    pub fn survey_card(&mut self) {
        let w = self.write().clone();
        let Some(dest) = w.dest.clone() else { return };
        if self.busy.is_some() {
            return;
        }
        // The label as it is now: the card may have been swapped since the last look.
        self.refresh_label();
        if let Some(why) = self.refused_by_label().or_else(|| self.library_unavailable()) {
            self.notice(why);
            return;
        }
        let plan = w.plan(&self.tracks, &self.index);
        let tracks = self.tracks.clone();
        let ctx = self.ctx.clone();
        self.busy = Some(Busy::new("checking the card's music", 0));
        self.spawn(move |tx| {
            let s = card::adopt::survey(&tracks, &plan, &w.manifest, &dest, |done, total| {
                if done % 25 == 0 {
                    let _ = tx.send(Msg::WriteProgress(done, total));
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::Surveyed(w.id, Box::new(s)));
        });
    }

    /// Move the card's copies of library songs into this write's layout (see `card::adopt`).
    pub fn take_over_card(&mut self) {
        let Some((id, survey)) = self.card_survey.take() else { return };
        let Some(dest) = self.write().dest.clone().filter(|_| self.write().id == id) else { return };
        if self.busy.is_some() {
            self.card_survey = Some((id, survey));
            return;
        }
        // The label as it is now: the card may have been swapped since the last look.
        self.refresh_label();
        if let Some(why) = self.refused_by_label().or_else(|| self.library_unavailable()) {
            self.notice(why);
            return;
        }
        let label = card::Label::of(self.write());
        let (gather, clean, clean_mac) = (self.gather_on_take_over, self.delete_clutter_on_take_over, self.write().options.clean_mac_files);
        let ctx = self.ctx.clone();
        self.busy = Some(Busy::new("taking over the card", 0));
        self.problems = None;
        self.spawn(move |tx| {
            let mut out = card::adopt::take_over(&survey, &dest, gather, clean, clean_mac, &id, |done, total| {
                if done % 10 == 0 || done == total {
                    let _ = tx.send(Msg::WriteProgress(done, total));
                    ctx.request_repaint();
                }
            });
            // Taken over, the card is this setup's.
            if let Err(e) = label.save(&dest) {
                out.errors.push(format!("{e:#}"));
            }
            let _ = tx.send(Msg::TookOver(id, Box::new(out)));
        });
    }

    /// Copy the card's music that isn't in the library into the music folder (see
    /// `card::adopt::import`), then rescan so it joins the library.
    pub fn import_from_card(&mut self) {
        let Some((id, survey)) = self.card_survey.take() else { return };
        let (Some(dest), Some(music)) = (self.write().dest.clone(), self.saved.source.clone()) else {
            self.card_survey = Some((id, survey));
            return;
        };
        if self.busy.is_some() {
            self.card_survey = Some((id, survey));
            return;
        }
        if let Some(why) = self.library_unavailable() {
            self.notice(why);
            self.card_survey = Some((id, survey));
            return;
        }
        let ctx = self.ctx.clone();
        let (busy, stop) = Busy::stoppable("copying music from the card", 0);
        self.busy = Some(busy);
        self.problems = None;
        self.spawn(move |tx| {
            let out = card::adopt::import(&survey, &dest, &music, card::adopt::IMPORT_FOLDER, &stop, |done, total| {
                if done % 10 == 0 {
                    let _ = tx.send(Msg::WriteProgress(done, total));
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::ImportedFromCard(Box::new(out)));
        });
    }

    pub fn undo_take_over(&mut self) {
        let w = self.write().clone();
        let Some(dest) = w.dest.clone() else { return };
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::new("putting the card back", 0));
        self.problems = None;
        self.spawn(move |tx| {
            let r = card::adopt::undo_latest(&w.id, &w.name, &dest).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::UndidTakeOver(w.id, r));
        });
    }

    /// The card's space and what this write's choice can use: (space, room). The room is what's
    /// free plus what its own earlier files occupy, plus the copies a take-over of the card (once
    /// checked) would move into place.
    pub fn card_room(&self) -> Option<(card::SpaceInfo, u64)> {
        let w = self.write();
        let info = w.dest.as_deref().and_then(card::space)?;
        let adoptable = self.card_survey.as_ref().filter(|(n, _)| *n == w.id).map(|(_, s)| s.used_bytes()).unwrap_or(0);
        let room = self.room_while_writing.unwrap_or(info.free + w.manifest_bytes() + adoptable);
        Some((info, room))
    }

    pub fn run_write(&mut self) {
        self.sync(false);
    }

    /// Sync after the user said yes to taking most of the card's songs off it.
    pub fn sync_confirmed(&mut self) {
        self.sync_confirm = None;
        self.sync(true);
    }

    fn sync(&mut self, confirmed: bool) {
        let w = self.write().clone();
        let (Some(dest), Some(source)) = (w.dest.clone(), self.saved.source.clone()) else { return };
        if self.busy.is_some() {
            return;
        }
        // The label as it is now: the card may have been swapped since the last look.
        self.refresh_label();
        if let Some(why) = self.refused_by_label().or_else(|| self.library_unavailable()) {
            self.notice(why);
            return;
        }
        let plan = w.plan(&self.tracks, &self.index);
        // Taking most of the card off is what a mistake looks like (music left out by accident, a
        // library half read): ask first.
        if !confirmed && w.prune {
            let (removing, songs) = card::removing(&w, &self.tracks, &plan);
            if removing >= MANY && removing * 2 > songs {
                self.sync_confirm = Some((w.id.clone(), removing, songs));
                return;
            }
        }
        self.room_while_writing = self.card_room().map(|(_, room)| room);
        let (busy, stop) = Busy::stoppable(format!("writing “{}”", w.name), plan.len());
        self.busy = Some(busy);
        self.problems = None;
        let tracks = self.tracks.clone();
        let playlists = self.saved.playlists.clone();
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let out = card::write(&w, &tracks, &plan, &playlists, &source, &dest, &stop, |p| {
                if let Progress::Step { done, total } = p
                    && done % 10 == 0
                {
                    let _ = tx.send(Msg::WriteProgress(done, total));
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::Written(w.id, Box::new(out)));
        });
    }

    pub fn add_to_playlist(&mut self, track: usize) {
        match self.active_playlist {
            Some(i) => self.add_songs_to_playlist(i, &[track]),
            None => self.notice("create a playlist first"),
        }
    }

    /// Add songs to the end of a playlist, and say so (undo takes them back out).
    pub fn add_songs_to_playlist(&mut self, i: usize, songs: &[usize]) {
        let Some(name) = self.saved.playlists.get(i).map(|p| p.name.clone()) else { return };
        let what = match songs {
            [one] => format!("“{}”", self.tracks[*one].display_title()),
            _ => format!("{} songs", songs.len()),
        };
        self.remember_playlists(format!("adding {what} to “{name}”"));
        let ids: Vec<PathBuf> = songs.iter().map(|&t| self.tracks[t].id()).collect();
        self.saved.playlists[i].tracks.extend(ids);
        self.notice_undoable(format!("added {what} to “{name}”"));
    }

    /// A new, empty playlist, named so no other has its name, and open.
    pub fn new_playlist(&mut self) {
        self.remember_playlists("making a new playlist");
        let taken = |n: &str| self.saved.playlists.iter().any(|p| p.name.eq_ignore_ascii_case(n));
        let name = (1..).map(|n| format!("Playlist {n}")).find(|n| !taken(n)).unwrap_or_default();
        self.saved.playlists.push(Playlist { name, tracks: Vec::new() });
        self.active_playlist = Some(self.saved.playlists.len() - 1);
    }

    /// Delete a playlist; a notice offers to undo it. Card setups stop putting it on their cards.
    pub fn delete_playlist(&mut self, i: usize) {
        let Some(name) = self.saved.playlists.get(i).map(|p| p.name.clone()) else { return };
        self.remember_playlists(format!("deleting the playlist “{name}”"));
        self.saved.playlists.remove(i);
        for w in &mut self.saved.writes {
            w.playlists.remove(&name);
        }
        self.active_playlist = self.saved.playlists.len().checked_sub(1).map(|last| i.min(last));
        self.write_preview = None;
        self.notice_undoable(format!("deleted the playlist “{name}”"));
    }

    /// Rename a playlist, and the setups that put it on their cards with it. A name that's empty,
    /// or another playlist's (card folders ignore case), is refused: the old one stays.
    pub fn rename_playlist(&mut self, i: usize, new: &str) -> bool {
        let new = new.trim();
        let Some(old) = self.saved.playlists.get(i).map(|p| p.name.clone()) else { return false };
        if new == old {
            return true;
        }
        if new.is_empty() || self.saved.playlists.iter().enumerate().any(|(j, p)| j != i && p.name.eq_ignore_ascii_case(new)) {
            self.notice(if new.is_empty() { format!("a playlist needs a name; kept “{old}”") } else { format!("there's already a playlist called “{new}”; kept “{old}”") });
            return false;
        }
        self.remember_playlists(format!("renaming “{old}” to “{new}”"));
        self.saved.playlists[i].name = new.to_string();
        for w in &mut self.saved.writes {
            if w.playlists.remove(&old) {
                w.playlists.insert(new.to_string());
            }
        }
        true
    }
}
