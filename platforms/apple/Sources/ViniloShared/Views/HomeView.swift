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
            LazyVStack(alignment: .leading, spacing: 34) {
                shelf(L10n.t(.recentlyPlayed), discover.recentlyPlayed)
                shelf(L10n.t(.madeForYou), discover.recommendedPlaylists)
                shelf(L10n.t(.recentlyAdded), discover.recentlyAdded)
                shelf(L10n.t(.charts), discover.charts)
                if !discover.recommendedSongs.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(L10n.t(.topSongs)).font(.title2.weight(.bold))
                        let songs = discover.recommendedSongs.map(Entry.song)
                        ForEach(Array(songs.enumerated()), id: \.offset) { _, entry in
                            if case .song(let track) = entry {
                                SongRow(track: track) { store.play(entry, in: songs) }
                            }
                        }
                    }
                }
            }
            .padding(.horizontal, 28)
            .padding(.vertical, 24)
        }
        .scrolledToolbarFade()
    }

    @ViewBuilder
    private func shelf(_ title: String, _ entries: [Entry]) -> some View {
        if !entries.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
                Text(title).font(.title2.weight(.bold))
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: 20) {
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
    @State private var filter = ""

    /// What the box above the list narrows the section to: titles, artists and names.
    private func matching(_ entries: [Entry]) -> [Entry] {
        let words = filter.trimmingCharacters(in: .whitespaces)
        guard !words.isEmpty else { return entries }
        return entries.filter { entry in
            let text: String =
                switch entry {
                case .song(let t): "\(t.title) \(t.artist) \(t.album)"
                case .album(let a): "\(a.name) \(a.artist)"
                case .artist(let a): a.name
                case .playlist(let p): p.name
                }
            return text.localizedCaseInsensitiveContains(words)
        }
    }

    private var sortChoices: [(SortBy, L10n.Key)] {
        switch view {
        case .songs: [(.title, .sortTitle), (.artist, .sortArtist), (.album, .sortAlbum), (.year, .sortYear)]
        case .albums: [(.title, .sortTitle), (.artist, .sortArtist), (.year, .sortYear)]
        default: [(.title, .sortTitle)]
        }
    }

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
                    SongList(entries: matching(rows.entries))
                        .scrolledToolbarFade(height: 136)
                } else {
                    EntryCollection(entries: matching(rows.entries))
                        .scrolledToolbarFade(height: 136)
                }
            }
        }
        .navigationTitle(title)
        .task(id: view) { store.loadLibrary(view) }
        // In the page, not the toolbar: the window's search field is the only search item.
        .safeAreaInset(edge: .top, spacing: 0) {
            HStack {
                Spacer(minLength: 0)
                GlassSearchField(text: $filter, prompt: L10n.t(.filterLibrary), symbol: "line.3.horizontal.decrease")
                    .frame(maxWidth: 260)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 8)
        }
        .toolbar {
            if sortChoices.count > 1 {
                ToolbarItem {
                    Menu {
                        let current = store.librarySort[view] ?? (.title, false)
                        ForEach(sortChoices, id: \.0) { choice in
                            Button {
                                store.sortLibrary(view, by: choice.0, reverse: current.reverse)
                            } label: {
                                if current.by == choice.0 {
                                    Label(L10n.t(choice.1), systemImage: "checkmark")
                                } else {
                                    Text(L10n.t(choice.1))
                                }
                            }
                        }
                        Divider()
                        Button(current.reverse ? L10n.t(.ascending) : L10n.t(.descending)) {
                            store.sortLibrary(view, by: current.by, reverse: !current.reverse)
                        }
                    } label: {
                        Label(L10n.t(.sort), systemImage: "arrow.up.arrow.down")
                    }
                    .help(L10n.t(.sort))
                }
            }
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
