// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// An album, an artist or a playlist: the thing itself above what is in it. An artist's page
/// holds their albums, not their tracks, which is a different request to Apple and a different
/// shape here.
struct PageView: View {
    @Environment(PlayerStore.self) private var store
    var route: PageRoute

    var body: some View {
        Group {
            switch store.pages[route] {
            case .none, .some(.idle), .some(.loading):
                StatusView(symbol: "music.note.list", title: L10n.t(.loading), busy: true)
            case .some(.loaded(let page)):
                content(page)
            }
        }
        .navigationTitle(title)
        .task(id: route) { store.open(route) }
    }

    private var title: String {
        if case .loaded(let page) = store.pages[route] { return page.header.title }
        return ""
    }

    private func content(_ page: PlayerStore.Page) -> some View {
        let songs = page.entries.filter { if case .song = $0 { true } else { false } }
        return ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header(page.header, songs: songs)
                if songs.isEmpty {
                    EntryCollection(entries: page.entries)
                        .frame(minHeight: 200)
                } else {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(songs.enumerated()), id: \.offset) { index, entry in
                            if case .song(let track) = entry {
                                HStack(spacing: 8) {
                                    Text("\(index + 1)")
                                        .font(.callout.monospacedDigit())
                                        .foregroundStyle(.secondary)
                                        .frame(width: 28, alignment: .trailing)
                                        .accessibilityHidden(true)
                                    SongRow(track: track, showsArtwork: false) {
                                        store.play(entry, in: songs)
                                    }
                                }
                                .padding(.vertical, 3)
                                if index < songs.count - 1 { Divider() }
                            }
                        }
                    }
                }
            }
            .padding(24)
        }
    }

    private func header(_ entry: Entry, songs: [Entry]) -> some View {
        HStack(alignment: .bottom, spacing: 24) {
            ArtworkView(artwork: entry.artwork, size: 220, shape: headerShape(entry))
            VStack(alignment: .leading, spacing: 8) {
                Text(kindLabel(entry)).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                    .textCase(.uppercase)
                Text(entry.title).font(.largeTitle.weight(.bold)).lineLimit(3)
                if !entry.subtitle.isEmpty {
                    Text(entry.subtitle).font(.title3).foregroundStyle(.secondary)
                }
                if !songs.isEmpty {
                    HStack {
                        Button {
                            store.playAll(songs, shuffled: false)
                        } label: {
                            Label(L10n.t(.playAll), systemImage: "play.fill")
                        }
                        .transportButtonStyle(prominent: true)
                        Button {
                            store.playAll(songs, shuffled: true)
                        } label: {
                            Label(L10n.t(.shuffleAll), systemImage: "shuffle")
                        }
                        .transportButtonStyle()
                    }
                    .controlSize(.large)
                    .padding(.top, 6)
                }
            }
            Spacer(minLength: 0)
        }
    }

    private func headerShape(_ entry: Entry) -> ArtworkShape {
        if case .artist = entry { return .circle }
        return .rounded(14)
    }

    private func kindLabel(_ entry: Entry) -> String {
        switch entry {
        case .song: L10n.t(.song)
        case .album: L10n.t(.album)
        case .artist: L10n.t(.artist)
        case .playlist: L10n.t(.playlist)
        }
    }
}
