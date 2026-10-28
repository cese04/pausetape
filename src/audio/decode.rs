//! Turning a file into samples: natively with Symphonia, or via ffmpeg for formats it can't
//! read (converted once to a cached stereo WAV). Also the loudness envelope for the waveform.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rodio::{Decoder, Source};

/// Where ffmpeg is: the copy bundled next to the app's executable (inside the .app), else an
/// installed one. Apps started from Finder don't get the shell's PATH, so the usual install
/// locations are checked as well. `NEATLIBRARY_FFMPEG` overrides everything.
pub fn ffmpeg() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
            let mut candidates: Vec<PathBuf> = std::env::var_os("NEATLIBRARY_FFMPEG").map(PathBuf::from).into_iter().collect();
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
        return Err(format!("{native_err}. Installing ffmpeg lets NeatLibrary play this format."));
    };
    on_convert();
    let wav = converted(path, ff)?;
    Decoder::try_from(File::open(&wav).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Cache of converted files, newest last; only a few are kept (a 5-minute track is ~55 MB).
const KEEP_CONVERTED: usize = 6;

fn converted_dir() -> PathBuf {
    std::env::temp_dir().join("neatlibrary-converted")
}

/// Remove conversions left over from earlier runs.
pub fn clear_converted() {
    let _ = std::fs::remove_dir_all(converted_dir());
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
    // First audio stream only (skips cover-art streams), downmixed to stereo 16-bit PCM.
    let status = std::process::Command::new(ff)
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "2", "-c:a", "pcm_s16le", "-f", "wav"])
        .arg(&tmp)
        .status()
        .map_err(|e| format!("running ffmpeg: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("ffmpeg couldn't convert {}", path.display()));
    }
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;

    // Trim the cache to the most recent conversions.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> =
            entries.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
        files.sort();
        for (_, old) in files.iter().rev().skip(KEEP_CONVERTED) {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(out)
}

/// RMS loudness in 100 ms windows, normalised to 0..=1. Decodes the whole file, so call it off
/// the UI thread.
pub fn envelope(path: &Path) -> Option<Vec<f32>> {
    let src = open(path, || {}).ok()?;
    let window = (src.sample_rate().get() as usize * src.channels().get() as usize / 10).max(1);
    let mut out = Vec::new();
    let (mut sum, mut n) = (0.0f64, 0usize);
    for s in src {
        sum += (s as f64) * (s as f64);
        n += 1;
        if n == window {
            out.push((sum / n as f64).sqrt() as f32);
            (sum, n) = (0.0, 0);
        }
    }
    let max = out.iter().cloned().fold(f32::EPSILON, f32::max);
    Some(out.into_iter().map(|v| v / max).collect())
}

#[cfg(test)]
mod tests {
    /// `NEATLIBRARY_FORMATS=<dir> cargo test -- --ignored decodes_formats --nocapture`
    /// Fully decodes every file in the folder and reports which formats work.
    #[test]
    #[ignore]
    fn decodes_formats() {
        let Some(dir) = std::env::var_os("NEATLIBRARY_FORMATS") else { return };
        let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let t = std::time::Instant::now();
            let env = super::envelope(&f);
            let codec = crate::library::mp4::mp4_audio_codec(&f).map(|c| format!(" [{}]", crate::library::mp4::codec_name(&c))).unwrap_or_default();
            print!("{:<28}{codec} in {:.2?}: ", f.file_name().unwrap().to_string_lossy(), t.elapsed());
            println!("{}", match env {
                Some(e) if !e.is_empty() => format!("decoded ({:.1} s)", e.len() as f32 / 10.0),
                _ => "cannot decode".into(),
            });
        }
    }
}
