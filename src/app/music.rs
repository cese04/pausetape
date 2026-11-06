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

    /// The library as the app last read it, without reading the folder again: that's for a new
    /// folder, a rescan, and work that changed its files. A folder never read before is scanned.
    pub fn load(&mut self) {
        let Some(root) = self.saved.source.clone() else { return };
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::new("opening the library", 0));
        let ctx = self.ctx.clone();
        self.spawn(move |tx| {
            let mut db = Db::open_default();
            let (tracks, stats) = match db.as_mut().ok().and_then(|db| library::load(&root, db)) {
                Some(stored) => stored,
                None => read_folder(&root, &mut db, false, &tx, &ctx),
            };
            let _ = tx.send(Msg::Scanned(root, tracks, stats));
        });
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
            let (tracks, stats) = read_folder(&root, &mut db, force, &tx, &ctx);
            let _ = tx.send(Msg::Scanned(root, tracks, stats));
        });
    }

    pub fn set_source(&mut self, dir: PathBuf) {
        // Work running now refers to songs by their place in the library; emptying it under that
        // work would put its results on the wrong songs, or crash.
        if let Some(b) = &self.busy {
            self.notice(format!("the music folder can be changed once “{}” is done", b.label));
            return;
        }
        self.saved.source = Some(dir);
        self.tracks.clear();
        self.queue.clear();
        self.rebuild();
        self.scan();
    }

    pub fn rebuild(&mut self) {
        self.index = grouping::build(&self.tracks, &self.saved.prefs);
        self.library_generation += 1;
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
        let mut on_card: HashMap<usize, u64> = w.manifest.iter().filter_map(|m| by_rel.get(&m.rel.to_string_lossy().to_lowercase()).map(|&t| (t, m.size))).collect();
        // Songs on the card under names the write no longer gives them: Sync moves them.
        let renamed = card::renamed(w, &self.tracks, &plan);
        for &(i, j) in &renamed {
            on_card.entry(plan[j].track).or_insert(w.manifest[i].size);
        }
        let by_hand = match &self.card_found {
            Some((id, dest, found)) if *id == w.id && w.dest.as_ref() == Some(dest) => found.iter().copied().filter(|t| !on_card.contains_key(t)).collect(),
            _ => HashSet::new(),
        };
        let mut s = CardState { on_card: on_card.keys().copied().collect(), planned: planned.keys().copied().collect(), by_hand, renamed: renamed.len(), ..Default::default() };
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
            let (mut ok, mut errors, mut fresh) = (Vec::new(), Vec::new(), Vec::new());
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
                        fresh.extend(library::read_file(&t.path));
                    }
                    Err(e) => errors.push(format!("{e:#}")),
                }
            }
            // What the files hold now, for the next time the library opens without reading them.
            if let Ok(mut db) = Db::open_default() {
                let _ = db.update(&fresh, &[]);
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
            let _ = tx.send(Msg::Trashed(moved, "file", errors));
        });
    }

    /// Move album copies that another copy covers (`Edition::superseded`) to the Trash: each
    /// copy's folder when it holds nothing else musical, else just its audio files (with their
    /// CUE sheets). Only while the copy that covers it is still there.
    pub fn trash_copies(&mut self, albums: Vec<(usize, usize)>) {
        if albums.is_empty() || self.busy.is_some() {
            return;
        }
        // (files of the copy, files of the copy that covers it)
        let mut copies: Vec<(Vec<PathBuf>, Vec<PathBuf>)> = Vec::new();
        for (ai, bi) in albums {
            let a = &self.index.artists[ai];
            let ck = &self.index.clusters[a.cluster].key;
            let al = &a.albums[bi];
            let Some(s) = al.edition.as_ref().and_then(|e| e.superseded.as_ref()) else { continue };
            let Some(better) = a.albums.iter().find(|o| o.base_id(ck) == al.base_id(ck) && o.edition.as_ref().is_some_and(|e| e.rank == s.rank)) else { continue };
            let files = |idxs: &[usize]| {
                let mut f: Vec<PathBuf> = idxs.iter().flat_map(|&i| std::iter::once(self.tracks[i].path.clone()).chain(self.tracks[i].cue.as_ref().map(|c| c.sheet.clone()))).collect();
                f.sort();
                f.dedup();
                f
            };
            copies.push((files(&al.tracks), files(&better.tracks)));
        }
        let total = copies.len();
        let (busy, stop) = Busy::stoppable("moving copies to the Trash", total);
        self.busy = Some(busy);
        self.spawn(move |tx| {
            let (mut moved, mut errors) = (0, Vec::new());
            for (n, (files, keep)) in copies.iter().enumerate() {
                let failed = errors.len();
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(gone) = keep.iter().find(|k| !k.exists()) {
                    errors.push(format!("{}: the copy that covers it is missing {}, so it stays", files[0].display(), gone.display()));
                    continue;
                }
                for t in trash_targets(files) {
                    if let Err(e) = crate::platform::trash(&t) {
                        errors.push(format!("{}: {e}", t.display()));
                    }
                }
                moved += (errors.len() == failed) as usize;
                let _ = tx.send(Msg::WriteProgress(n + 1, total));
            }
            let _ = tx.send(Msg::Trashed(moved, "album copy", errors));
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

    /// Rename artist `c` in the songs' tags: wherever their artist or album artist is one of the
    /// names it goes by ("Flyng Lotus feat. Thundercat" keeps its guest). The changes are
    /// corrections like any other, written into the files and card copies when tags are saved.
    pub fn rename_artist(&mut self, c: usize, name: String) {
        let name = name.trim().to_string();
        let cl = &self.index.clusters[c];
        if name.is_empty() || name == cl.name || [VARIOUS, UNKNOWN].iter().any(|n| artist_key(n) == cl.key) {
            return;
        }
        let (old_key, keys) = (cl.key.clone(), cl.keys.clone());
        let names: HashSet<String> = cl.variants.keys().cloned().collect();
        let songs: Vec<usize> = self.index.artists.iter().filter(|a| a.cluster == c).flat_map(|a| a.albums.iter().flat_map(|al| al.tracks.iter().copied())).collect();
        let touched = rename_in_tags(&mut self.tracks, &songs, &names, &name);
        // A name given in the app before renames went into the tags would hide the new one.
        for k in &keys {
            self.saved.prefs.names.remove(k);
        }
        if let Some(db) = &mut self.db
            && let Err(e) = db.save_edits(touched.iter().map(|&i| &self.tracks[i]))
        {
            self.notices.push(format!("couldn't save corrections to the library database: {e:#}"));
        }
        // What MusicBrainz said follows the artist to its new name.
        let new_key = artist_key(&name);
        if let Some(ArtistState::Found { info, fetched }) = self.artist_info.get(&old_key) {
            let (info, fetched) = (info.clone(), *fetched);
            if let Some(db) = &mut self.db {
                let _ = db.save_artist_info(&new_key, &info, fetched);
            }
            self.artist_info.insert(new_key, ArtistState::Found { info, fetched });
        }
        self.notice(format!("renamed “{name}” in the tags of {}; written into the files when you write corrected tags", plural_files(touched.len())));
        self.rebuild();
    }

    /// How the app introduces itself to MusicBrainz, with the user's contact if given.
    pub(super) fn user_agent(&self) -> String {
        let contact = self.saved.mb_contact.trim();
        if contact.is_empty() {
            format!("Pausetape/{}", env!("CARGO_PKG_VERSION"))
        } else {
            format!("Pausetape/{} ( {contact} )", env!("CARGO_PKG_VERSION"))
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

    /// The artist panel's facts and links for artist `c`: from the library database if they were
    /// asked for before, else from MusicBrainz.
    pub fn want_artist_info(&mut self, c: usize) {
        self.artist_from_db(c);
        if matches!(self.artist_info.get(&self.index.clusters[c].key), Some(ArtistState::Unasked)) {
            self.ask_artist_info(c, None);
        }
    }

    /// What the library database holds about artist `c`, without asking MusicBrainz.
    pub fn artist_from_db(&mut self, c: usize) {
        let key = self.index.clusters[c].key.clone();
        if !self.artist_info.contains_key(&key) {
            let stored = self.db.as_ref().and_then(|db| db.artist_info(&key).ok().flatten());
            self.artist_info.insert(key, stored.map_or(ArtistState::Unasked, |(info, fetched)| ArtistState::Found { info, fetched }));
        }
    }

    /// Ask MusicBrainz about artist `c`: the entry `mbid` (one chosen, or the same again), or
    /// the one artist its name matches.
    pub fn ask_artist_info(&mut self, c: usize, mbid: Option<String>) {
        let cl = &self.index.clusters[c];
        let job = Job::ArtistInfo { cluster_key: cl.key.clone(), name: cl.name.clone(), mbid };
        self.artist_info.insert(cl.key.clone(), ArtistState::Asking);
        self.mb().send(job);
    }

    /// The MusicBrainz artists going by artist `c`'s name, to choose another from.
    pub fn choose_artist(&mut self, c: usize) {
        let cl = &self.index.clusters[c];
        let job = Job::Artist { cluster_key: cl.key.clone(), name: cl.name.clone() };
        self.artist_info.insert(cl.key.clone(), ArtistState::Asking);
        self.mb().send(job);
    }

    pub fn lookup_album(&mut self, artist: usize, album: usize) {
        let a = &self.index.artists[artist];
        let al: &AlbumNode = &a.albums[album];
        let id = al.id(&self.index.clusters[a.cluster].key);
        let tracks = self.local_tracks(&al.tracks);
        let ids = al.tracks.iter().map(|&i| self.tracks[i].id()).collect();
        let job = match &al.title {
            Some(title) => Job::Album { album_id: id.clone(), artist: a.name.clone(), album: title.clone(), tracks },
            None => Job::Loose { album_id: id.clone(), tracks },
        };
        self.lookups.insert(id, Lookup::Pending(ids));
        self.mb().send(job);
    }

    /// Songs as MusicBrainz lookups compare them, numbered by their place in `idxs`: a rescan can
    /// renumber the library before the reply comes back (see `Msg` handling of `Reply::Album`).
    pub(super) fn local_tracks(&self, idxs: &[usize]) -> Vec<LocalTrack> {
        idxs.iter()
            .enumerate()
            .map(|(n, &i)| {
                let t = &self.tracks[i];
                LocalTrack {
                    idx: n,
                    title: t.display_title(),
                    artist: t.artist.clone().unwrap_or_default(),
                    album_artist: t.album_artist.clone().unwrap_or_default(),
                    album: t.album.clone().unwrap_or_default(),
                    track: t.track,
                    disc: t.disc,
                    year: t.year,
                    genres: t.genres.as_deref().unwrap_or_default().join("; "),
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
        self.covers_changed(&songs);
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
                Field::Genre => t.genres = Some(crate::library::genres::split([v.as_str()])),
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

/// Put `name` in place of any of `names` at the start of the songs' artist and album artist,
/// keeping what follows ("feat. …", other artists of a multi-value tag). The songs changed.
fn rename_in_tags(tracks: &mut [Track], songs: &[usize], names: &HashSet<String>, name: &str) -> Vec<usize> {
    let renamed = |field: &str| {
        let primary = primary_artist(field);
        let at = field.find(primary.as_str())?;
        (names.contains(&primary) && primary != name).then(|| format!("{name}{}", &field[at + primary.len()..]))
    };
    let mut touched = Vec::new();
    for &i in songs {
        let t = &mut tracks[i];
        let mut changed = false;
        for field in [&mut t.album_artist, &mut t.artist] {
            if let Some(new) = field.as_deref().and_then(renamed) {
                *field = Some(new);
                changed = true;
            }
        }
        if changed {
            t.dirty = true;
            touched.push(i);
        }
    }
    touched
}

/// Scan `root`, saying so in the stats if the database couldn't be opened.
fn read_folder(root: &std::path::Path, db: &mut anyhow::Result<Db>, force: bool, tx: &Sender<Msg>, ctx: &egui::Context) -> (Vec<Track>, library::ScanStats) {
    let (tracks, mut stats) = library::scan(root, db.as_mut().ok(), force, |n| {
        let _ = tx.send(Msg::ScanProgress(n));
        ctx.request_repaint();
    });
    if let Err(e) = db {
        stats.db_error = Some(format!("{e:#}"));
    }
    (tracks, stats)
}

/// Suggestions for the songs of a request (`ids`, by place) as suggestions for the library's songs
/// now, found by id. Songs no longer in the library lose theirs.
pub(super) fn in_library(changes: Vec<FieldChange>, ids: &[PathBuf], tracks: &[Track]) -> Vec<FieldChange> {
    let now: HashMap<PathBuf, usize> = tracks.iter().enumerate().map(|(i, t)| (t.id(), i)).collect();
    changes
        .into_iter()
        .filter_map(|mut c| {
            c.track = *ids.get(c.track).and_then(|id| now.get(id))?;
            Some(c)
        })
        .collect()
}

/// What to move to the Trash to remove an album copy made of `files`: each folder of it whole
/// when everything musical in it (and no subfolder) is the copy's, else just its files there.
fn trash_targets(files: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<&std::path::Path> = files.iter().filter_map(|f| f.parent()).collect();
    dirs.sort();
    dirs.dedup();
    let mut out = Vec::new();
    for dir in dirs {
        let own = |e: &std::fs::DirEntry| {
            let p = e.path();
            let audio = p.extension().is_some_and(|x| crate::library::AUDIO.contains(&x.to_string_lossy().to_lowercase().as_str()));
            !p.is_dir() && (!audio || files.contains(&p))
        };
        if std::fs::read_dir(dir).is_ok_and(|rd| rd.flatten().all(|e| own(&e))) {
            out.push(dir.to_path_buf());
        } else {
            out.extend(files.iter().filter(|f| f.parent() == Some(dir)).cloned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_an_artist_changes_its_tags() {
        let song = |album_artist: Option<&str>, artist: &str, album: &str| Track {
            path: format!("{album}/{artist}.flac").into(),
            album_artist: album_artist.map(String::from),
            artist: Some(artist.into()),
            album: Some(album.into()),
            ..Default::default()
        };
        let mut tracks = vec![
            song(Some("Flyng Lotus"), "Flyng Lotus", "Cosmogramma"),
            song(Some("Flyng Lotus"), "Flyng Lotus feat. Thundercat", "Cosmogramma"),
            song(None, "Flyng Lotus; Erykah Badu", "Until the Quiet Comes"),
            song(None, "Thundercat", "Drunk"),
        ];
        let names: HashSet<String> = ["Flyng Lotus".to_string()].into();
        let touched = rename_in_tags(&mut tracks, &[0, 1, 2, 3], &names, "Flying Lotus");
        assert_eq!(touched, [0, 1, 2], "the guest's own album stays as it is");
        let fields: Vec<(Option<&str>, Option<&str>)> = tracks.iter().map(|t| (t.album_artist.as_deref(), t.artist.as_deref())).collect();
        assert_eq!(fields, [
            (Some("Flying Lotus"), Some("Flying Lotus")),
            (Some("Flying Lotus"), Some("Flying Lotus feat. Thundercat")),
            (None, Some("Flying Lotus; Erykah Badu")),
            (None, Some("Thundercat")),
        ]);
        assert!(tracks[..3].iter().all(|t| t.dirty) && !tracks[3].dirty);
        // The library groups them under the new name, without any decision kept for it.
        let index = grouping::build(&tracks, &ArtistPrefs::default());
        let names: Vec<&str> = index.artists.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Flying Lotus", "Thundercat"]);
    }

    #[test]
    fn suggestions_follow_their_songs_through_a_rescan() {
        let song = |p: &str| Track { path: p.into(), ..Default::default() };
        let change = |track: usize, new: &str| FieldChange { track, field: Field::Title, old: String::new(), new: new.into(), accept: true };
        // Asked about a.flac and b.flac; meanwhile a rescan put other songs first and lost a.flac.
        let ids = vec![PathBuf::from("a.flac"), PathBuf::from("b.flac")];
        let now = vec![song("new.flac"), song("other.flac"), song("b.flac")];
        let got = in_library(vec![change(0, "A"), change(1, "B")], &ids, &now);
        assert_eq!(got.iter().map(|c| (c.track, c.new.as_str())).collect::<Vec<_>>(), vec![(2, "B")]);
    }

    #[test]
    fn a_covered_copy_goes_with_its_folder_unless_it_shares_it() {
        let root = std::env::temp_dir().join(format!("pausetape-test-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (own, shared) = (root.join("Album (MP3)"), root.join("Mixed"));
        for d in [&own, &shared] {
            std::fs::create_dir_all(d).unwrap();
        }
        for f in [own.join("01.mp3"), own.join("02.mp3"), own.join("cover.jpg"), shared.join("01.mp3"), shared.join("other album.flac")] {
            std::fs::write(f, b"x").unwrap();
        }
        let files = vec![own.join("01.mp3"), own.join("02.mp3"), shared.join("01.mp3")];
        assert_eq!(trash_targets(&files), vec![own.clone(), shared.join("01.mp3")], "the folder that's all its own goes whole, cover and all");
        std::fs::create_dir_all(own.join("Scans")).unwrap();
        assert_eq!(trash_targets(&files[..2]), vec![own.join("01.mp3"), own.join("02.mp3")], "a subfolder could hold anything: only the songs go");
        std::fs::remove_dir_all(root).unwrap();
    }
}
