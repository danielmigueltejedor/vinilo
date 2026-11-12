// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// Listen Now: what the engine sends for recently played, recommendations and charts. A shelf
/// the engine left empty is not drawn; nothing here is made up to fill the page.
struct HomeView: View {
    @Environment(PlayerStore.self) private var store

    var body: some View {
        Group {
            switch store.discover {
            case .idle, .loading:
                StatusView(symbol: "house", title: L10n.t(.loading), busy: true)
            case .loaded(let discover) where discover.isEmpty:
                StatusView(
                    symbol: "music.note.house", title: L10n.t(.emptyLibrary),
                    message: L10n.t(store.source == .appleMusic ? .emptyLibraryBody : .emptyLibraryAny))
            case .loaded(let discover):
                shelves(discover)
            }
        }
        .navigationTitle(L10n.t(.home))
        .task { store.loadDiscover() }
    }

    private func shelves(_ discover: Discover) -> some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 28) {
                shelf(L10n.t(.recentlyPlayed), discover.recentlyPlayed)
                shelf(L10n.t(.madeForYou), discover.recommendedPlaylists)
                shelf(L10n.t(.recentlyAdded), discover.recentlyAdded)
                shelf(L10n.t(.charts), discover.charts)
                if !discover.recommendedSongs.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(L10n.t(.topSongs)).font(.title3.weight(.semibold))
                        let songs = discover.recommendedSongs.map(Entry.song)
                        ForEach(Array(songs.enumerated()), id: \.offset) { _, entry in
                            if case .song(let track) = entry {
                                SongRow(track: track) { store.play(entry, in: songs) }
                            }
                        }
                    }
                }
            }
            .padding(20)
        }
    }

    @ViewBuilder
    private func shelf(_ title: String, _ entries: [Entry]) -> some View {
        if !entries.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
                Text(title).font(.title3.weight(.semibold))
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: 18) {
                        ForEach(Array(entries.enumerated()), id: \.offset) { _, entry in
                            tile(entry, in: entries)
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func tile(_ entry: Entry, in entries: [Entry]) -> some View {
        if let route = entry.pageTarget {
            NavigationLink(value: route) { EntryTile(entry: entry, size: 160) }
                .buttonStyle(.plain)
        } else if case .song(let track) = entry {
            Button { store.play(entry, in: entries) } label: {
                EntryTile(entry: .song(track), size: 160)
            }
            .buttonStyle(.plain)
            .disabled(!track.isPlayable)
        }
    }
}

/// Artists, albums, songs or playlists from the user's library.
struct LibrarySection: View {
    @Environment(PlayerStore.self) private var store
    var view: LibraryView
    var title: String

    var body: some View {
        Group {
            switch store.library[view] {
            case .none, .some(.idle), .some(.loading):
                StatusView(symbol: "music.note.list", title: L10n.t(.loading), busy: true)
            case .some(.loaded(let rows)) where rows.entries.isEmpty:
                StatusView(
                    symbol: "music.note.list", title: L10n.t(.emptyLibrary),
                    message: L10n.t(store.source == .appleMusic ? .emptyLibraryBody : .emptyLibraryAny))
            case .some(.loaded(let rows)):
                if view == .songs {
                    SongList(entries: rows.entries)
                } else {
                    EntryCollection(entries: rows.entries)
                }
            }
        }
        .navigationTitle(title)
        .task(id: view) { store.loadLibrary(view) }
        .toolbar {
            ToolbarItem {
                Button {
                    store.refreshLibrary()
                } label: {
                    Label(L10n.t(.refresh), systemImage: "arrow.clockwise")
                }
                .disabled(store.libraryRefreshing)
                .help(store.libraryRefreshing ? L10n.t(.refreshing) : L10n.t(.refresh))
            }
        }
    }
}

/// Every song, as a list that only builds the rows on screen.
struct SongList: View {
    @Environment(PlayerStore.self) private var store
    var entries: [Entry]

    var body: some View {
        List {
            ForEach(Array(entries.enumerated()), id: \.offset) { _, entry in
                if case .song(let track) = entry {
                    SongRow(track: track) { store.play(entry, in: entries) }
                }
            }
        }
        .listStyle(.inset)
    }
}
