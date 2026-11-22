//! Covers for albums without one: an image already in the album's folder, else the front cover
//! of the MusicBrainz release that matches the album, from the Cover Art Archive. Found covers
//! are corrections like edited tags: embedded in card copies at the next Sync (players show
//! embedded covers, not images beside the songs), and in the music files with "write corrected tags".

use super::*;
use crate::musicbrainz::CoverJob;

/// Downloaded covers, one file per album, kept until written into the songs (and after, so a
/// card copy made again still finds it).
fn covers_dir() -> PathBuf {
    crate::platform::data_dir().join("covers")
}

/// Where a cover was found for an album's songs, or why not.
pub struct FoundCover {
    pub album: String,
    pub tracks: Vec<usize>,
    pub result: Result<(PathBuf, &'static str), String>,
}

impl App {
    /// The album has no cover a player would show: none embedded, none waiting to be.
    pub fn lacks_cover(&self, al: &AlbumNode) -> bool {
        al.tracks.iter().all(|&i| !self.tracks[i].embedded_cover && self.tracks[i].cover.is_none())
    }

    /// (artist, album) of every album without a cover.
    pub fn albums_without_cover(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (ai, a) in self.index.artists.iter().enumerate() {
            for (bi, al) in a.albums.iter().enumerate() {
                if self.lacks_cover(al) {
                    out.push((ai, bi));
                }
            }
        }
        out
    }

    /// Find covers for these albums in the background, and add what's found as corrections.
    pub fn find_covers(&mut self, albums: Vec<(usize, usize)>) {
        if self.busy.is_some() || albums.is_empty() {
            return;
        }
        let mut todo = Vec::new();
        for (ai, bi) in albums {
            let a = &self.index.artists[ai];
            let al = &a.albums[bi];
            let name = format!("{} · {}", a.name, al.title.as_deref().unwrap_or("Loose tracks"));
            let job = al.title.as_ref().map(|title| CoverJob { artist: a.name.clone(), album: title.clone(), tracks: self.local_tracks(&al.tracks) });
            todo.push((name, al.tracks.clone(), self.tracks[al.tracks[0]].path.clone(), job));
        }
        let (busy, stop) = Busy::stoppable(format!("finding covers for {}", plural_albums(todo.len())), todo.len());
        self.busy = Some(busy);
        self.cover_misses.clear();
        let ua = self.user_agent();
        self.spawn(move |tx| {
            let total = todo.len();
            let mut found = Vec::new();
            // An image beside the songs needs no network; the rest are looked up one by one.
            let (mut online, mut jobs) = (Vec::new(), Vec::new());
            for (album, tracks, first, job) in todo {
                match (crate::library::cover::beside(&first), job) {
                    (Some(image), _) => found.push(FoundCover { album, tracks, result: Ok((image, "folder")) }),
                    (None, Some(job)) => {
                        online.push((album, tracks, first));
                        jobs.push(job);
                    }
                    (None, None) => found.push(FoundCover { album, tracks, result: Err("no album name to look up".into()) }),
                }
            }
            let _ = tx.send(Msg::WriteProgress(found.len(), total));
            let dir = covers_dir();
            crate::musicbrainz::find_covers(ua, &jobs, &stop, |i, r| {
                let (album, tracks, first) = std::mem::take(&mut online[i]);
                let result = r.and_then(|(bytes, _)| save_cover(&dir, &first, &bytes).map_err(|e| format!("{e:#}"))).map(|p| (p, "archive"));
                found.push(FoundCover { album, tracks, result });
                let _ = tx.send(Msg::WriteProgress(found.len(), total));
            });
            let _ = tx.send(Msg::CoversFound(found, stop.load(Ordering::Relaxed)));
        });
    }

    /// Take the covers found as corrections of their albums' songs.
    pub(super) fn covers_found(&mut self, found: Vec<FoundCover>, stopped: bool) {
        let (mut from_folder, mut from_archive, mut touched) = (0, 0, Vec::new());
        for f in found {
            match f.result {
                Ok((image, from)) => {
                    if from == "folder" {
                        from_folder += 1;
                    } else {
                        from_archive += 1;
                    }
                    for &i in &f.tracks {
                        let t = &mut self.tracks[i];
                        t.cover = Some(image.clone());
                        t.dirty = true;
                        touched.push(i);
                    }
                }
                Err(why) => self.cover_misses.push((f.album, why)),
            }
        }
        if let Some(db) = &mut self.db
            && let Err(e) = db.save_edits(touched.iter().map(|&i| &self.tracks[i]))
        {
            self.notices.push(format!("couldn't save corrections to the library database: {e:#}"));
        }
        self.write_preview = None;
        self.covers_changed(&touched);
        let mut s = format!("{}covers for {}", if stopped { "stopped: " } else { "" }, plural_albums(from_folder + from_archive));
        let mut from = Vec::new();
        if from_folder > 0 {
            from.push(format!("{from_folder} from images in their folders"));
        }
        if from_archive > 0 {
            from.push(format!("{from_archive} from the Cover Art Archive"));
        }
        if !from.is_empty() {
            s.push_str(&format!(" ({})", from.join(", ")));
        }
        if !self.cover_misses.is_empty() {
            s.push_str(&format!(" · {} not found, listed in the library", self.cover_misses.len()));
        }
        if !touched.is_empty() {
            s.push_str(" · they go into card copies at the next Sync, and into the music files with \"write corrected tags\"");
        }
        self.notice(s);
    }

    /// Forget a cover waiting to be embedded in these songs.
    pub fn drop_cover(&mut self, tracks: &[usize]) {
        for &i in tracks {
            self.tracks[i].cover = None;
        }
        if let Some(db) = &mut self.db
            && let Err(e) = db.save_edits(tracks.iter().map(|&i| &self.tracks[i]))
        {
            self.notices.push(format!("couldn't save corrections to the library database: {e:#}"));
        }
        self.write_preview = None;
        self.covers_changed(tracks);
    }
}

/// Keep a downloaded cover, after checking it's an image. Named after the album's first song,
/// so finding it again replaces it.
fn save_cover(dir: &std::path::Path, first: &std::path::Path, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let format = image::guess_format(bytes).map_err(|_| anyhow::anyhow!("the Cover Art Archive sent something that isn't an image"))?;
    let ext = if format == image::ImageFormat::Png { "png" } else { "jpg" };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    first.hash(&mut h);
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{:016x}.{ext}", h.finish()));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn plural_albums(n: usize) -> String {
    if n == 1 { "1 album".into() } else { format!("{n} albums") }
}
