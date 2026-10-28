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
        self.busy = Some(Busy { label: "reading tags".into(), done: 0, total: 0 });
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
            Some(src) => card::build_plan(&self.tracks, &self.index, &self.saved.tidy, &Custom::default())
                .iter()
                .filter(|p| src.join(&p.rel) == self.tracks[p.track].path)
                .count(),
            None => 0,
        };
    }

    pub fn dirty_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.dirty).count()
    }

    pub fn save_tags_to_source(&mut self) {
        let dirty: Vec<(usize, Track)> = self.tracks.iter().enumerate().filter(|(_, t)| t.dirty).map(|(i, t)| (i, t.clone())).collect();
        if dirty.is_empty() || self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy { label: "writing tags".into(), done: 0, total: dirty.len() });
        self.spawn(move |tx| {
            let mut ok = Vec::new();
            let mut errors = Vec::new();
            for (i, t) in dirty {
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

    fn mb(&mut self) -> &musicbrainz::Client {
        let contact = self.saved.mb_contact.trim();
        let ua = if contact.is_empty() {
            format!("NeatLibrary/{}", env!("CARGO_PKG_VERSION"))
        } else {
            format!("NeatLibrary/{} ( {contact} )", env!("CARGO_PKG_VERSION"))
        };
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
        let tracks: Vec<LocalTrack> = al
            .tracks
            .iter()
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
            .collect();
        let job = match &al.title {
            Some(title) => Job::Album { album_id: id.clone(), artist: a.name.clone(), album: title.clone(), tracks },
            None => Job::Loose { album_id: id.clone(), tracks },
        };
        self.lookups.insert(id, Lookup::Pending);
        self.mb().send(job);
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
