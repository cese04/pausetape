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
    /// Show an artist's links to stores and sites, or fold them away (by cluster key).
    ToggleArtistLinks(String),
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
    /// A plain click on a song: it alone is selected.
    OnlySong(usize),
    /// The arrow keys moved to a song: select it alone, or (with Shift) add it; bring it into view.
    SongCursor(usize, bool),
    /// The same in the album table, by album id and row.
    AlbumCursor(String, usize, bool),
    /// ⌘D: these albums onto the card, or off it if they're all there.
    ToggleCard(Vec<(usize, usize)>),
    /// Songs to the end of a playlist (by position); into a new playlist.
    AddToPlaylist(usize, Vec<usize>),
    NewPlaylistWith(Vec<usize>),
    /// An artist's albums in the library by artist, opened (from the details panel).
    ShowArtist(usize),
    /// A genre's artists and albums (from the details panel).
    ShowGenre(String),
    /// An album selected, its details shown (beside the library, or as a sheet).
    Details(usize, usize),
    /// Find these songs' lyrics ahead of time.
    FindLyrics(Vec<usize>),
    /// Songs after the one playing; at the end of the queue.
    PlayNext(Vec<usize>),
    AddToQueue(Vec<usize>),
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
            Action::LookupAlbum(a, b) => {
                app.lookup_album(a, b);
                // In the album table, suggestions aren't under the album: say where they are.
                if app.saved.album_table || app.tab != crate::app::Tab::Library {
                    let title = app.index.artists[a].albums[b].title.clone().unwrap_or_else(|| "these songs".into());
                    app.notice(format!("asking MusicBrainz about “{title}”: its suggestions show in its details and in Review › Suggested tags"));
                }
            }
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
            Action::ToggleArtistLinks(k) => {
                if !app.artist_links_open.remove(&k) {
                    app.artist_links_open.insert(k);
                }
            }
            Action::CardAlbum(a, b, on) => app.card_album(a, b, on),
            Action::CardArtist(a, on) => app.card_artist(a, on),
            Action::TrashRepeats(pairs) => app.trash_repeats(pairs),
            Action::EditTags(songs) => app.edit_tags(songs),
            Action::Select(t) => app.toggle_selected(t),
            Action::SelectRun(ts) => app.select_run(ts),
            Action::OnlySong(t) => {
                app.selected = [t].into();
                app.select_anchor = Some(t);
            }
            Action::SongCursor(t, extend) => {
                if !extend {
                    app.selected.clear();
                }
                app.selected.insert(t);
                app.select_anchor = Some(t);
                app.scroll_to_song = Some(t);
            }
            Action::AlbumCursor(id, row, extend) => {
                if !extend {
                    app.selected_albums.clear();
                }
                app.selected_albums.insert(id.clone());
                app.album_anchor = Some(id);
                app.album_scroll_to = Some(row);
            }
            Action::ToggleCard(albums) => app.toggle_on_card(albums),
            Action::AddToPlaylist(i, songs) => app.add_songs_to_playlist(i, &songs),
            Action::ShowArtist(a) => {
                let name = app.index.artists[a].name.clone();
                app.expanded.insert(crate::library::names::artist_key(&name));
                app.tab = crate::app::Tab::Library;
                app.saved.album_table = false;
                (app.duplicates_only, app.look_alikes_only) = (false, false);
                app.filter = name;
                app.details_sheet = false;
            }
            Action::ShowGenre(g) => {
                super::genres::open(app, Some(&g));
                app.details_sheet = false;
            }
            Action::Details(a, b) => {
                let artist = &app.index.artists[a];
                let al = &artist.albums[b];
                let id = al.id(&app.index.clusters[artist.cluster].key);
                let first = al.tracks[0];
                if app.saved.album_table {
                    app.selected_albums = [id.clone()].into();
                    app.album_anchor = Some(id);
                } else {
                    app.selected = [first].into();
                    app.select_anchor = Some(first);
                }
                app.details_sheet = !app.details_beside;
            }
            Action::PlayNext(songs) => app.play_next(&songs),
            Action::FindLyrics(songs) => app.find_lyrics(&songs),
            Action::AddToQueue(songs) => app.add_to_queue(&songs),
            Action::NewPlaylistWith(songs) => {
                app.new_playlist();
                if let Some(i) = app.active_playlist {
                    app.add_songs_to_playlist(i, &songs);
                }
            }
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

