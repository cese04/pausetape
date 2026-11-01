//! DSD files (`.dsf`, `.dff`), which lofty doesn't read: rate, channels and length from their
//! headers, and their tags, an ID3v2 block, read by handing lofty a tiny WAV file carrying it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

pub const EXTS: &[&str] = &["dsf", "dff"];

/// Played here and converted for the card at this rate (24-bit FLAC): DSD64 and up hold
/// nothing a player could make of above it, and the Echo Mini takes at most 192 kHz.
pub const PCM_RATE: u32 = 88_200;

pub struct Dsd {
    /// The DSD rate: 2,822,400 for DSD64.
    pub rate: u32,
    pub channels: u16,
    pub duration: Duration,
    pub id3: Option<Vec<u8>>,
}

pub fn is_dsd(ext: &str) -> bool {
    EXTS.contains(&ext)
}

pub fn read(path: &Path, ext: &str) -> Option<Dsd> {
    let mut f = File::open(path).ok()?;
    match ext {
        "dsf" => dsf(&mut f),
        "dff" => dff(&mut f),
        _ => None,
    }
}

/// Sony's DSF: little-endian; a "DSD " chunk pointing at the ID3v2 block at the end, then "fmt ".
fn dsf(f: &mut File) -> Option<Dsd> {
    let mut h = [0u8; 92];
    f.read_exact(&mut h).ok()?;
    if &h[0..4] != b"DSD " || &h[28..32] != b"fmt " {
        return None;
    }
    let u32_at = |i: usize| u32::from_le_bytes(h[i..i + 4].try_into().unwrap());
    let u64_at = |i: usize| u64::from_le_bytes(h[i..i + 8].try_into().unwrap());
    let (meta, channels, rate, samples) = (u64_at(20), u32_at(52) as u16, u32_at(56), u64_at(64));
    let id3 = (meta > 0).then(|| tail(f, meta)).flatten();
    Some(Dsd { rate, channels, duration: length(samples, rate), id3 })
}

/// Philips' DSDIFF: big-endian chunks in a "FRM8" form; the rate and channels in "PROP", the
/// sound in "DSD " (or compressed, "DST ", whose length isn't worked out), tags in "ID3 ".
fn dff(f: &mut File) -> Option<Dsd> {
    let mut head = [0u8; 16];
    f.read_exact(&mut head).ok()?;
    if &head[0..4] != b"FRM8" || &head[12..16] != b"DSD " {
        return None;
    }
    let (mut rate, mut channels, mut bytes, mut id3) = (0, 0, 0, None);
    // (id, size) of the chunk at the reader's place.
    let chunk = |f: &mut File| -> Option<([u8; 4], u64)> {
        let mut c = [0u8; 12];
        f.read_exact(&mut c).ok()?;
        Some((c[0..4].try_into().unwrap(), u64::from_be_bytes(c[4..12].try_into().unwrap())))
    };
    while let Some((id, size)) = chunk(f) {
        let start = f.stream_position().ok()?;
        match &id {
            b"PROP" => {
                // "SND " then sub-chunks.
                f.seek(SeekFrom::Current(4)).ok()?;
                while f.stream_position().ok()? < start + size {
                    let (sub, n) = chunk(f)?;
                    let at = f.stream_position().ok()?;
                    let mut b = [0u8; 4];
                    match &sub {
                        b"FS  " => {
                            f.read_exact(&mut b).ok()?;
                            rate = u32::from_be_bytes(b);
                        }
                        b"CHNL" => {
                            f.read_exact(&mut b[..2]).ok()?;
                            channels = u16::from_be_bytes([b[0], b[1]]);
                        }
                        _ => {}
                    }
                    f.seek(SeekFrom::Start(at + n + n % 2)).ok()?;
                }
            }
            b"DSD " => bytes = size,
            b"ID3 " => {
                let mut b = vec![0u8; size.min(16 << 20) as usize];
                f.read_exact(&mut b).ok()?;
                id3 = Some(b);
            }
            _ => {}
        }
        f.seek(SeekFrom::Start(start + size + size % 2)).ok()?;
    }
    let samples = if channels > 0 { bytes * 8 / channels as u64 } else { 0 };
    (rate > 0).then(|| Dsd { rate, channels, duration: length(samples, rate), id3 })
}

fn length(samples: u64, rate: u32) -> Duration {
    if rate == 0 { Duration::ZERO } else { Duration::from_secs_f64(samples as f64 / rate as f64) }
}

/// Everything from `at` to the end (at most 16 MB: tags with a cover).
fn tail(f: &mut File, at: u64) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut b = Vec::new();
    f.take(16 << 20).read_to_end(&mut b).ok()?;
    b.starts_with(b"ID3").then_some(b)
}

/// The tags in an ID3v2 block, as lofty reads them from a WAV file made around it.
pub fn tags(id3: &[u8]) -> Option<lofty::file::TaggedFile> {
    use lofty::file::FileType;
    let mut wav = Vec::with_capacity(id3.len() + 64);
    let fmt: [u8; 16] = [1, 0, 2, 0, 0x44, 0xac, 0, 0, 0x10, 0xb1, 2, 0, 4, 0, 16, 0];
    let body = 4 + (8 + 16) + 8 + (8 + id3.len() + id3.len() % 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(body as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&fmt);
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&0u32.to_le_bytes());
    wav.extend_from_slice(b"id3 ");
    wav.extend_from_slice(&(id3.len() as u32).to_le_bytes());
    wav.extend_from_slice(id3);
    if id3.len() % 2 == 1 {
        wav.push(0);
    }
    // The made-up WAV has no audio to measure.
    let opts = lofty::config::ParseOptions::new().read_properties(false);
    lofty::probe::Probe::new(std::io::Cursor::new(wav)).set_file_type(FileType::Wav).options(opts).read().ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A DSF file of `secs` seconds of DSD64 silence, tagged with `title` and `artist`.
    pub(crate) fn dsf_bytes(secs: u32, title: &str, artist: &str) -> Vec<u8> {
        let (rate, channels, block) = (2_822_400u32, 2u32, 4096usize);
        let samples = rate as u64 * secs as u64;
        let blocks = (samples as usize / 8).div_ceil(block);
        let mut data = b"data".to_vec();
        data.extend_from_slice(&(12 + (blocks * block * channels as usize) as u64).to_le_bytes());
        data.resize(data.len() + blocks * block * channels as usize, 0x69);
        let frame = |id: &str, text: &str| {
            let mut f = id.as_bytes().to_vec();
            f.extend_from_slice(&(1 + text.len() as u32).to_be_bytes());
            f.extend_from_slice(&[0, 0, 3]);
            f.extend_from_slice(text.as_bytes());
            f
        };
        let frames = [frame("TIT2", title), frame("TPE1", artist)].concat();
        let n = frames.len() as u32;
        let mut id3 = b"ID3\x04\x00\x00".to_vec();
        id3.extend_from_slice(&[(n >> 21) as u8 & 0x7f, (n >> 14) as u8 & 0x7f, (n >> 7) as u8 & 0x7f, n as u8 & 0x7f]);
        id3.extend_from_slice(&frames);
        let mut fmt = b"fmt ".to_vec();
        fmt.extend_from_slice(&52u64.to_le_bytes());
        for v in [1u32, 0, 2, channels, rate, 1] {
            fmt.extend_from_slice(&v.to_le_bytes());
        }
        fmt.extend_from_slice(&samples.to_le_bytes());
        fmt.extend_from_slice(&(block as u32).to_le_bytes());
        fmt.extend_from_slice(&0u32.to_le_bytes());
        let meta = 28 + fmt.len() + data.len();
        let mut out = b"DSD ".to_vec();
        for v in [28u64, (meta + id3.len()) as u64, meta as u64] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        [out, fmt, data, id3].concat()
    }

    #[test]
    fn dsf_headers_and_tags() {
        use lofty::prelude::*;
        let dir = std::env::temp_dir().join(format!("neatlibrary-test-dsd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.dsf");
        std::fs::write(&path, dsf_bytes(2, "Test Tone", "Sample Artist")).unwrap();
        let d = read(&path, "dsf").unwrap();
        assert_eq!((d.rate, d.channels, d.duration), (2_822_400, 2, Duration::from_secs(2)));
        let tagged = tags(d.id3.as_deref().unwrap()).unwrap();
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag()).unwrap();
        assert_eq!((tag.title().as_deref(), tag.artist().as_deref()), (Some("Test Tone"), Some("Sample Artist")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
