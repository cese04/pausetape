//! Scanning, tag corrections, artist decisions and MusicBrainz lookups.

use super::*;

impl App {
    /// Rescan the music folder, reusing stored tags for files that haven't changed.
    pub fn scan(&mut self) {
        self.scan_with(false);
    }

    /// Ignore the database and open every file again.
    pub fn read_all_again(&mut self) {
        self.scan_with(true);
    }

    fn scan_with(&mut self, force: bool) {
        let Some(root) = self.saved.source.clone() else { return };
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::new("reading tags", 0));
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let mut db = Db::open_default();
            let (tracks, mut stats) = library::scan(&root, db.as_mut().ok(), force, |n| {
                let _ = tx.send(Msg::ScanProgress(n));
                ctx.request_repaint();
            });
            if let Err(e) = db {
                stats.db_error = Some(format!("{e:#}"));
            }
            let _ = tx.send(Msg::Scanned(root, tracks, stats));
        });
    }

    pub fn set_source(&mut self, dir: PathBuf) {
        self.saved.source = Some(dir);
        self.tracks.clear();
        self.queue.clear();
        self.rebuild();
        self.scan();
    }

    pub fn rebuild(&mut self) {
        self.index = grouping::build(&self.tracks, &self.saved.prefs);
        self.tidy_plan = None;
        self.write_preview = None;
        self.in_place = match &self.saved.source {
            Some(src) => card::tidy_plan(&self.tracks, &self.index, &self.saved.tidy)
                .iter()
                .filter(|p| src.join(&p.rel) == self.tracks[p.track].path)
                .count(),
            None => 0,
        };
        // Layouts saved before the card was "what you chose" choose what they used to put there.
        if !self.index.artists.is_empty() {
            let keys: Vec<String> = self.index.artists.iter().map(|a| self.index.clusters[a.cluster].key.clone()).collect();
            for w in &mut self.saved.writes {
                w.custom.choose_from_exclusions(&keys);
            }
        }
        self.refresh_on_card();
    }

    /// Compare the active write's choice with what its manifest says is on the card (see
    /// `CardState`). Cheap enough to run on every change of the choice.
    pub fn refresh_on_card(&mut self) {
        let w = self.write();
        let plan = w.plan(&self.tracks, &self.index);
        let planned: HashMap<usize, &PlanItem> = plan.iter().filter(|p| p.skip.is_none()).map(|p| (p.track, p)).collect();
        // Card files back to tracks: by a plan with every album, for those no longer chosen,
        // then by the plan as chosen, which wins where names coincide.
        let mut by_rel: HashMap<String, usize> = HashMap::new();
        if !w.manifest.is_empty() {
            let mut all = w.custom.clone();
            all.chosen = Some(card::Chosen { artists: self.index.artists.iter().map(|a| self.index.clusters[a.cluster].key.clone()).collect(), albums: Default::default() });
            all.excluded_albums.clear();
            let everything = card::build_plan(&self.tracks, &self.index, &w.options, &all);
            for p in everything.iter().chain(&plan).filter(|p| p.skip.is_none() || p.skip == Some(card::REPEAT)) {
                by_rel.insert(p.rel.to_string_lossy().to_lowercase(), p.track);
            }
        }
        let on_card: HashMap<usize, u64> = w.manifest.iter().filter_map(|m| by_rel.get(&m.rel.to_string_lossy().to_lowercase()).map(|&t| (t, m.size))).collect();
        let mut s = CardState { on_card: on_card.keys().copied().collect(), planned: planned.keys().copied().collect(), ..Default::default() };
        s.planned_bytes = planned.values().map(|p| card::card_size(&self.tracks[p.track], p.target)).sum();
        let ids: Vec<PathBuf> = self.tracks.iter().map(Track::id).collect();
        let by_path = ids.iter().map(PathBuf::as_path).zip(&self.tracks).collect();
        s.planned_bytes += playlist::planned(&w.chosen_playlists(&self.saved.playlists), &by_path, &w.playlists_folder, w.options.rules())
            .iter()
            .flat_map(|(_, copies, _)| copies.iter().map(|c| card::card_size(c.track, c.target)))
            .sum::<u64>();
        let (mut adds, mut removes) = (HashSet::new(), HashSet::new());
        for a in &self.index.artists {
            let ck = &self.index.clusters[a.cluster].key;
            for al in &a.albums {
                let add: Vec<usize> = al.tracks.iter().copied().filter(|t| planned.contains_key(t) && !on_card.contains_key(t)).collect();
                let (repeats, remove): (Vec<usize>, Vec<usize>) = al.tracks.iter().copied().filter(|t| on_card.contains_key(t) && !planned.contains_key(t)).partition(|&t| al.is_repeat(t));
                s.repeats += repeats.len();
                s.remove_bytes += repeats.iter().map(|t| on_card[t]).sum::<u64>();
                s.add_bytes += add.iter().map(|t| card::card_size(&self.tracks[*t], planned[t].target)).sum::<u64>();
                s.remove_bytes += remove.iter().map(|t| on_card[t]).sum::<u64>();
                if !add.is_empty() {
                    adds.insert(al.base_id(ck));
                }
                if !remove.is_empty() {
                    removes.insert(al.base_id(ck));
                }
            }
        }
        s.replaced = adds.intersection(&removes).count();
        (s.added, s.removed) = (adds.len() - s.replaced, removes.len() - s.replaced);
        self.card_state = s;
    }

    pub fn dirty_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.dirty).count()
    }

    /// Corrections that can be written into the music files. Songs inside an album file (CUE)
    /// keep theirs in the app; they reach the card copies, which are separate files.
    pub fn dirty_in_files(&self) -> usize {
        self.tracks.iter().filter(|t| t.dirty && t.cue.is_none()).count()
    }

    pub fn save_tags_to_source(&mut self) {
        let dirty: Vec<(usize, Track)> = self.tracks.iter().enumerate().filter(|(_, t)| t.dirty && t.cue.is_none()).map(|(i, t)| (i, t.clone())).collect();
        if dirty.is_empty() || self.busy.is_some() {
            return;
        }
        let (busy, stop) = Busy::stoppable("writing tags", dirty.len());
        self.busy = Some(busy);
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let mut ok = Vec::new();
            let mut errors = Vec::new();
            let total = dirty.len();
            for (n, (i, t)) in dirty.into_iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if n % 10 == 0 {
                    let _ = tx.send(Msg::WriteProgress(n, total));
                    ctx.request_repaint();
                }
                match library::write_tags(&t, &t.path) {
                    Ok(()) => {
                        let (size, mtime) = std::fs::metadata(&t.path).map(|m| (m.len(), library::mtime_of(&m))).unwrap_or((t.size, t.mtime));
                        ok.push((i, size, mtime));
                    }
                    Err(e) => errors.push(format!("{e:#}")),
                }
            }
            let _ = tx.send(Msg::TagsSaved(ok, errors));
        });
    }

    /// Move extra copies of songs stored twice to the Trash: (extra copy, the copy that stays),
    /// as `AlbumNode::repeats` lists them. Each goes only if it still is the same file as the
    /// copy that stays (same size, same first and last 64 KB). Then the library is read again.
    pub fn trash_repeats(&mut self, pairs: Vec<(usize, usize)>) {
        if pairs.is_empty() || self.busy.is_some() {
            return;
        }
        let files: Vec<(PathBuf, PathBuf)> = pairs.iter().map(|&(c, k)| (self.tracks[c].path.clone(), self.tracks[k].path.clone())).collect();
        let (busy, stop) = Busy::stoppable("moving to the Trash", files.len());
        self.busy = Some(busy);
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let mut errors = Vec::new();
            let mut moved = 0;
            for (n, (copy, keep)) in files.iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let same = || {
                    let (a, b) = (std::fs::metadata(copy).ok()?.len(), std::fs::metadata(keep).ok()?.len());
                    Some(a == b && card::adopt::sample(copy, a).ok()? == card::adopt::sample(keep, b).ok()?)
                };
                match same() {
                    Some(true) => match crate::platform::trash(copy) {
                        Ok(()) => moved += 1,
                        Err(e) => errors.push(format!("{}: {e}", copy.display())),
                    },
                    Some(false) => errors.push(format!("{}: no longer the same as {}, so it stays", copy.display(), keep.display())),
                    None => errors.push(format!("{}: couldn't be read", copy.display())),
                }
                let _ = tx.send(Msg::WriteProgress(n + 1, files.len()));
                ctx.request_repaint();
            }
            let _ = tx.send(Msg::Trashed(moved, errors));
        });
    }

    pub fn merge(&mut self, a: usize, b: usize) {
        let (ka, kb) = (self.index.clusters[a].key.clone(), self.index.clusters[b].key.clone());
        self.saved.prefs.merges.push((ka, kb));
        self.rebuild();
    }

    pub fn reject(&mut self, a: usize, b: usize) {
        let (ka, kb) = (self.index.clusters[a].key.clone(), self.index.clusters[b].key.clone());
        self.saved.prefs.reject(&ka, &kb);
        self.rebuild();
    }

    /// Name a cluster. Any other cluster that already goes by that name joins it, which is how
    /// a MusicBrainz name ("Guns N' Roses") pulls in variants the key couldn't ("Guns and Roses").
    pub fn set_cluster_name(&mut self, c: usize, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let me = self.index.clusters[c].key.clone();
        for k in &self.index.clusters[c].keys {
            self.saved.prefs.names.insert(k.clone(), name.clone());
        }
        let target = artist_key(&name);
        for (i, other) in self.index.clusters.iter().enumerate() {
            if i != c && (other.keys.contains(&target) || other.name == name) {
                self.saved.prefs.merges.push((me.clone(), other.key.clone()));
            }
        }
        self.rebuild();
    }

    /// How the app introduces itself to MusicBrainz, with the user's contact if given.
    pub(super) fn user_agent(&self) -> String {
        let contact = self.saved.mb_contact.trim();
        if contact.is_empty() {
            format!("NeatLibrary/{}", env!("CARGO_PKG_VERSION"))
        } else {
            format!("NeatLibrary/{} ( {contact} )", env!("CARGO_PKG_VERSION"))
        }
    }

    fn mb(&mut self) -> &musicbrainz::Client {
        let ua = self.user_agent();
        if self.mb.as_ref().is_none_or(|(u, _)| *u != ua) {
            let wake = self.ctx.clone();
            let client = musicbrainz::Client::spawn(ua.clone(), self.mb_tx.clone(), move || wake.request_repaint());
            self.mb = Some((ua, client));
        }
        &self.mb.as_ref().unwrap().1
    }

    pub fn lookup_artist(&mut self, c: usize) {
        let cl = &self.index.clusters[c];
        let job = Job::Artist { cluster_key: cl.key.clone(), name: cl.name.clone() };
        self.artist_hits.insert(cl.key.clone(), None);
        self.mb().send(job);
    }

    pub fn lookup_album(&mut self, artist: usize, album: usize) {
        let a = &self.index.artists[artist];
        let al: &AlbumNode = &a.albums[album];
        let id = al.id(&self.index.clusters[a.cluster].key);
        let tracks = self.local_tracks(&al.tracks);
        let job = match &al.title {
            Some(title) => Job::Album { album_id: id.clone(), artist: a.name.clone(), album: title.clone(), tracks },
            None => Job::Loose { album_id: id.clone(), tracks },
        };
        self.lookups.insert(id, Lookup::Pending);
        self.mb().send(job);
    }

    /// Songs as MusicBrainz lookups compare them.
    pub(super) fn local_tracks(&self, idxs: &[usize]) -> Vec<LocalTrack> {
        idxs.iter()
            .map(|&i| {
                let t = &self.tracks[i];
                LocalTrack {
                    idx: i,
                    title: t.display_title(),
                    artist: t.artist.clone().unwrap_or_default(),
                    album_artist: t.album_artist.clone().unwrap_or_default(),
                    album: t.album.clone().unwrap_or_default(),
                    track: t.track,
                    disc: t.disc,
                    year: t.year,
                    seconds: t.duration.as_secs_f64(),
                }
            })
            .collect()
    }

    /// Add the cover image found beside the playing track to the tags of its album's songs (or
    /// edition's), as a correction: it goes into card copies, and into the music files with
    /// "write corrected tags".
    pub fn add_cover(&mut self, track: usize, image: PathBuf) {
        let album = self.index.artists.iter().flat_map(|a| &a.albums).find(|al| al.tracks.contains(&track));
        let songs: Vec<usize> = album.map(|al| al.tracks.clone()).unwrap_or_else(|| vec![track]);
        for &i in &songs {
            let t = &mut self.tracks[i];
            t.cover = Some(image.clone());
            t.dirty = true;
        }
        if let Some(db) = &mut self.db
            && let Err(e) = db.save_edits(songs.iter().map(|&i| &self.tracks[i]))
        {
            self.notices.push(format!("couldn't save corrections to the library database: {e:#}"));
        }
        self.write_preview = None;
        let name = image.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Songs inside an album file (CUE) can't be retagged in place; their card copies can.
        let where_ = if songs.iter().all(|&i| self.tracks[i].cue.is_some()) {
            "in card copies when you next write"
        } else {
            "in card copies when you next write, and in the music files with \"write corrected tags\""
        };
        self.notice(format!("{name} will be added as the cover of {}: {where_}", plural_files(songs.len())));
    }

    pub fn apply_lookup(&mut self, id: &str) {
        let Some(Lookup::Ready { changes, .. }) = self.lookups.remove(id) else { return };
        let mut touched = HashSet::new();
        for ch in changes.into_iter().filter(|c| c.accept) {
            touched.insert(ch.track);
            let t = &mut self.tracks[ch.track];
            let v = ch.new;
            match ch.field {
                Field::Title => t.title = Some(v),
                Field::Artist => t.artist = Some(v),
                Field::AlbumArtist => t.album_artist = Some(v),
                Field::Album => t.album = Some(v),
                Field::Track => t.track = v.parse().ok().or(t.track),
                Field::Disc => t.disc = v.parse().ok().or(t.disc),
                Field::Year => t.year = v.parse().ok().or(t.year),
            }
            t.dirty = true;
        }
        // Keep the corrections across restarts until they're written into the files.
        if let Some(db) = &mut self.db
            && let Err(e) = db.save_edits(touched.iter().map(|&i| &self.tracks[i]))
        {
            self.notices.push(format!("couldn't save corrections to the library database: {e:#}"));
        }
        self.rebuild();
    }
}
