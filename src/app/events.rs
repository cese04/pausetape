//! The per-frame message pump: results from background work are folded into the state here.

use super::*;

impl App {
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::ScanProgress(n) => {
                    if let Some(b) = &mut self.busy {
                        b.done = n;
                    }
                }
                Msg::Scanned(root, tracks, stats) => {
                    self.busy = None;
                    if Some(&root) != self.saved.source.as_ref() {
                        continue;
                    }
                    let unplayable = tracks.iter().filter(|t| !t.on_recho()).count();
                    let mut parts = vec![format!("{} songs", tracks.len())];
                    if stats.cached > 0 {
                        parts.push(format!("{} unchanged (from the library database)", stats.cached));
                    }
                    parts.push(format!("{} read from files", stats.read));
                    if stats.removed > 0 {
                        parts.push(format!("{} gone since last time", stats.removed));
                    }
                    if stats.edits > 0 {
                        parts.push(format!("{} unsaved corrections restored", stats.edits));
                    }
                    if unplayable > 0 {
                        parts.push(format!("{unplayable} in formats RECHO can't play"));
                    }
                    if let Some(e) = &stats.db_error {
                        parts.push(format!("database unavailable, read everything: {e}"));
                    }
                    self.notice(parts.join(" · "));
                    self.tracks = tracks;
                    self.lookups.clear();
                    self.queue.clear();
                    self.rebuild();
                }
                Msg::Organize(Progress::Step { done, total }) => {
                    self.busy = Some(Busy { label: "tidying source".into(), done, total });
                }
                Msg::Organize(Progress::Finished(r)) => {
                    self.busy = None;
                    self.notice(format!(
                        "{} copied · {} moved · {} unchanged · {} skipped · {} retagged · {} mac files removed{}",
                        r.copied,
                        r.moved,
                        r.unchanged,
                        r.skipped,
                        r.tagged,
                        r.cleaned,
                        if r.errors.is_empty() { String::new() } else { format!(" · {} errors (first: {})", r.errors.len(), r.errors[0]) }
                    ));
                    if !r.moves.is_empty() {
                        playlist::remap(&mut self.saved.playlists, &r.moves);
                        self.scan();
                    }
                    self.tidy_plan = None;
                }
                Msg::WriteProgress(done, total) => {
                    if let Some(b) = &mut self.busy {
                        (b.done, b.total) = (done, total);
                    }
                }
                Msg::Written(name, out) => {
                    self.busy = None;
                    self.notice(format!(
                        "“{name}”: {} copied · {} unchanged · {} removed · {} left out · {} retagged · {} mac files cleaned{}",
                        out.copied,
                        out.unchanged,
                        out.removed,
                        out.skipped,
                        out.tagged,
                        out.cleaned,
                        if out.errors.is_empty() { String::new() } else { format!(" · {} errors (first: {})", out.errors.len(), out.errors[0]) }
                    ));
                    if let Some(w) = self.saved.writes.iter_mut().find(|w| w.name == name) {
                        w.manifest = out.manifest;
                        w.last_written = Some(card::now());
                    }
                    self.write_preview = None;
                    self.volumes = card::volumes();
                }
                Msg::Previewed(name, p) => {
                    if self.busy.as_ref().is_some_and(|b| b.label.starts_with("comparing")) {
                        self.busy = None;
                    }
                    if self.write().name == name {
                        self.write_preview = Some((name, *p));
                    }
                }
                Msg::TagsSaved(ok, errors) => {
                    self.busy = None;
                    for &(i, size, mtime) in &ok {
                        let t = &mut self.tracks[i];
                        (t.dirty, t.size, t.mtime) = (false, size, mtime);
                    }
                    // The corrections live in the files now; the next scan re-reads them.
                    if let Some(db) = &mut self.db {
                        let _ = db.drop_edits(ok.iter().map(|&(i, _, _)| self.tracks[i].path.as_path()));
                    }
                    self.notice(format!("tags written to {} files{}", ok.len(), if errors.is_empty() { String::new() } else { format!(" · {} failed", errors.len()) }));
                }
                Msg::Cover(path, img) => {
                    if self.current().is_some_and(|i| self.tracks[i].path == path) {
                        let tex = img.map(|img| self.ctx.load_texture("cover", img, egui::TextureOptions::LINEAR));
                        self.cover = Some((path, tex));
                    }
                }
                Msg::Envelope(path, env) => {
                    if self.current().is_some_and(|i| self.tracks[i].path == path) {
                        self.envelope = Some((path, env));
                    }
                }
            }
        }
        while let Ok(reply) = self.mb_rx.try_recv() {
            match reply {
                Reply::Artist { cluster_key, result } => {
                    self.artist_hits.insert(cluster_key, Some(result));
                }
                Reply::Album { album_id, result } => {
                    let l = match result {
                        Ok((label, changes)) => Lookup::Ready { label, changes },
                        Err(e) => Lookup::Failed(e),
                    };
                    self.lookups.insert(album_id, l);
                }
            }
        }
        if self.audio.take_ended() {
            self.next();
        }
    }
}
