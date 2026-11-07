//! Turning a file into samples: natively with Symphonia, or via ffmpeg for formats it can't
//! read (converted once to a cached stereo WAV). Also the loudness envelope for the waveform.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Decoder, Sample, SampleRate, Source};

/// Where ffmpeg is: the copy bundled next to the app's executable (inside the .app), else an
/// installed one. Apps started from Finder don't get the shell's PATH, so the usual install
/// locations are checked as well. `PAUSETAPE_FFMPEG` overrides everything.
pub fn ffmpeg() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
            let mut candidates: Vec<PathBuf> = std::env::var_os("PAUSETAPE_FFMPEG").map(PathBuf::from).into_iter().collect();
            if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
                candidates.push(dir.join(exe));
            }
            candidates.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/opt/local/bin"].iter().map(|d| Path::new(d).join(exe)));
            if let Some(path) = std::env::var_os("PATH") {
                candidates.extend(std::env::split_paths(&path).map(|d| d.join(exe)));
            }
            candidates.into_iter().find(|p| p.is_file())
        })
        .as_deref()
}

/// The ffmpeg in use has this encoder ("flac", "libmp3lame"). Builds differ: the minimal one
/// bundled with the app has only what Pausetape asks of it.
pub fn ffmpeg_encodes(name: &str) -> bool {
    static ENCODERS: OnceLock<String> = OnceLock::new();
    let list = ENCODERS.get_or_init(|| {
        let Some(ff) = ffmpeg() else { return String::new() };
        crate::platform::quiet_command(ff)
            .args(["-hide_banner", "-encoders"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    });
    // Lines look like " A....D flac                 FLAC (Free Lossless Audio Codec)".
    list.lines().any(|l| l.split_whitespace().nth(1) == Some(name))
}

/// The ffmpeg in use is the one shipped inside the app.
pub fn ffmpeg_is_bundled() -> bool {
    let here = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    ffmpeg().is_some_and(|f| here.is_some_and(|d| f.parent() == Some(d.as_path())))
}

type FileSource = Decoder<BufReader<File>>;

/// Open `path` for decoding: natively when Symphonia can, else via a stereo WAV made by ffmpeg
/// (Dolby E-AC-3/AC-3, TrueHD, DTS, Opus, APE, WavPack, WMA…; not AC-4, which FFmpeg can't decode). `on_convert` runs before converting.
pub fn open(path: &Path, on_convert: impl FnOnce()) -> Result<FileSource, String> {
    let native = File::open(path).map_err(|e| e.to_string()).and_then(|f| Decoder::try_from(f).map_err(|e| e.to_string()));
    let native_err = match native {
        Ok(src) => return Ok(src),
        Err(e) => e,
    };
    let Some(ff) = ffmpeg() else {
        return Err(format!("{native_err}. Installing ffmpeg lets Pausetape play this format."));
    };
    on_convert();
    let wav = converted(path, ff)?;
    Decoder::try_from(File::open(&wav).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Part of a file, from `start` to `end` (or the file's end): one song of an album stored as a
/// single file with a CUE sheet. Times and seeking are relative to the song.
pub type Span = (Duration, Option<Duration>);

/// `open`, then keep to `span` if there is one.
pub fn open_span(path: &Path, span: Option<Span>, on_convert: impl FnOnce()) -> Result<Section<FileSource>, String> {
    let src = open(path, on_convert)?;
    match span {
        Some((start, end)) => Section::new(src, start, end).map_err(|e| format!("seeking to the song: {e}")),
        None => Ok(Section::whole(src)),
    }
}

/// A source limited to a section of another, counted in sample frames so songs cut from an
/// album file start and end exactly where the CUE sheet says.
pub struct Section<S> {
    inner: S,
    /// The section's first sample frame in `inner` (0 for a whole file).
    first: u64,
    /// Samples (all channels) the section holds, and how many are left.
    len: Option<u64>,
    left: Option<u64>,
}

impl<S: Source> Section<S> {
    fn whole(inner: S) -> Self {
        Section { inner, first: 0, len: None, left: None }
    }

    fn new(inner: S, start: Duration, end: Option<Duration>) -> Result<Self, SeekError> {
        let first = frame_at(&inner, start);
        let len = end.map(|e| (frame_at(&inner, e).saturating_sub(first)) * inner.channels().get() as u64);
        let mut s = Section { inner, first, len, left: len };
        if first > 0 {
            s.seek_frame(first)?;
        }
        Ok(s)
    }

    /// Seek to the middle of a sample frame: a time exactly on a frame boundary can round down
    /// to the frame before it.
    fn seek_frame(&mut self, frame: u64) -> Result<(), SeekError> {
        let rate = self.inner.sample_rate().get() as u128;
        self.inner.try_seek(Duration::from_nanos(((frame as u128 * 2 + 1) * 1_000_000_000 / (rate * 2)) as u64))
    }
}

/// The sample frame nearest to `t`.
fn frame_at(s: &impl Source, t: Duration) -> u64 {
    let rate = s.sample_rate().get() as u128;
    ((t.as_nanos() * rate + 500_000_000) / 1_000_000_000) as u64
}

impl<S: Source> Iterator for Section<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.left == Some(0) {
            return None;
        }
        let s = self.inner.next()?;
        if let Some(l) = &mut self.left {
            *l -= 1;
        }
        Some(s)
    }
}

impl<S: Source> Source for Section<S> {
    fn current_span_len(&self) -> Option<usize> {
        match (self.inner.current_span_len(), self.left) {
            (Some(n), Some(l)) => Some(n.min(l as usize)),
            (None, Some(l)) => Some(l as usize),
            (n, None) => n,
        }
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        let per_sec = self.sample_rate().get() as u128 * self.channels().get() as u128;
        match self.len {
            Some(n) => Some(Duration::from_nanos((n as u128 * 1_000_000_000 / per_sec) as u64)),
            None => {
                let start = Duration::from_nanos((self.first as u128 * 1_000_000_000 / self.sample_rate().get() as u128) as u64);
                self.inner.total_duration().map(|d| d.saturating_sub(start))
            }
        }
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let offset = frame_at(&self.inner, pos);
        self.seek_frame(self.first + offset)?;
        if let Some(len) = self.len {
            self.left = Some(len.saturating_sub(offset * self.channels().get() as u64));
        }
        Ok(())
    }
}

/// Cache of converted files: the newest are kept up to this size (a 5-minute song is ~55 MB, a
/// CD stored as one file ~600 MB); the newest is always kept, as it's likely playing.
const KEEP_BYTES: u64 = 1 << 30;

fn converted_dir() -> PathBuf {
    std::env::temp_dir().join("pausetape-converted")
}

/// Remove conversions left over from earlier runs, those made under the app's former name too.
pub fn clear_converted() {
    let _ = std::fs::remove_dir_all(converted_dir());
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!("{}-converted", crate::FORMER_NAME.to_lowercase())));
}

fn converted(path: &Path, ff: &Path) -> Result<PathBuf, String> {
    use std::hash::{Hash, Hasher};
    // One conversion at a time: the player and the waveform often ask for the same file at once.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok()).hash(&mut h);
    let dir = converted_dir();
    let out = dir.join(format!("{:016x}.wav", h.finish()));
    if out.is_file() {
        return Ok(out);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = out.with_extension("partial.wav");
    // First audio stream only (skips cover-art streams), downmixed to stereo 16-bit PCM; DSD
    // would come out at an eighth of its rate (352.8 kHz and up), so it's brought down.
    let mut cmd = crate::platform::quiet_command(ff);
    cmd.args(["-nostdin", "-v", "error", "-y", "-i"]).arg(path).args(["-map", "0:a:0", "-ac", "2", "-c:a", "pcm_s16le"]);
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if crate::library::dsd::is_dsd(&ext) {
        cmd.args(["-ar", &crate::library::dsd::PCM_RATE.to_string()]);
    }
    // Its errors are kept for the player to show, not left on the terminal the app came from.
    let run = cmd.args(["-f", "wav"]).arg(&tmp).output().map_err(|e| format!("running ffmpeg: {e}"))?;
    if !run.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(ffmpeg_error(&run.stderr));
    }
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;

    // Trim the cache to the most recent conversions that fit in `KEEP_BYTES`.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                Some((m.modified().ok()?, m.len(), e.path()))
            })
            .collect();
        files.sort();
        let mut kept = 0;
        for (i, (_, size, old)) in files.iter().rev().enumerate() {
            kept += size;
            if i > 0 && kept > KEEP_BYTES {
                // A file another copy of the app is playing stays readable where it's open
                // (macOS, Linux) or can't be removed (Windows): either way, nothing breaks.
                let _ = std::fs::remove_file(old);
            }
        }
    }
    Ok(out)
}

/// What went wrong, from ffmpeg's error output: its first line, the cause (the ones after follow
/// from it), without the "[in#0 @ 0x…]" it starts with. The caller names the file.
fn ffmpeg_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let first = first.strip_prefix('[').and_then(|l| l.split_once("] ")).map_or(first, |(_, cause)| cause);
    if first.is_empty() { "ffmpeg couldn't convert it".into() } else { format!("ffmpeg couldn't convert it: {first}") }
}

/// What reading a song through found.
pub enum Envelope {
    /// RMS loudness in 100 ms windows, normalised to 0..=1.
    Loudness(Vec<f32>),
    /// Its audio is broken, and how ("its audio stops making sense at 0:04").
    Damaged(String),
}

/// Read the whole file (or song) through, so call it off the UI thread. `expected` is how long
/// its tags say it is: much less audio than that means part of it was lost. `None` if it
/// doesn't open.
pub fn envelope(path: &Path, span: Option<Span>, expected: Duration) -> Option<Envelope> {
    let src = open_span(path, span, || {}).ok()?;
    let per_sec = src.sample_rate().get() as usize * src.channels().get() as usize;
    let window = (per_sec / 10).max(1);
    let mut out = Vec::new();
    let (mut sum, mut n, mut read) = (0.0f64, 0usize, 0usize);
    // Symphonia panics on some damaged AAC (a section that never ends runs off its table)
    // rather than saying so; what was read up to there tells where.
    let whole = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for s in src {
            sum += (s as f64) * (s as f64);
            (n, read) = (n + 1, read + 1);
            if n == window {
                out.push((sum / n as f64).sqrt() as f32);
                (sum, n) = (0.0, 0);
            }
        }
    }));
    let heard = Duration::from_secs_f64(read as f64 / per_sec.max(1) as f64);
    let clock = |d: Duration| format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60);
    if whole.is_err() {
        return Some(Envelope::Damaged(format!("its audio stops making sense at {}", clock(heard))));
    }
    // Half: a length worked out from the bitrate (an MP3 without a header saying it) can be
    // somewhat off, but not by that much.
    if expected >= Duration::from_secs(5) && heard < expected / 2 {
        return Some(Envelope::Damaged(if heard < Duration::from_secs(1) {
            format!("none of its audio can be read, though it should last {}", clock(expected))
        } else {
            format!("only {} of its audio can be read, of the {} it should last", clock(heard), clock(expected))
        }));
    }
    let max = out.iter().cloned().fold(f32::EPSILON, f32::max);
    Some(Envelope::Loudness(out.into_iter().map(|v| v / max).collect()))
}

#[cfg(test)]
mod tests {
    /// A mono 8 kHz WAV, `secs` long, of a tone.
    fn tone(path: &std::path::Path, secs: u32) {
        let samples: Vec<u8> = (0..8000 * secs).flat_map(|i| (((i as f32 * 0.3).sin() * 8000.0) as i16).to_le_bytes()).collect();
        let mut b = b"RIFF".to_vec();
        b.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 1, 0, 0x40, 0x1f, 0, 0, 0x80, 0x3e, 0, 0, 2, 0, 16, 0]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        b.extend_from_slice(&samples);
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn audio_far_shorter_than_its_tags_say_is_damage() {
        use super::Envelope;
        let dir = std::env::temp_dir().join(format!("pausetape-test-envelope-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (song, empty) = (dir.join("song.wav"), dir.join("empty.wav"));
        tone(&song, 3);
        tone(&empty, 0);
        let secs = std::time::Duration::from_secs;
        // As long as it says, or near enough: loudness, every 100 ms.
        assert!(matches!(super::envelope(&song, None, secs(3)), Some(Envelope::Loudness(e)) if e.len() == 30));
        assert!(matches!(super::envelope(&song, None, secs(5)), Some(Envelope::Loudness(_))));
        match super::envelope(&song, None, secs(60)) {
            Some(Envelope::Damaged(why)) => assert_eq!(why, "only 0:03 of its audio can be read, of the 1:00 it should last"),
            _ => panic!("3 s of the 60 its tags say is damage"),
        }
        match super::envelope(&empty, None, secs(158)) {
            Some(Envelope::Damaged(why)) => assert_eq!(why, "none of its audio can be read, though it should last 2:38"),
            _ => panic!("no audio at all is damage"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ffmpeg_errors_name_the_cause() {
        let damaged = b"[in#0 @ 0x7ce2c18000] Error opening input: Invalid data found when processing input\nError opening input file bad.ape.\nError opening input files: Invalid data found when processing input\n";
        assert_eq!(super::ffmpeg_error(damaged), "ffmpeg couldn't convert it: Error opening input: Invalid data found when processing input");
        let cut_short = b"[in#0 @ 0x7828c20000] No frames in the file!\n[in#0 @ 0x7828c18000] Error opening input: Invalid argument\n";
        assert_eq!(super::ffmpeg_error(cut_short), "ffmpeg couldn't convert it: No frames in the file!");
        assert_eq!(super::ffmpeg_error(b""), "ffmpeg couldn't convert it");
    }

    /// `PAUSETAPE_SECTION=<album.flac>:<start frames>:<end frames>:<reference.raw>` with the
    /// reference being that section cut by ffmpeg as 16-bit stereo PCM; checks that playback of
    /// a CUE song yields exactly those samples.
    #[test]
    #[ignore]
    fn section_matches_cut() {
        use rodio::Source;
        let Ok(spec) = std::env::var("PAUSETAPE_SECTION") else { return };
        let parts: Vec<&str> = spec.split(':').collect();
        let frames = |s: &str| crate::library::cue::frames_to_time(s.parse().unwrap());
        let src = super::open_span(std::path::Path::new(parts[0]), Some((frames(parts[1]), Some(frames(parts[2])))), || {}).unwrap();
        let expect_len = src.total_duration().unwrap();
        let got: Vec<f32> = src.collect();
        let raw = std::fs::read(parts[3]).unwrap();
        let want: Vec<f32> = raw.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let worst = got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        println!("samples: got {} want {} · duration {expect_len:?} · max difference {worst}", got.len(), want.len());
        assert_eq!(got.len(), want.len());
        assert!(worst < 1e-4);
    }

    /// `PAUSETAPE_FORMATS=<dir> cargo test -- --ignored decodes_formats --nocapture`
    /// Fully decodes every file in the folder and reports which formats work.
    #[test]
    #[ignore]
    fn decodes_formats() {
        let Some(dir) = std::env::var_os("PAUSETAPE_FORMATS") else { return };
        let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let t = std::time::Instant::now();
            let env = super::envelope(&f, None, std::time::Duration::ZERO);
            let codec = crate::library::mp4::mp4_audio_codec(&f).map(|c| format!(" [{}]", crate::library::mp4::codec_name(&c))).unwrap_or_default();
            print!("{:<28}{codec} in {:.2?}: ", f.file_name().unwrap().to_string_lossy(), t.elapsed());
            println!("{}", match env {
                Some(super::Envelope::Loudness(e)) if !e.is_empty() => format!("decoded ({:.1} s)", e.len() as f32 / 10.0),
                Some(super::Envelope::Damaged(why)) => format!("damaged: {why}"),
                _ => "cannot decode".into(),
            });
        }
    }
}
