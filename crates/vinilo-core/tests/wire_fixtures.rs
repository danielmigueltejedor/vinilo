// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

//! The daemon's wire format, written down as files the other frontends are tested against.
//!
//! `platforms/apple` decodes `events.jsonl` and encodes into `requests.jsonl` in its own tests,
//! so a field renamed here fails *this* test first, with the instruction to regenerate, and the
//! Swift tests then fail until its models follow. Regenerate with:
//!
//! ```text
//! UPDATE_WIRE_FIXTURES=1 cargo test -p vinilo-core --test wire_fixtures
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use vinilo_core::discover::Discover;
use vinilo_core::entry::Entry;
use vinilo_core::ipc::{
    CatalogFilter, Event, LyricLine, Lyrics, PageKind, PlayMode, QueueItem, Request, Snapshot,
    Stage, Transport, View, WriteAction,
};
use vinilo_core::music::types::{Album, Artist, Artwork, Playlist, Track, TrackId};
use vinilo_core::player::protocol::RepeatMode;
use vinilo_core::sort::SortBy;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../platforms/apple/Tests/ViniloKitTests/Fixtures")
}

fn track(id: &str, title: &str) -> Track {
    Track {
        date_added: String::new(),
        year: "2010".into(),
        favorite: true,
        in_library: true,
        library_id: Some(format!("i.{id}")),
        id: TrackId(id.into()),
        catalog_id: Some(id.into()),
        title: title.into(),
        artist: "Fixture Artist".into(),
        album: "Fixture Album".into(),
        duration_ms: 215_000,
        track_number: 3,
        artwork: Some(Artwork::new("https://example.invalid/art/{w}x{h}bb.jpg")),
    }
}

fn album() -> Album {
    Album {
        id: "l.album1".into(),
        date_added: "2026-01-02T03:04:05Z".into(),
        name: "Fixture Album".into(),
        artist: "Fixture Artist".into(),
        artwork: Some(Artwork::new("https://example.invalid/album/{w}x{h}bb.jpg")),
        year: "2010".into(),
        track_count: 12,
        library: true,
    }
}

fn artist() -> Artist {
    Artist {
        id: "r.artist1".into(),
        name: "Fixture Artist".into(),
        artwork: None,
        genres: "Pop, Latin".into(),
        library: true,
    }
}

fn playlist() -> Playlist {
    Playlist {
        id: "p.list1".into(),
        date_added: String::new(),
        last_modified: "2026-02-03T00:00:00Z".into(),
        name: "Fixture Playlist".into(),
        curator: String::new(),
        description: String::new(),
        artwork: None,
        library: true,
    }
}

fn events() -> Vec<Event> {
    vec![
        Event::Snapshot(Snapshot {
            track_id: Some("1440857781".into()),
            title: "Fixture Song".into(),
            artist: "Fixture Artist".into(),
            album: "Fixture Album".into(),
            art_path: Some("/tmp/cover.jpg".into()),
            position_ms: 61_000,
            duration_ms: 215_000,
            playing: true,
            busy: false,
            volume: 0.75,
            shuffle: true,
            repeat: RepeatMode::All,
            can_next: true,
            can_previous: false,
        }),
        Event::Queue {
            items: vec![QueueItem {
                id: Some("1440857781".into()),
                title: "Fixture Song".into(),
                artist: "Fixture Artist".into(),
                album: "Fixture Album".into(),
                duration_ms: 215_000,
            }],
            position: 0,
        },
        Event::Stage(Stage::Connecting),
        Event::Stage(Stage::Ready),
        Event::Stage(Stage::SignedOut),
        Event::Stage(Stage::Broken {
            detail: "the sidecar did not start".into(),
        }),
        Event::Rows {
            view: View::Songs,
            entries: vec![
                Entry::Song(track("1440857781", "Fixture Song")),
                Entry::Album(album()),
                Entry::Artist(artist()),
                Entry::Playlist(playlist()),
            ],
            total: 535,
        },
        Event::Results {
            query: "fixture".into(),
            entries: vec![Entry::Song(track("1440857781", "Fixture Song"))],
            offset: 0,
            more: true,
        },
        Event::Page {
            kind: PageKind::LibraryAlbum,
            id: "l.album1".into(),
            header: Entry::Album(album()),
            entries: vec![Entry::Song(track("1440857781", "Fixture Song"))],
        },
        Event::LibraryChanged,
        Event::LibraryRefreshing { refreshing: true },
        Event::Lyrics {
            id: "1440857781".into(),
            lyrics: Some(Lyrics {
                synced: true,
                lines: vec![LyricLine {
                    start_ms: 1_500,
                    text: "A line".into(),
                }],
            }),
            error: None,
        },
        Event::Error {
            detail: "Apple said no".into(),
        },
        Event::Discover(Discover::default()),
    ]
}

fn requests() -> Vec<Request> {
    vec![
        Request::Subscribe,
        Request::Snapshot,
        Request::Queue,
        Request::Stage,
        Request::Transport(Transport::Play),
        Request::Transport(Transport::PlayPause),
        Request::Transport(Transport::Next),
        Request::Transport(Transport::Previous),
        Request::Transport(Transport::Seek { position_ms: 4200 }),
        Request::Transport(Transport::SetVolume { volume: 0.5 }),
        Request::Transport(Transport::SetShuffle { shuffle: true }),
        Request::Transport(Transport::SetRepeat {
            mode: RepeatMode::One,
        }),
        Request::JumpTo { index: 3 },
        Request::SignIn,
        Request::SignOut,
        Request::Browse {
            view: View::Albums,
            query: "abc".into(),
            offset: 40,
            limit: 40,
            sort: SortBy::Artist,
            reverse: true,
        },
        Request::Search {
            query: "fixture".into(),
            filter: CatalogFilter::Songs,
            offset: 25,
        },
        Request::Open {
            kind: PageKind::LibraryArtist,
            id: "r.artist1".into(),
        },
        Request::Discover,
        Request::Enqueue {
            ids: vec!["1".into(), "2".into()],
            next: true,
        },
        Request::RemoveFromQueue { index: 1 },
        Request::MoveInQueue { from: 1, to: 4 },
        Request::ClearQueue,
        Request::Write {
            action: WriteAction::AddToLibrary,
            id: "1440857781".into(),
            playlist_id: None,
            name: None,
        },
        Request::Refresh,
        Request::Play {
            ids: vec!["1".into(), "2".into(), "3".into()],
            index: 1,
            start: PlayMode::Shuffled,
        },
        Request::PlayFiles {
            paths: vec!["/Users/fixture/Music/a.flac".into(), "/Users/fixture/Music".into()],
            index: 0,
        },
        Request::Lyrics {
            id: "1440857781".into(),
        },
        Request::Quit,
    ]
}

fn lines<T: serde::Serialize>(items: &[T]) -> String {
    let mut text = String::new();
    for item in items {
        text.push_str(&serde_json::to_string(item).unwrap());
        text.push('\n');
    }
    text
}

fn check(name: &str, expected: String) {
    let path = fixtures().join(name);
    if std::env::var_os("UPDATE_WIRE_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, expected).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        on_disk, expected,
        "{name} no longer matches the wire format. If the change is intended, regenerate it \
         (UPDATE_WIRE_FIXTURES=1 cargo test -p vinilo-core --test wire_fixtures) and update \
         platforms/apple/Sources/ViniloKit to follow."
    );
}

#[test]
fn the_events_every_frontend_decodes_are_unchanged() {
    check("events.jsonl", lines(&events()));
}

#[test]
fn the_requests_every_frontend_sends_are_unchanged() {
    check("requests.jsonl", lines(&requests()));
}

#[test]
fn every_event_in_the_fixture_reads_back() {
    for event in events() {
        let line = serde_json::to_string(&event).unwrap();
        serde_json::from_str::<Event>(&line).unwrap();
    }
}
