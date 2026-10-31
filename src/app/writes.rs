//! Card writes, reorganising the music folder, and playlist edits.

use super::*;

impl App {
    pub fn preview_tidy(&mut self) {
        self.tidy_plan = Some(card::tidy_plan(&self.tracks, &self.index, &self.saved.tidy));
    }

    pub fn run_tidy(&mut self) {
        let (Some(plan), Some(source)) = (self.tidy_plan.clone(), self.saved.source.clone()) else { return };
        if self.busy.is_some() {
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

    pub fn write(&self) -> &WriteProfile {
        &self.saved.writes[self.saved.active_write]
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
        self.card_renaming = None;
    }

    /// Put an album on the active write's card, or take it off. Like everything chosen, it
    /// reaches the card at the next Sync.
    pub fn card_album(&mut self, artist: usize, album: usize, on: bool) {
        let a = &self.index.artists[artist];
        let ck = self.index.clusters[a.cluster].key.clone();
        let al = &a.albums[album];
        self.saved.writes[self.saved.active_write].custom.set_album(&ck, al, on);
        self.chosen_changed();
    }

    /// Put a whole artist on the card (albums added later too), or take all of it off.
    pub fn card_artist(&mut self, artist: usize, on: bool) {
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
        w.manifest.clear();
        w.last_written = None;
        self.saved.writes.push(w);
        self.select_write(self.saved.writes.len() - 1);
    }

    pub fn delete_write(&mut self) {
        if self.saved.writes.len() > 1 {
            self.saved.writes.remove(self.saved.active_write);
            self.select_write(self.saved.active_write.saturating_sub(1));
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
            let _ = tx.send(Msg::Previewed(w.name, Box::new(p)));
        });
    }

    /// Look at what's on the card besides this write's files: which are copies of library songs.
    pub fn survey_card(&mut self) {
        let w = self.write().clone();
        let Some(dest) = w.dest.clone() else { return };
        if self.busy.is_some() {
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
            let _ = tx.send(Msg::Surveyed(w.name, Box::new(s)));
        });
    }

    /// Move the card's copies of library songs into this write's layout (see `card::adopt`).
    pub fn take_over_card(&mut self) {
        let Some((name, survey)) = self.card_survey.take() else { return };
        let Some(dest) = self.write().dest.clone().filter(|_| self.write().name == name) else { return };
        if self.busy.is_some() {
            self.card_survey = Some((name, survey));
            return;
        }
        let (gather, clean, clean_mac) = (self.gather_on_take_over, self.delete_clutter_on_take_over, self.write().options.clean_mac_files);
        let ctx = self.ctx.clone();
        self.busy = Some(Busy::new("taking over the card", 0));
        self.problems = None;
        self.spawn(move |tx| {
            let out = card::adopt::take_over(&survey, &dest, gather, clean, clean_mac, &name, |done, total| {
                if done % 10 == 0 || done == total {
                    let _ = tx.send(Msg::WriteProgress(done, total));
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::TookOver(name, Box::new(out)));
        });
    }

    /// Copy the card's music that isn't in the library into the music folder (see
    /// `card::adopt::import`), then rescan so it joins the library.
    pub fn import_from_card(&mut self) {
        let Some((name, survey)) = self.card_survey.take() else { return };
        let (Some(dest), Some(music)) = (self.write().dest.clone(), self.saved.source.clone()) else {
            self.card_survey = Some((name, survey));
            return;
        };
        if self.busy.is_some() {
            self.card_survey = Some((name, survey));
            return;
        }
        let ctx = self.ctx.clone();
        let (busy, stop) = Busy::stoppable("copying music from the card", 0);
        self.busy = Some(busy);
        self.problems = None;
        self.spawn(move |tx| {
            let out = card::adopt::import(&survey, &dest, &music, &stop, |done, total| {
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
            let r = card::adopt::undo_latest(&w.name, &dest).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::UndidTakeOver(w.name, r));
        });
    }

    /// The card's space and what this write's choice can use: (space, room). The room is what's
    /// free plus what its own earlier files occupy, plus the copies a take-over of the card (once
    /// checked) would move into place.
    pub fn card_room(&self) -> Option<(card::SpaceInfo, u64)> {
        let w = self.write();
        let info = w.dest.as_deref().and_then(card::space)?;
        let adoptable = self.card_survey.as_ref().filter(|(n, _)| *n == w.name).map(|(_, s)| s.used_bytes()).unwrap_or(0);
        let room = self.room_while_writing.unwrap_or(info.free + w.manifest_bytes() + adoptable);
        Some((info, room))
    }

    pub fn run_write(&mut self) {
        let w = self.write().clone();
        let (Some(dest), Some(source)) = (w.dest.clone(), self.saved.source.clone()) else { return };
        if self.busy.is_some() {
            return;
        }
        let plan = w.plan(&self.tracks, &self.index);
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
            let _ = tx.send(Msg::Written(w.name, Box::new(out)));
        });
    }

    pub fn add_to_playlist(&mut self, track: usize) {
        let Some(p) = self.active_playlist.and_then(|i| self.saved.playlists.get_mut(i)) else {
            self.notice("create a playlist first");
            return;
        };
        p.tracks.push(self.tracks[track].id());
    }
}
