//! Which codec is inside an MP4-family file (.m4a, .mp4, …): AAC and ALAC play natively;
//! Dolby, Opus and others need ffmpeg.

use std::path::Path;

pub(super) const MP4_EXTS: &[&str] = &["m4a", "m4b", "mp4", "alac"];

/// Readable name for an MP4 sample-entry code.
pub fn codec_name(code: &str) -> &str {
    match code {
        "mp4a" => "AAC",
        "alac" => "ALAC",
        "ec-3" => "Dolby E-AC-3",
        "ac-3" => "Dolby AC-3",
        "ac-4" => "Dolby AC-4",
        "Opus" => "Opus",
        "fLaC" => "FLAC",
        ".mp3" => "MP3",
        "mha1" | "mhm1" => "MPEG-H 3D Audio",
        other => other,
    }
}

/// The sample-entry code of the first audio track in an MP4 file, found by walking
/// moov › trak › mdia › minf › stbl › stsd. Only box headers are read, so it's cheap.
pub fn mp4_audio_codec(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    fn boxes(f: &mut std::fs::File, start: u64, end: u64) -> Vec<([u8; 4], u64, u64)> {
        let mut out = Vec::new();
        let mut pos = start;
        while pos + 8 <= end {
            let mut h = [0u8; 8];
            if f.seek(SeekFrom::Start(pos)).is_err() || f.read_exact(&mut h).is_err() {
                break;
            }
            let mut size = u32::from_be_bytes(h[..4].try_into().unwrap()) as u64;
            let kind: [u8; 4] = h[4..].try_into().unwrap();
            let mut header = 8;
            if size == 1 {
                let mut big = [0u8; 8];
                if f.read_exact(&mut big).is_err() {
                    break;
                }
                size = u64::from_be_bytes(big);
                header = 16;
            } else if size == 0 {
                size = end - pos;
            }
            if size < header {
                break;
            }
            out.push((kind, pos + header, (pos + size).min(end)));
            pos += size;
        }
        out
    }
    fn child(f: &mut std::fs::File, (s, e): (u64, u64), name: &[u8; 4]) -> Option<(u64, u64)> {
        boxes(f, s, e).into_iter().find(|(k, _, _)| k == name).map(|(_, s, e)| (s, e))
    }
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let moov = child(&mut f, (0, len), b"moov")?;
    for (kind, s, e) in boxes(&mut f, moov.0, moov.1) {
        if &kind != b"trak" {
            continue;
        }
        let Some(mdia) = child(&mut f, (s, e), b"mdia") else { continue };
        // hdlr: version/flags (4), pre_defined (4), handler type (4).
        let Some(hdlr) = child(&mut f, mdia, b"hdlr") else { continue };
        let mut handler = [0u8; 4];
        if f.seek(SeekFrom::Start(hdlr.0 + 8)).is_err() || f.read_exact(&mut handler).is_err() || &handler != b"soun" {
            continue;
        }
        let stsd = child(&mut f, mdia, b"minf").and_then(|m| child(&mut f, m, b"stbl")).and_then(|t| child(&mut f, t, b"stsd"))?;
        // stsd: version/flags (4), entry count (4), then the first entry's size (4) and code (4).
        let mut code = [0u8; 4];
        f.seek(SeekFrom::Start(stsd.0 + 12)).ok()?;
        f.read_exact(&mut code).ok()?;
        return Some(String::from_utf8_lossy(&code).into_owned());
    }
    None
}
