//! Putting music on the SD card: the layout plan, copying and pruning, saved writes, drives,
//! and playlists. Also used to tidy the music folder in place (with an undo journal).

pub mod adopt;
mod copy;
mod device;
mod journal;
pub mod pattern;
pub mod label;
mod plan;
pub mod playlist;
pub mod testcard;
mod volumes;
mod write;

pub use device::{Device, Player, Results, Sample, Tested};
pub use copy::{Mode, Progress, Written, clean_mac_files, execute, materialise, prune, up_to_date};
pub use label::Label;
pub use journal::{latest_journal, save_problems, undo_latest};
pub use plan::{AlbumOrder, Chosen, Custom, Options, PlanItem, REPEAT, Rules, Target, Unplayable, build_plan, card_size, song_path, target, tidy_plan};
pub use volumes::{SpaceInfo, Volume, connected, eject_ended, ejecting, held, let_go_of_ejected, space, volumes};
#[cfg(windows)]
pub use volumes::drives;
pub use write::{Outcome, Preview, WriteProfile, now, preview, removing, renamed, write};
