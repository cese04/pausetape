//! Decoding ahead of the audio output. A source played as it is gets read from disk and decoded
//! inside the output's callback, as the sound card asks for samples, so a disk slow to answer (an
//! external drive also being read by a card write) is heard as a dropout. `Prefetch` decodes on
//! a thread of its own, a few seconds ahead; the callback only takes what's ready.

use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded, select};
use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};

/// Samples per chunk handed over (all channels), rounded down to whole frames.
const CHUNK: usize = 4096;
/// How far ahead the decoder runs.
const AHEAD: Duration = Duration::from_secs(8);

/// Decoded samples, from the decoder's seek number `generation`; empty at the end of the source.
struct Chunk {
    generation: u64,
    samples: Vec<Sample>,
}

enum Cmd {
    Seek { to: Duration, generation: u64, done: Sender<Result<(), SeekError>> },
}

pub struct Prefetch {
    chunks: Receiver<Chunk>,
    cmds: Sender<Cmd>,
    /// The chunk being played. Never empty until the end, so the output always knows how many
    /// samples are left before the next song may change the rate or channels.
    current: std::vec::IntoIter<Sample>,
    /// Chunks from before the last seek are dropped.
    generation: u64,
    ended: bool,
    channels: ChannelCount,
    rate: SampleRate,
    total: Option<Duration>,
}

impl Prefetch {
    pub fn new<S: Source + Send + 'static>(mut inner: S) -> Self {
        let (channels, rate, total) = (inner.channels(), inner.sample_rate(), inner.total_duration());
        let per_sec = rate.get() as usize * channels.get() as usize;
        let chunk_len = (CHUNK / channels.get() as usize).max(1) * channels.get() as usize;
        let (chunk_tx, chunks) = bounded::<Chunk>((per_sec * AHEAD.as_secs() as usize / chunk_len).max(2));
        let (cmds, cmd_rx) = bounded::<Cmd>(1);
        thread::spawn(move || {
            let mut generation = 0;
            let mut finished = false;
            loop {
                if finished {
                    // At the end, wait for a seek back (or for the player to let go).
                    match cmd_rx.recv() {
                        Ok(cmd) => finished = !seek(&mut inner, cmd, &mut generation),
                        Err(_) => return,
                    }
                    continue;
                }
                let samples: Vec<Sample> = inner.by_ref().take(chunk_len).collect();
                finished = samples.len() < chunk_len;
                // The end is marked by an empty chunk.
                let end = (finished && !samples.is_empty()).then(|| Chunk { generation, samples: Vec::new() });
                for chunk in std::iter::once(Chunk { generation, samples }).chain(end) {
                    select! {
                        send(chunk_tx, chunk) -> sent => if sent.is_err() { return },
                        recv(cmd_rx) -> cmd => match cmd {
                            // A seek while the buffer is full: what's decoded is stale anyway.
                            Ok(cmd) => {
                                if seek(&mut inner, cmd, &mut generation) {
                                    finished = false;
                                    break;
                                }
                            }
                            Err(_) => return,
                        },
                    }
                }
            }
        });
        let mut p = Prefetch { chunks, cmds, current: Vec::new().into_iter(), generation: 0, ended: false, channels, rate, total };
        p.fill();
        p
    }

    /// Take the next chunk of the current seek, waiting for it if need be.
    fn fill(&mut self) {
        loop {
            match self.chunks.recv() {
                Ok(chunk) if chunk.generation != self.generation => continue,
                Ok(chunk) if !chunk.samples.is_empty() => self.current = chunk.samples.into_iter(),
                // An empty chunk marks the end; a decoder thread that's gone, too.
                _ => self.ended = true,
            }
            return;
        }
    }
}

/// Carry out a seek on the decoder and say how it went. Done, what follows has the seek's
/// number; failed, nothing changes and what's decoded ahead still plays.
fn seek<S: Source>(inner: &mut S, cmd: Cmd, generation: &mut u64) -> bool {
    let Cmd::Seek { to, generation: g, done } = cmd;
    let res = inner.try_seek(to);
    let ok = res.is_ok();
    if ok {
        *generation = g;
    }
    let _ = done.send(res);
    ok
}

impl Iterator for Prefetch {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let s = self.current.next()?;
        if self.current.len() == 0 && !self.ended {
            // Waits only when the decoder has fallen behind, which is when it would have anyway.
            self.fill();
        }
        Some(s)
    }
}

impl Source for Prefetch {
    /// What's left of the chunk, then `Some(0)` at the end. `None` ("this never changes") would
    /// keep the output at this song's rate and channels for the songs after it: a 96 kHz song
    /// after a 44.1 kHz one played at under half speed.
    fn current_span_len(&self) -> Option<usize> {
        Some(self.current.len())
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total
    }

    fn try_seek(&mut self, to: Duration) -> Result<(), SeekError> {
        let (done, result) = bounded(1);
        let gone = || SeekError::NotSupported { underlying_source: "decoder thread ended" };
        let generation = self.generation + 1;
        self.cmds.send(Cmd::Seek { to, generation, done }).map_err(|_| gone())?;
        result.recv().map_err(|_| gone())??;
        (self.generation, self.current, self.ended) = (generation, Vec::new().into_iter(), false);
        self.fill();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    fn tone(secs: usize) -> (SamplesBuffer, Vec<Sample>) {
        let (rate, channels) = (8_000, 2);
        let samples: Vec<Sample> = (0..rate * channels * secs).map(|i| (i % 1000) as Sample / 1000.0).collect();
        (SamplesBuffer::new(NonZero::new(channels as u16).unwrap(), NonZero::new(rate as u32).unwrap(), samples.clone()), samples)
    }

    #[test]
    fn plays_ahead_unchanged_and_seeks() {
        // Longer than the buffer, so the decoder waits for the output to catch up.
        let (src, samples) = tone(20);
        let mut p = Prefetch::new(src);
        assert_eq!((p.channels().get(), p.sample_rate().get(), p.total_duration()), (2, 8_000, Some(Duration::from_secs(20))));
        let start: Vec<Sample> = p.by_ref().take(1000).collect();
        assert_eq!(start, samples[..1000]);
        // Seeking drops what was decoded ahead and carries on from the new place.
        std::thread::sleep(Duration::from_millis(50));
        p.try_seek(Duration::from_secs(10)).unwrap();
        let from: usize = 10 * 8_000 * 2;
        let rest: Vec<Sample> = p.by_ref().collect();
        assert_eq!(rest, samples[from..], "everything from 10 s, then the end");
        // Back from the end, too.
        p.try_seek(Duration::from_secs(19)).unwrap();
        assert_eq!(p.count(), 8_000 * 2);
    }

    #[test]
    fn the_next_song_plays_at_its_own_rate() {
        // As the player does: one song after another, at another rate, out of an 8 kHz output.
        let rate = NonZero::new(8_000).unwrap();
        let (mixer, mut out) = rodio::mixer::mixer(NonZero::new(2).unwrap(), rate);
        let player = rodio::Player::connect_new(&mixer);
        let (first, _) = tone(1);
        let faster = SamplesBuffer::new(NonZero::new(2).unwrap(), NonZero::new(16_000).unwrap(), vec![0.5; 16_000 * 2 * 3]);
        player.append(Prefetch::new(first));
        player.append(Prefetch::new(faster));
        // The first song, then a second of the next: at its own rate, a second into it.
        for _ in 0..8_000 * 2 * 2 {
            out.next();
        }
        let pos = player.get_pos().as_secs_f64();
        assert!((0.95..1.05).contains(&pos), "a second of output played {pos:.2} s of the next song");
    }

    /// A source that holds a sender, so a test can tell when it's dropped.
    struct Probe(SamplesBuffer, #[allow(dead_code)] Sender<()>);

    impl Iterator for Probe {
        type Item = Sample;
        fn next(&mut self) -> Option<Sample> {
            self.0.next()
        }
    }

    impl Source for Probe {
        fn current_span_len(&self) -> Option<usize> {
            self.0.current_span_len()
        }
        fn channels(&self) -> ChannelCount {
            self.0.channels()
        }
        fn sample_rate(&self) -> SampleRate {
            self.0.sample_rate()
        }
        fn total_duration(&self) -> Option<Duration> {
            self.0.total_duration()
        }
    }

    #[test]
    fn the_decoder_stops_with_the_player() {
        let (src, _) = tone(60);
        let (alive, gone) = bounded::<()>(0);
        let mut p = Prefetch::new(Probe(src, alive));
        assert!(p.next().is_some());
        drop(p);
        // The thread drops its source once nobody listens.
        assert!(matches!(gone.recv_timeout(Duration::from_secs(2)), Err(crossbeam_channel::RecvTimeoutError::Disconnected)));
    }

    #[test]
    fn a_failed_seek_carries_on() {
        // `Probe` can't seek: playback goes on from where it was, to the end.
        let (src, samples) = tone(1);
        let (alive, _gone) = bounded::<()>(0);
        let mut p = Prefetch::new(Probe(src, alive));
        let first: Vec<Sample> = p.by_ref().take(10).collect();
        std::thread::sleep(Duration::from_millis(50));
        assert!(p.try_seek(Duration::from_millis(500)).is_err());
        assert_eq!(first.into_iter().chain(p).collect::<Vec<_>>(), samples);
    }

    /// `PAUSETAPE_STUTTER=<song>|<another song>|<folder> cargo test --release -- --ignored stutter --nocapture`
    /// Plays (silently) the first song as it is and the second through `Prefetch`, 20 s each at the
    /// pace of an audio output, while another thread reads the folder's files as a card write
    /// would. Counts the 10 ms slices the output would have waited for (heard as dropouts).
    #[test]
    #[ignore]
    fn stutter() {
        use std::io::Read;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Instant;
        let Ok(spec) = std::env::var("PAUSETAPE_STUTTER") else { return };
        let parts: Vec<&str> = spec.split('|').collect();
        let (a, b, folder) = (parts[0], parts[1], parts[2].to_string());
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let busy = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut buf = vec![0u8; 1 << 20];
                let mut read = 0u64;
                for e in walkdir::WalkDir::new(&folder).into_iter().flatten().filter(|e| e.file_type().is_file()) {
                    let Ok(mut f) = std::fs::File::open(e.path()) else { continue };
                    while let Ok(n) = f.read(&mut buf) {
                        if n == 0 || stop.load(Ordering::Relaxed) {
                            break;
                        }
                        read += n as u64;
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                }
                read
            })
        };
        fn run(label: &str, mut src: impl Source) {
            let per_slice = (src.sample_rate().get() as usize * src.channels().get() as usize / 100).max(1);
            let (mut late, mut worst) = (0, Duration::ZERO);
            let start = Instant::now();
            for slice in 0..2000u32 {
                let t = Instant::now();
                if src.by_ref().take(per_slice).count() < per_slice {
                    break;
                }
                let took = t.elapsed();
                worst = worst.max(took);
                if took > Duration::from_millis(10) {
                    late += 1;
                }
                let due = Duration::from_millis(10) * (slice + 1);
                std::thread::sleep(due.saturating_sub(start.elapsed()));
            }
            println!("{label:<9} slices late: {late:>4} · worst wait {worst:.1?}");
        }
        let open = |p: &str| crate::audio::decode::open_span(std::path::Path::new(p), None, || {}).unwrap();
        run("as it is", open(a));
        run("prefetch", Prefetch::new(open(b)));
        stop.store(true, Ordering::Relaxed);
        println!("read meanwhile: {} MB", busy.join().unwrap() >> 20);
    }
}
