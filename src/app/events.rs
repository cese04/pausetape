//! The per-frame message pump: results from background work are folded into the state here.

use super::*;

impl App {
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::ScanProgress(n) => {
                    if let Some(b) = &mut self.busy {
                        b.progress(n, 0);
                    }
                }
                Msg::Scanned(root, tracks, stats) => {
                    self.busy = None;
                    if Some(&root) != self.saved.source.as_ref() {
                        continue;
                    }
                    let device = self.player();
                    let unplayable = tracks.iter().filter(|t| !device.plays(t)).count();
                    let mut parts = vec![format!("{} songs", tracks.len())];
                    if stats.cached > 0 {
                        parts.push(format!("{} unchanged (from the library database)", plural_files(stats.cached)));
                    }
                    parts.push(format!("{} read from files", stats.read));
                    if stats.removed > 0 {
                        parts.push(format!("{} gone since last time", stats.removed));
                    }
                    if stats.edits > 0 {
                        parts.push(format!("{} unsaved corrections restored", stats.edits));
                    }
                    if unplayable > 0 {
                        parts.push(format!("{unplayable} in formats {} can't play", device.short()));
                    }
                    if let Some(e) = &stats.db_error {
                        parts.push(format!("database unavailable, read everything: {e}"));
                    }
                    self.notice(parts.join(" · "));
                    self.tracks = tracks;
                    self.lookups.clear();
                    self.queue.clear();
                    // Track numbers change with a new scan.
                    self.selected.clear();
                    self.select_anchor = None;
                    self.tag_edit = None;
                    self.rebuild();
                }
                Msg::Organize(Progress::Step { done, total }) => {
                    if let Some(b) = &mut self.busy {
                        b.progress(done, total);
                    }
                }
                Msg::Organize(Progress::Finished(r)) => {
                    self.busy = None;
                    self.notice(format!(
                        "{}{} copied · {} moved · {} unchanged · {} skipped · {} retagged · {} mac files removed{}",
                        if r.stopped { "stopped: " } else { "" },
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
                        b.progress(done, total);
                    }
                }
                Msg::Written(name, out) => {
                    self.busy = None;
                    self.room_while_writing = None;
                    let errors = self.report_problems(&format!("writing “{name}”"), out.errors.clone());
                    if out.stopped {
                        self.notice(format!(
                            "“{name}” stopped: {} copied · {} unchanged · {} removed{errors}. What's on the card is recorded; Sync carries on from there.",
                            out.copied, out.unchanged, out.removed,
                        ));
                    } else {
                        self.notice(format!(
                            "“{name}”: {} copied · {} unchanged{} · {} removed · {} left out · {} retagged · {} mac files cleaned{errors}",
                            out.copied,
                            out.unchanged,
                            if out.renamed > 0 { format!(" · {} renamed", out.renamed) } else { String::new() },
                            out.removed,
                            out.skipped,
                            out.tagged,
                            out.cleaned,
                        ));
                    }
                    if let Some(w) = self.saved.writes.iter_mut().find(|w| w.name == name) {
                        w.manifest = out.manifest;
                        if !out.stopped {
                            w.last_written = Some(card::now());
                        }
                    }
                    self.write_preview = None;
                    self.volumes = card::volumes();
                    self.refresh_on_card();
                    self.refresh_label();
                }
                Msg::Surveyed(name, s) => {
                    self.busy = None;
                    if self.write().name == name {
                        self.card_survey = Some((name, *s));
                    }
                }
                Msg::TookOver(name, mut out) => {
                    self.busy = None;
                    self.take_over_open = false;
                    if let Some(w) = self.saved.writes.iter_mut().find(|w| w.name == name) {
                        // The moved files are now this write's, as if it had copied them.
                        let mut have: HashSet<String> = w.manifest.iter().map(|m| m.rel.to_string_lossy().to_lowercase()).collect();
                        for wr in out.written {
                            if have.insert(wr.rel.to_string_lossy().to_lowercase()) {
                                w.manifest.push(wr);
                            }
                        }
                    }
                    let errors = self.report_problems("taking over the card", std::mem::take(&mut out.errors));
                    let deleted = if out.deleted > 0 { format!(", {} clutter files deleted ({})", out.deleted, crate::ui::text::bytes(out.deleted_bytes)) } else { String::new() };
                    let cleaned = if out.cleaned > 0 { format!(", {} macOS ._ files removed", out.cleaned) } else { String::new() };
                    self.notice(format!("card taken over: {} songs moved into place, {} other files gathered{deleted}{cleaned}{errors}", out.moved, out.gathered));
                    self.refresh_on_card();
                    self.refresh_label();
                    self.preview_write();
                }
                Msg::ImportedFromCard(mut out) => {
                    self.busy = None;
                    let errors = self.report_problems("copying music from the card", std::mem::take(&mut out.errors));
                    self.notice(format!(
                        "{}copied {} files ({}) from the card into “{}” in your music folder{errors}. Check the card again once the library is rescanned: those songs can then be taken over too.",
                        if out.stopped { "stopped: " } else { "" },
                        out.copied,
                        crate::ui::text::bytes(out.bytes),
                        card::adopt::IMPORT_FOLDER
                    ));
                    self.scan();
                }
                Msg::FolderSurveyed(from, s) => {
                    self.busy = None;
                    self.adding = Some(Adding { from, survey: *s, versions: false });
                }
                Msg::Added(mut out) => {
                    self.busy = None;
                    let errors = self.report_problems("adding music", std::mem::take(&mut out.errors));
                    self.notice(format!(
                        "{}added {} files ({}) to “{}” in your music folder{errors}",
                        if out.stopped { "stopped: " } else { "" },
                        out.copied,
                        crate::ui::text::bytes(out.bytes),
                        card::adopt::ADDED_FOLDER
                    ));
                    self.scan();
                }
                Msg::UndidTakeOver(name, r) => {
                    self.busy = None;
                    match r {
                        Ok((released, restored, errors)) => {
                            if let Some(w) = self.saved.writes.iter_mut().find(|w| w.name == name) {
                                let gone: HashSet<String> = released.iter().map(|r| r.to_string_lossy().to_lowercase()).collect();
                                w.manifest.retain(|m| !gone.contains(&m.rel.to_string_lossy().to_lowercase()));
                            }
                            let errors = self.report_problems("putting the card back", errors);
                            self.notice(format!("card put back as it was: {restored} files{errors}"));
                            self.refresh_on_card();
                        }
                        Err(e) => self.notice(format!("undo: {e}")),
                    }
                    self.preview_write();
                }
                Msg::Previewed(name, p) => {
                    if self.busy.as_ref().is_some_and(|b| b.label.starts_with("comparing")) {
                        self.busy = None;
                    }
                    if self.write().name == name {
                        // A card with music the app didn't put there, before any write: explain
                        // the take-over once, on its own.
                        let w = &mut self.saved.writes[self.saved.active_write];
                        let owned = self.dest_label.as_ref().is_some_and(|l| l.id != w.id);
                        if p.foreign > 0 && w.manifest.is_empty() && w.last_written.is_none() && !w.take_over_offered && !owned {
                            w.take_over_offered = true;
                            self.take_over_open = true;
                        }
                        self.write_preview = Some((name, *p));
                    }
                }
                Msg::TestCardWritten(made) => {
                    self.busy = None;
                    let ok = made.iter().filter(|(_, r)| r.is_ok()).count();
                    self.notice(format!("test folder written to the card: {ok} of {} samples · eject the card and try them on the player", made.len()));
                    self.test_card = Some(made);
                    self.write_preview = None;
                }
                Msg::TestCardRemoved(r) => {
                    self.busy = None;
                    match r {
                        Ok(()) => {
                            self.test_card = None;
                            self.notice("test folder taken off the card");
                        }
                        Err(e) => self.notice(format!("couldn't take the test folder off the card: {e}")),
                    }
                }
                Msg::Trashed(moved, what, errors) => {
                    self.busy = None;
                    let note = match errors.first() {
                        None => String::new(),
                        Some(e) => format!(" · {} left alone, e.g. {e}", errors.len()),
                    };
                    let moved = match (moved, what) {
                        (1, w) => format!("1 {w}"),
                        (n, "album copy") => format!("{n} album copies"),
                        (n, _) => plural_files(n),
                    };
                    self.notice(format!("{moved} moved to the Trash{note}"));
                    self.scan();
                }
                Msg::TagsSaved(ok, errors) => {
                    self.busy = None;
                    for &(i, size, mtime) in &ok {
                        let t = &mut self.tracks[i];
                        t.embedded_cover |= t.cover.is_some();
                        (t.dirty, t.size, t.mtime, t.cover) = (false, size, mtime, None);
                    }
                    // A cover now embedded in the playing track: look it up again.
                    if let Some(c) = self.current().filter(|c| ok.iter().any(|&(i, _, _)| i == *c)) {
                        self.load_cover(self.tracks[c].path.clone());
                    }
                    // The corrections live in the files now; the next scan re-reads them.
                    if let Some(db) = &mut self.db {
                        let ids: Vec<PathBuf> = ok.iter().map(|&(i, _, _)| self.tracks[i].id()).collect();
                        let _ = db.drop_edits(ids.iter().map(PathBuf::as_path));
                    }
                    self.notice(format!("tags written to {} files{}", ok.len(), if errors.is_empty() { String::new() } else { format!(" · {} failed", errors.len()) }));
                }
                Msg::Cover(path, img, file) => {
                    if self.current().is_some_and(|i| self.tracks[i].path == path) {
                        let tex = img.map(|img| self.ctx.load_texture("cover", img, egui::TextureOptions::LINEAR));
                        self.cover = Some((path, tex, file));
                    }
                }
                Msg::CoversFound(found, stopped) => {
                    self.busy = None;
                    self.covers_found(found, stopped);
                }
                Msg::Envelope(id, env) => {
                    if self.current().is_some_and(|i| self.tracks[i].id() == id) {
                        self.envelope = Some((id, env));
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
        self.follow_audio();
    }
}
