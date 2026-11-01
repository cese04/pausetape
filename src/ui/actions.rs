//! Library and artist rows collect what the user clicked as `Action`s while drawing (when
//! the app state is borrowed); `apply` carries them out afterwards.

use crate::app::App;

pub(super) enum Action {
    Play(Vec<usize>, usize),
    /// Open an artist or close it; opening one asks MusicBrainz about it (artist index).
    ToggleArtist(String, usize),
    LookupAlbum(usize, usize),
    ApplyLookup(String),
    Dismiss(String),
    Add(usize),
    Merge(usize, usize),
    Reject(usize, usize),
    /// The artist panel: what the library database holds; ask MusicBrainz (about an entry);
    /// the MusicBrainz artists going by its name.
    ArtistFromDb(usize),
    AskArtistInfo(usize, Option<String>),
    ChooseArtist(usize),
    /// Renaming an artist in its songs' tags: the text being typed, then the name.
    Renaming(usize, String),
    Rename(usize, String),
    StopRenaming,
    /// Artist and album by index: on the card or off it, at the next Sync.
    CardAlbum(usize, usize, bool),
    /// The whole artist on the card or off it.
    CardArtist(usize, bool),
    /// Move extra copies of songs stored twice to the Trash: (extra copy, the copy that stays).
    TrashRepeats(Vec<(usize, usize)>),
    /// Open the tag editor on these songs.
    EditTags(Vec<usize>),
    /// Add a song to the selection or take it out; add a run of songs.
    Select(usize),
    SelectRun(Vec<usize>),
    ClearSelection,
    /// Move album copies another copy covers to the Trash (artist, album).
    TrashCopies(Vec<(usize, usize)>),
    /// Find covers for these albums (artist, album).
    FindCovers(Vec<(usize, usize)>),
    /// Albums on the card or off it.
    CardAlbums(Vec<(usize, usize)>, bool),
    SortAlbums(crate::app::SortBy),
    /// In the album table: ⌘-click one album, shift-click a run, a plain click just one.
    ToggleAlbum(String),
    AddAlbums(Vec<String>),
    OnlyAlbum(String),
    ClearAlbums,
}

pub(super) fn apply(app: &mut App, actions: Vec<Action>) {
    for a in actions {
        match a {
            Action::Play(q, i) => app.play(q, i),
            Action::ToggleArtist(k, a) => {
                if !app.expanded.remove(&k) {
                    app.expanded.insert(k);
                    if let Some(c) = app.index.artists.get(a).map(|a| a.cluster) {
                        app.want_artist_info(c);
                    }
                }
            }
            Action::LookupAlbum(a, b) => app.lookup_album(a, b),
            Action::ApplyLookup(id) => app.apply_lookup(&id),
            Action::Dismiss(id) => {
                app.lookups.remove(&id);
            }
            Action::Add(t) => app.add_to_playlist(t),
            Action::Merge(a, b) => app.merge(a, b),
            Action::Reject(a, b) => app.reject(a, b),
            Action::ArtistFromDb(c) => app.artist_from_db(c),
            Action::AskArtistInfo(c, mbid) => app.ask_artist_info(c, mbid),
            Action::ChooseArtist(c) => app.choose_artist(c),
            Action::Renaming(c, text) => app.renaming = Some((c, text)),
            Action::StopRenaming => app.renaming = None,
            Action::CardAlbum(a, b, on) => app.card_album(a, b, on),
            Action::CardArtist(a, on) => app.card_artist(a, on),
            Action::TrashRepeats(pairs) => app.trash_repeats(pairs),
            Action::EditTags(songs) => app.edit_tags(songs),
            Action::Select(t) => app.toggle_selected(t),
            Action::SelectRun(ts) => app.select_run(ts),
            Action::ClearSelection => {
                app.selected.clear();
                app.select_anchor = None;
            }
            Action::FindCovers(albums) => app.find_covers(albums),
            Action::TrashCopies(albums) => app.trash_copies(albums),
            Action::CardAlbums(albums, on) => app.card_albums(&albums, on),
            Action::SortAlbums(by) => app.sort_albums(by),
            Action::ToggleAlbum(id) => {
                if !app.selected_albums.remove(&id) {
                    app.selected_albums.insert(id.clone());
                }
                app.album_anchor = Some(id);
            }
            Action::AddAlbums(ids) => {
                app.album_anchor = ids.last().cloned().or(app.album_anchor.take());
                app.selected_albums.extend(ids);
            }
            Action::OnlyAlbum(id) => {
                app.selected_albums = [id.clone()].into();
                app.album_anchor = Some(id);
            }
            Action::ClearAlbums => {
                app.selected_albums.clear();
                app.album_anchor = None;
            }
            Action::Rename(c, n) => {
                app.renaming = None;
                app.rename_artist(c, n);
            }
        }
    }
}

