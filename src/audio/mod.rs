//! Playing music: the playback thread, and decoding (natively or through ffmpeg).

mod decode;
mod player;
mod prefetch;

pub use decode::{Envelope, clear_converted, envelope, ffmpeg, ffmpeg_encodes, ffmpeg_is_bundled};
pub use player::{Audio, Item, Status};
