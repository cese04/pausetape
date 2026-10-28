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
        self.busy = Some(Busy { label: "tidying source".into(), done: 0, total: plan.len() });
        let tracks = self.tracks.clone();
        let opts = self.saved.tidy.clone();
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let report = card::execute(&tracks, &plan, &source, &source, Mode::MoveInPlace, &opts, &[], |p| {
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
        &mut self.saved.writes[self.saved.active_write]
    }

    pub fn select_write(&mut self, i: usize) {
        self.saved.active_write = i.min(self.saved.writes.len() - 1);
        self.write_preview = None;
        self.card_renaming = None;
    }

    pub fn new_write(&mut self, copy_current: bool) {
        let mut w = if copy_current { self.write().clone() } else { WriteProfile::default() };
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
        self.busy.get_or_insert(Busy { label: "comparing with the card".into(), done: 0, total: 0 });
        self.spawn(move |tx| {
            // Statting every file on a slow card can take a while, so this runs off the UI thread
            // with its own index (rebuilding one is much cheaper than the card I/O).
            let index = grouping::build(&tracks, &prefs);
            let p = card::preview(&w, &tracks, &index, &playlists, &dest);
            let _ = tx.send(Msg::Previewed(w.name, Box::new(p)));
        });
    }

    pub fn run_write(&mut self) {
        let w = self.write().clone();
        let (Some(dest), Some(source)) = (w.dest.clone(), self.saved.source.clone()) else { return };
        if self.busy.is_some() {
            return;
        }
        let plan = w.plan(&self.tracks, &self.index);
        self.busy = Some(Busy { label: format!("writing “{}”", w.name), done: 0, total: plan.len() });
        let tracks = self.tracks.clone();
        let playlists = self.saved.playlists.clone();
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let out = card::write(&w, &tracks, &plan, &playlists, &source, &dest, |p| {
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
