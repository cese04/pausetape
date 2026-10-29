//! Playing music: the playback thread, and decoding (natively or through ffmpeg).

mod decode;
mod player;

pub use decode::{clear_converted, envelope, ffmpeg, ffmpeg_encodes, ffmpeg_is_bundled};
pub use player::{Audio, Status};
