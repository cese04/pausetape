//! Playback on a dedicated thread (the OS audio stream isn't `Send`); the UI talks to it with
//! commands and reads a shared `Status`. The thread owns the play queue, so songs follow one
//! another even when the window isn't being drawn.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Sender, unbounded};
use rodio::Source;

use super::decode::{Span, open_span};

enum Cmd {
    Queue(Vec<Item>, usize),
    Jump(usize),
    Toggle,
    Seek(Duration),
    Volume(f32),
    Stop,
}

/// One entry of the play queue: a file, or a song's part of an album file. `None` for an entry
/// nothing here can decode; playback steps over it.
pub type Item = Option<(PathBuf, Option<Span>)>;

#[derive(Clone, Default)]
pub struct Status {
    pub path: Option<PathBuf>,
    /// Position in the queue of what's playing (or last played).
    pub index: Option<usize>,
    pub pos: Duration,
    pub len: Option<Duration>,
    pub paused: bool,
    pub error: Option<String>,
    /// Set while ffmpeg converts a file the built-in decoder can't read.
    pub converting: bool,
    /// Entries stepped over since the UI last asked (`take_skipped`).
    pub skipped: usize,
}

pub struct Audio {
    tx: Sender<Cmd>,
    status: Arc<Mutex<Status>>,
}

/// The playback thread's own state. It owns the queue and moves on to the next song itself, so
/// playback continues while the window isn't drawing (hidden behind other windows, minimised).
struct Queue {
    items: Vec<Item>,
    /// A song is loaded and hasn't reported its end yet.
    live: bool,
}

impl Queue {
    /// Play the first entry from `from` that opens; report where the queue stands.
    fn start(&mut self, from: usize, player: &rodio::Player, shared: &Mutex<Status>, wake: &impl Fn()) {
        player.clear();
        self.live = false;
        let mut skipped = 0;
        for i in from..self.items.len() {
            let Some((path, span)) = self.items[i].clone() else {
                skipped += 1;
                continue;
            };
            let decoded = open_span(&path, span, || {
                let mut st = shared.lock().unwrap();
                (st.converting, st.path, st.index) = (true, Some(path.clone()), Some(i));
                wake();
            });
            let mut st = shared.lock().unwrap();
            match decoded {
                Ok(src) => {
                    let total = st.skipped + skipped;
                    *st = Status { len: src.total_duration(), path: Some(path), index: Some(i), skipped: total, ..Default::default() };
                    // Decoded ahead on a thread of its own: a busy disk mustn't reach the speakers.
                    player.append(super::prefetch::Prefetch::new(src));
                    player.play();
                    self.live = true;
                    wake();
                    return;
                }
                Err(e) => {
                    // Note it and carry on with the next song.
                    st.error = Some(format!("{}: {e}", path.display()));
                    skipped += 1;
                }
            }
        }
        // Nothing left to play: the queue is done.
        let mut st = shared.lock().unwrap();
        let (error, index, total) = (st.error.take(), st.index, st.skipped + skipped);
        *st = Status { error, index, skipped: total, ..Default::default() };
        wake();
    }
}

impl Audio {
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = unbounded::<Cmd>();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        thread::spawn(move || {
            let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(s) => s,
                Err(e) => {
                    shared.lock().unwrap().error = Some(format!("no audio output: {e}"));
                    return;
                }
            };
            let player = rodio::Player::connect_new(sink.mixer());
            let mut queue = Queue { items: Vec::new(), live: false };
            loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Cmd::Queue(items, from)) => {
                        queue.items = items;
                        queue.start(from, &player, &shared, &wake);
                    }
                    Ok(Cmd::Jump(i)) => queue.start(i, &player, &shared, &wake),
                    Ok(Cmd::Toggle) => {
                        if player.is_paused() { player.play() } else { player.pause() }
                    }
                    Ok(Cmd::Seek(to)) => {
                        let _ = player.try_seek(to);
                    }
                    Ok(Cmd::Volume(v)) => player.set_volume(v),
                    Ok(Cmd::Stop) => {
                        player.clear();
                        queue.live = false;
                        let index = shared.lock().unwrap().index;
                        *shared.lock().unwrap() = Status { index, ..Default::default() };
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                }
                {
                    let mut st = shared.lock().unwrap();
                    st.paused = player.is_paused();
                    st.pos = player.get_pos();
                }
                if queue.live && player.empty() {
                    let next = shared.lock().unwrap().index.map_or(0, |i| i + 1);
                    queue.start(next, &player, &shared, &wake);
                }
            }
        });
        Audio { tx, status }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// How many queue entries were stepped over (undecodable, or failed to open) since last asked.
    pub fn take_skipped(&self) -> usize {
        std::mem::take(&mut self.status.lock().unwrap().skipped)
    }

    /// Play `items` from position `from`, then the rest in order, without the UI's help.
    pub fn play_queue(&self, items: Vec<Item>, from: usize) {
        let _ = self.tx.send(Cmd::Queue(items, from));
    }

    /// Play the queue from position `i` (stepping over entries that can't play).
    pub fn jump(&self, i: usize) {
        let _ = self.tx.send(Cmd::Jump(i));
    }
    pub fn toggle(&self) {
        let _ = self.tx.send(Cmd::Toggle);
    }
    pub fn seek(&self, to: Duration) {
        let _ = self.tx.send(Cmd::Seek(to));
    }
    pub fn volume(&self, v: f32) {
        let _ = self.tx.send(Cmd::Volume(v));
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    /// `NEATLIBRARY_PLAY=<song> cargo test -- --ignored queue_advances --nocapture` (plays sound)
    /// Queues two one-second parts of a song, plus an entry that can't play, and checks the
    /// audio thread moves through them with no UI asking it to.
    #[test]
    #[ignore]
    fn queue_advances_without_ui() {
        let Some(song) = std::env::var_os("NEATLIBRARY_PLAY").map(std::path::PathBuf::from) else { return };
        let audio = super::Audio::spawn(|| {});
        let part = |a: u64, b: u64| Some((song.clone(), Some((Duration::from_secs(a), Some(Duration::from_secs(b))))));
        audio.play_queue(vec![part(30, 31), None, part(60, 61)], 0);
        let t = Instant::now();
        let mut seen = Vec::new();
        while t.elapsed() < Duration::from_secs(6) {
            let st = audio.status();
            if seen.last() != Some(&(st.index, st.path.is_some())) {
                seen.push((st.index, st.path.is_some()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("{seen:?}, skipped {}", audio.take_skipped());
        assert!(seen.contains(&(Some(0), true)) && seen.contains(&(Some(2), true)), "played 0, stepped over 1, played 2");
        assert_eq!(seen.last(), Some(&(Some(2), false)), "then stopped at the end of the queue");
    }
}
