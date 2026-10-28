//! Playback on a dedicated thread (the OS audio stream isn't `Send`); the UI talks to it with
//! commands and reads a shared `Status`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Sender, unbounded};
use rodio::Source;

use super::decode::{Span, open_span};

enum Cmd {
    Load(PathBuf, Option<Span>),
    Toggle,
    Seek(Duration),
    Volume(f32),
    Stop,
}

#[derive(Clone, Default)]
pub struct Status {
    pub path: Option<PathBuf>,
    pub pos: Duration,
    pub len: Option<Duration>,
    pub paused: bool,
    /// The current track played to its end (the UI advances the queue and clears this).
    pub ended: bool,
    pub error: Option<String>,
    /// Set while ffmpeg converts a file the built-in decoder can't read.
    pub converting: bool,
}

pub struct Audio {
    tx: Sender<Cmd>,
    status: Arc<Mutex<Status>>,
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
            // A track is loaded and hasn't reported its end yet.
            let mut live = false;
            loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Cmd::Load(path, span)) => {
                        player.clear();
                        let decoded = open_span(&path, span, || {
                            *shared.lock().unwrap() = Status { converting: true, path: Some(path.clone()), ..Default::default() };
                            wake();
                        });
                        let mut st = shared.lock().unwrap();
                        match decoded {
                            Ok(src) => {
                                *st = Status { len: src.total_duration(), path: Some(path), ..Default::default() };
                                player.append(src);
                                player.play();
                                live = true;
                            }
                            Err(e) => *st = Status { error: Some(format!("{}: {e}", path.display())), ..Default::default() },
                        }
                    }
                    Ok(Cmd::Toggle) => {
                        if player.is_paused() { player.play() } else { player.pause() }
                    }
                    Ok(Cmd::Seek(to)) => {
                        let _ = player.try_seek(to);
                    }
                    Ok(Cmd::Volume(v)) => player.set_volume(v),
                    Ok(Cmd::Stop) => {
                        player.clear();
                        live = false;
                        *shared.lock().unwrap() = Status::default();
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                }
                let mut st = shared.lock().unwrap();
                st.paused = player.is_paused();
                st.pos = player.get_pos();
                if live && player.empty() {
                    live = false;
                    st.ended = true;
                    wake();
                }
            }
        });
        Audio { tx, status }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn take_ended(&self) -> bool {
        std::mem::take(&mut self.status.lock().unwrap().ended)
    }

    /// Play `path`, or only `span` of it (a song inside an album file).
    pub fn load(&self, path: &Path, span: Option<Span>) {
        let _ = self.tx.send(Cmd::Load(path.to_path_buf(), span));
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
