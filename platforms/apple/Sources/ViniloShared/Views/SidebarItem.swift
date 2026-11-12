// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// Where the sidebar can take you.
public enum SidebarItem: String, CaseIterable, Hashable, Sendable {
    case home, search
    case artists, albums, songs, playlists

    var title: String {
        switch self {
        case .home: L10n.t(.home)
        case .search: L10n.t(.search)
        case .artists: L10n.t(.artists)
        case .albums: L10n.t(.albums)
        case .songs: L10n.t(.songs)
        case .playlists: L10n.t(.playlists)
        }
    }

    var symbol: String {
        switch self {
        case .home: "house"
        case .search: "magnifyingglass"
        case .artists: "music.mic"
        case .albums: "square.stack"
        case .songs: "music.note"
        case .playlists: "music.note.list"
        }
    }

    var libraryView: LibraryView? {
        switch self {
        case .artists: .artists
        case .albums: .albums
        case .songs: .songs
        case .playlists: .playlists
        case .home, .search: nil
        }
    }
}
