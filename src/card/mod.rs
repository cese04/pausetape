//! Putting music on the SD card: the layout plan, copying and pruning, saved writes, drives,
//! and playlists. Also used to tidy the music folder in place (with an undo journal).

pub mod adopt;
mod copy;
mod journal;
mod plan;
pub mod playlist;
mod volumes;
mod write;

pub use copy::{Mode, Progress, Written, clean_mac_files, execute, materialise, prune, up_to_date};
pub use journal::{latest_journal, undo_latest};
pub use plan::{AlbumOrder, Custom, Options, PlanItem, Target, Unplayable, album_folder, build_plan, card_size, target, tidy_plan};
pub use volumes::{SpaceInfo, Volume, space, volumes};
pub use write::{Outcome, Preview, WriteProfile, now, preview, write};
