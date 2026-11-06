//! Lyrics for the details panel: asked for when a song's are shown, or an album's at once.
//! One worker, one song at a time: the song's own (a `.lrc` beside it, its tags), else what the
//! library database keeps, else LRCLIB's, kept in the database once found (or not found), so
//! they're there offline after.

use super::*;
use crate::lyrics::{self, Lyrics};

/// A song's lyrics, as far as they're known.
#[derive(Clone)]
pub enum LyricsState {
    Looking,
    Found(Lyrics),
    /// Looked for, none found (nor in the song).
    None,
    Failed(String),
}

/// Songs to find lyrics for: (track id, file, what to ask LRCLIB).
pub struct LyricsJob {
    songs: Vec<(PathBuf, PathBuf, lyrics::Query)>,
    /// Ask again, past what's kept ("Try again").
    again: bool,
    /// For an album or a selection: counted in the status bar, and stoppable.
    stop: Option<Arc<AtomicBool>>,
}

impl App {
    /// The lyrics of song `t`: what's known, asking for them the first time.
    pub fn lyrics_of(&mut self, t: usize) -> &LyricsState {
        let id = self.tracks[t].id();
        if !self.lyrics.contains_key(&id) {
            self.lyrics.insert(id.clone(), LyricsState::Looking);
            let job = LyricsJob { songs: vec![self.lyrics_song(t)], again: false, stop: None };
            self.send_lyrics(job);
        }
        &self.lyrics[&id]
    }

    /// Look for song `t`'s lyrics again, on LRCLIB too.
    pub fn lyrics_again(&mut self, t: usize) {
        let id = self.tracks[t].id();
        self.lyrics.insert(id, LyricsState::Looking);
        let job = LyricsJob { songs: vec![self.lyrics_song(t)], again: true, stop: None };
        self.send_lyrics(job);
    }

    /// Find the lyrics of these songs (an album, a selection) in the background.
    pub fn find_lyrics(&mut self, songs: &[usize]) {
        if self.busy.is_some() {
            self.notice("finding lyrics waits until the work under way is done");
            return;
        }
        let todo: Vec<usize> = songs.iter().copied().filter(|&t| !matches!(self.lyrics.get(&self.tracks[t].id()), Some(LyricsState::Found(_) | LyricsState::None))).collect();
        if todo.is_empty() {
            self.notice("their lyrics are already found (or known not to be there)");
            return;
        }
        let (busy, stop) = Busy::stoppable(format!("finding lyrics for {}", plural_songs(todo.len())), todo.len());
        self.busy = Some(busy);
        for &t in &todo {
            self.lyrics.insert(self.tracks[t].id(), LyricsState::Looking);
        }
        let job = LyricsJob { songs: todo.iter().map(|&t| self.lyrics_song(t)).collect(), again: false, stop: Some(stop) };
        self.send_lyrics(job);
    }

    fn lyrics_song(&self, t: usize) -> (PathBuf, PathBuf, lyrics::Query) {
        let tr = &self.tracks[t];
        let query = lyrics::Query {
            artist: tr.artist.clone().or(tr.album_artist.clone()).unwrap_or_default(),
            title: tr.display_title(),
            album: tr.album.clone().unwrap_or_default(),
            seconds: tr.duration.as_secs_f64().round() as u64,
        };
        (tr.id(), tr.path.clone(), query)
    }

    /// The worker, started the first time lyrics are wanted.
    fn send_lyrics(&mut self, job: LyricsJob) {
        if self.lyrics_tx.is_none() {
            let (jobs_tx, jobs) = unbounded::<LyricsJob>();
            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
            let user_agent = format!("Pausetape/{} (https://github.com/cese04/pausetape)", env!("CARGO_PKG_VERSION"));
            thread::spawn(move || lyrics_worker(jobs, tx, ctx, user_agent));
            self.lyrics_tx = Some(jobs_tx);
        }
        if let Some(tx) = &self.lyrics_tx {
            let _ = tx.send(job);
        }
    }

    /// A song's lyrics, from the worker.
    pub(super) fn lyrics_found(&mut self, id: PathBuf, found: Result<Option<Lyrics>, String>) {
        let state = match found {
            Ok(Some(l)) => LyricsState::Found(l),
            Ok(None) => LyricsState::None,
            Err(e) => LyricsState::Failed(e),
        };
        self.lyrics.insert(id, state);
    }
}

fn plural_songs(n: usize) -> String {
    if n == 1 { "1 song".into() } else { format!("{n} songs") }
}

fn lyrics_worker(jobs: Receiver<LyricsJob>, tx: Sender<Msg>, ctx: egui::Context, user_agent: String) {
    let agent = lyrics::agent();
    let mut db = Db::open_default().ok();
    for job in jobs {
        let total = job.songs.len();
        let (mut found, mut none, mut failed) = (0, 0, 0);
        for (n, (id, path, query)) in job.songs.into_iter().enumerate() {
            if job.stop.as_ref().is_some_and(|s| s.load(Ordering::Relaxed)) {
                // Not looked for: not known either way.
                let _ = tx.send(Msg::Lyrics(id, Err("stopped before its turn".into())));
                continue;
            }
            if job.stop.is_some() {
                let _ = tx.send(Msg::WriteProgress(n, total));
            }
            // The song's own first, as they are now (a `.lrc` put beside it later wins).
            let kept = || if job.again { None } else { db.as_ref().and_then(|db| db.lyrics(&id).ok().flatten()) };
            let result = match lyrics::local(&path).map(Some).or_else(kept) {
                Some(k) => Ok(k),
                None => {
                    lyrics::pause();
                    let looked = lyrics::fetch(&agent, &user_agent, &query).map_err(|e| format!("{e:#}"));
                    // Kept found or not found, not failed (it's asked again next time).
                    if let (Ok(l), Some(db)) = (&looked, db.as_mut()) {
                        let _ = db.save_lyrics(&id, l.as_ref(), crate::card::now());
                    }
                    looked
                }
            };
            match &result {
                Ok(Some(_)) => found += 1,
                Ok(None) => none += 1,
                Err(_) => failed += 1,
            }
            let _ = tx.send(Msg::Lyrics(id, result));
            ctx.request_repaint();
        }
        if job.stop.is_some() {
            let _ = tx.send(Msg::LyricsDone(found, none, failed));
            ctx.request_repaint();
        }
    }
}
