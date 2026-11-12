// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

// The daemon's wire format, as Swift types. The source of truth is `crates/vinilo-core/src/ipc.rs`;
// `Tests/ViniloKitTests/Fixtures` are generated from it by a Rust test, so a field renamed there
// fails here instead of on somebody's player.

public enum RepeatMode: String, Codable, Sendable, CaseIterable {
    case none, one, all
}

public enum LibraryView: String, Codable, Sendable, CaseIterable {
    case songs, albums, artists, playlists
}

public enum PageKind: String, Codable, Sendable {
    case album, artist, playlist, libraryAlbum, libraryArtist, libraryPlaylist
}

public enum SortBy: String, Codable, Sendable {
    case title, artist, album, year, added
}

public enum CatalogFilter: String, Codable, Sendable {
    case all, songs, albums, artists, playlists
}

public enum PlayMode: String, Codable, Sendable {
    /// A row click: this list, from here, in order.
    case clicked
    /// A Play button: in order, whatever mode was on before.
    case inOrder
    /// A Shuffle button.
    case shuffled
}

public enum WriteAction: String, Codable, Sendable {
    case favorite, unfavorite, addToLibrary, removeFromLibrary
    case createPlaylist, addToPlaylist, removeFromPlaylist
}

/// Apple serves artwork as a URL template with `{w}x{h}` in it; the daemon passes it through.
public struct Artwork: Codable, Hashable, Sendable {
    public let template: String

    public init(template: String) {
        self.template = template
    }

    public init(from decoder: any Decoder) throws {
        template = try decoder.singleValueContainer().decode(String.self)
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(template)
    }

    /// A concrete address at `size`×`size` pixels. Templates with no placeholder come back as is.
    public func url(size: Int) -> URL? {
        let pixels = String(max(1, size))
        let text = template
            .replacingOccurrences(of: "{w}", with: pixels)
            .replacingOccurrences(of: "{h}", with: pixels)
            .replacingOccurrences(of: "{f}", with: "jpg")
            .replacingOccurrences(of: "{c}", with: "bb")
        return URL(string: text)
    }
}

public struct Track: Decodable, Hashable, Sendable {
    public var dateAdded: String
    public var year: String
    public var favorite: Bool
    public var inLibrary: Bool
    public var libraryId: String?
    public var id: String
    /// What the player can be handed. `nil` is a track that exists only in the library and cannot
    /// be streamed, which a row has to show as unavailable rather than offer.
    public var catalogId: String?
    public var title: String
    public var artist: String
    public var album: String
    public var durationMs: Int
    public var trackNumber: Int
    public var artwork: Artwork?

    public var isPlayable: Bool { catalogId != nil }

    public init(
        id: String, catalogId: String?, title: String, artist: String, album: String,
        durationMs: Int = 0, trackNumber: Int = 0, artwork: Artwork? = nil,
        year: String = "", favorite: Bool = false, inLibrary: Bool = false,
        libraryId: String? = nil, dateAdded: String = ""
    ) {
        self.id = id
        self.catalogId = catalogId
        self.title = title
        self.artist = artist
        self.album = album
        self.durationMs = durationMs
        self.trackNumber = trackNumber
        self.artwork = artwork
        self.year = year
        self.favorite = favorite
        self.inLibrary = inLibrary
        self.libraryId = libraryId
        self.dateAdded = dateAdded
    }
}

public struct Album: Decodable, Hashable, Sendable {
    public var id: String
    public var dateAdded: String
    public var name: String
    public var artist: String
    public var artwork: Artwork?
    public var year: String
    public var trackCount: Int
    /// True when `id` is a library id. The two id spaces are not interchangeable.
    public var library: Bool

    public init(
        id: String, name: String, artist: String, artwork: Artwork? = nil, year: String = "",
        trackCount: Int = 0, library: Bool = true, dateAdded: String = ""
    ) {
        self.id = id
        self.name = name
        self.artist = artist
        self.artwork = artwork
        self.year = year
        self.trackCount = trackCount
        self.library = library
        self.dateAdded = dateAdded
    }
}

public struct Playlist: Decodable, Hashable, Sendable {
    public var id: String
    public var dateAdded: String
    public var lastModified: String
    public var name: String
    public var curator: String
    public var description: String
    public var artwork: Artwork?
    public var library: Bool

    public init(
        id: String, name: String, curator: String = "", artwork: Artwork? = nil,
        library: Bool = true
    ) {
        self.id = id
        self.name = name
        self.curator = curator
        self.artwork = artwork
        self.library = library
        self.dateAdded = ""
        self.lastModified = ""
        self.description = ""
    }
}

public struct Artist: Decodable, Hashable, Sendable {
    public var id: String
    public var name: String
    public var artwork: Artwork?
    public var genres: String
    public var library: Bool

    public init(
        id: String, name: String, artwork: Artwork? = nil, genres: String = "",
        library: Bool = true
    ) {
        self.id = id
        self.name = name
        self.artwork = artwork
        self.genres = genres
        self.library = library
    }
}

/// A row in any list. Songs play; everything else opens a page.
public enum Entry: Decodable, Hashable, Sendable {
    case song(Track)
    case album(Album)
    case artist(Artist)
    case playlist(Playlist)

    private enum Key: String, CodingKey { case kind }

    public init(from decoder: any Decoder) throws {
        let kind = try decoder.container(keyedBy: Key.self).decode(String.self, forKey: .kind)
        switch kind {
        case "song": self = .song(try Track(from: decoder))
        case "album": self = .album(try Album(from: decoder))
        case "artist": self = .artist(try Artist(from: decoder))
        case "playlist": self = .playlist(try Playlist(from: decoder))
        default:
            throw DecodingError.dataCorrupted(
                .init(codingPath: decoder.codingPath, debugDescription: "unknown entry kind \(kind)")
            )
        }
    }

    public var id: String {
        switch self {
        case .song(let track): track.catalogId ?? track.id
        case .album(let album): album.id
        case .artist(let artist): artist.id
        case .playlist(let playlist): playlist.id
        }
    }

    public var title: String {
        switch self {
        case .song(let track): track.title
        case .album(let album): album.name
        case .artist(let artist): artist.name
        case .playlist(let playlist): playlist.name
        }
    }

    /// The second line, collapsing rather than leaving a dangling separator.
    public var subtitle: String {
        switch self {
        case .song(let track):
            [track.artist, track.album].filter { !$0.isEmpty }.joined(separator: " — ")
        case .album(let album):
            [album.artist, album.year].filter { !$0.isEmpty }.joined(separator: " · ")
        case .artist(let artist): artist.genres
        case .playlist(let playlist): playlist.curator
        }
    }

    public var artwork: Artwork? {
        switch self {
        case .song(let track): track.artwork
        case .album(let album): album.artwork
        case .artist(let artist): artist.artwork
        case .playlist(let playlist): playlist.artwork
        }
    }

    /// What opening this row asks the daemon for. **The library/catalog split comes from the
    /// flag, never from the id's shape**: a library id 404s against the catalog and back.
    public var pageTarget: PageRoute? {
        switch self {
        case .song: nil
        case .album(let album):
            PageRoute(kind: album.library ? .libraryAlbum : .album, id: album.id)
        case .artist(let artist):
            PageRoute(kind: artist.library ? .libraryArtist : .artist, id: artist.id)
        case .playlist(let playlist):
            PageRoute(kind: playlist.library ? .libraryPlaylist : .playlist, id: playlist.id)
        }
    }
}

/// Which page to open, and the key a loaded page is stored under.
public struct PageRoute: Hashable, Sendable {
    public var kind: PageKind
    public var id: String

    public init(kind: PageKind, id: String) {
        self.kind = kind
        self.id = id
    }
}

public struct QueueItem: Decodable, Hashable, Sendable {
    public var id: String?
    public var title: String
    public var artist: String
    public var album: String
    public var durationMs: Int

    public init(id: String?, title: String, artist: String, album: String, durationMs: Int) {
        self.id = id
        self.title = title
        self.artist = artist
        self.album = album
        self.durationMs = durationMs
    }
}

/// What is playing, flattened for a client that only draws.
public struct Snapshot: Decodable, Hashable, Sendable {
    public var trackId: String?
    public var title: String
    public var artist: String
    public var album: String
    /// A local file the daemon already fetched. A client does not ask Apple for it.
    public var artPath: String?
    public var positionMs: Int
    public var durationMs: Int
    public var playing: Bool
    /// Still working towards audio. Not the same as paused.
    public var busy: Bool
    public var volume: Double
    public var shuffle: Bool
    public var repeatMode: RepeatMode
    public var canNext: Bool
    public var canPrevious: Bool

    enum CodingKeys: String, CodingKey {
        case trackId, title, artist, album, artPath, positionMs, durationMs, playing, busy
        case volume, shuffle, canNext, canPrevious
        case repeatMode = "repeat"
    }

    public init(
        trackId: String? = nil, title: String = "", artist: String = "", album: String = "",
        artPath: String? = nil, positionMs: Int = 0, durationMs: Int = 0, playing: Bool = false,
        busy: Bool = false, volume: Double = 1, shuffle: Bool = false,
        repeatMode: RepeatMode = .none, canNext: Bool = false, canPrevious: Bool = false
    ) {
        self.trackId = trackId
        self.title = title
        self.artist = artist
        self.album = album
        self.artPath = artPath
        self.positionMs = positionMs
        self.durationMs = durationMs
        self.playing = playing
        self.busy = busy
        self.volume = volume
        self.shuffle = shuffle
        self.repeatMode = repeatMode
        self.canNext = canNext
        self.canPrevious = canPrevious
    }

    /// Nothing loaded.
    public static let idle = Snapshot()

    public var hasTrack: Bool { trackId != nil || !title.isEmpty }
}

/// How far along the daemon is.
public enum Stage: Hashable, Sendable {
    /// The sidecar is up but MusicKit has not attached yet.
    case connecting
    case ready
    /// No Apple session. Only a window can fix this, and the daemon asks the sidecar for one.
    case signedOut
    /// Broken in a way a restart will not fix.
    case broken(String)
}

public struct LyricLine: Decodable, Hashable, Sendable {
    public var startMs: Int
    public var text: String
}

public struct Lyrics: Decodable, Hashable, Sendable {
    public var synced: Bool
    public var lines: [LyricLine]
}

/// Listen Now: recently played, recommendations, charts.
public struct Discover: Decodable, Hashable, Sendable {
    public var provider: String
    public var recentlyPlayed: [Entry]
    public var recommendedPlaylists: [Entry]
    public var recommendedSongs: [Track]
    public var recentlyAdded: [Entry]
    public var charts: [Entry]

    public init(
        provider: String = "", recentlyPlayed: [Entry] = [], recommendedPlaylists: [Entry] = [],
        recommendedSongs: [Track] = [], recentlyAdded: [Entry] = [], charts: [Entry] = []
    ) {
        self.provider = provider
        self.recentlyPlayed = recentlyPlayed
        self.recommendedPlaylists = recommendedPlaylists
        self.recommendedSongs = recommendedSongs
        self.recentlyAdded = recentlyAdded
        self.charts = charts
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        provider = try c.decodeIfPresent(String.self, forKey: .provider) ?? ""
        recentlyPlayed = try c.decodeIfPresent([Entry].self, forKey: .recentlyPlayed) ?? []
        recommendedPlaylists =
            try c.decodeIfPresent([Entry].self, forKey: .recommendedPlaylists) ?? []
        recommendedSongs = try c.decodeIfPresent([Track].self, forKey: .recommendedSongs) ?? []
        recentlyAdded = try c.decodeIfPresent([Entry].self, forKey: .recentlyAdded) ?? []
        charts = try c.decodeIfPresent([Entry].self, forKey: .charts) ?? []
    }

    private enum Key: String, CodingKey {
        case provider, recentlyPlayed, recommendedPlaylists, recommendedSongs, recentlyAdded, charts
    }

    public var isEmpty: Bool {
        recentlyPlayed.isEmpty && recommendedPlaylists.isEmpty && recommendedSongs.isEmpty
            && recentlyAdded.isEmpty && charts.isEmpty
    }
}

/// Daemon → client.
public enum Event: Decodable, Equatable, Sendable {
    case snapshot(Snapshot)
    case queue(items: [QueueItem], position: Int)
    case stage(Stage)
    case rows(view: LibraryView, entries: [Entry], total: Int)
    /// Carries the query it answers: a result for two keystrokes ago must not replace one for the
    /// word that is actually in the box.
    case results(query: String, entries: [Entry], offset: Int, more: Bool)
    case page(route: PageRoute, header: Entry, entries: [Entry])
    case discover(Discover)
    /// An invalidation, not the rows: ask again for the page being drawn.
    case libraryChanged
    case libraryRefreshing(Bool)
    case lyrics(id: String, lyrics: Lyrics?, error: String?)
    case error(String)

    private enum Key: String, CodingKey {
        case event, items, position, stage, detail, view, entries, total, query, offset, more
        case kind, id, header, refreshing, lyrics, error
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        let tag = try c.decode(String.self, forKey: .event)
        switch tag {
        case "snapshot":
            self = .snapshot(try Snapshot(from: decoder))
        case "queue":
            self = .queue(
                items: try c.decode([QueueItem].self, forKey: .items),
                position: try c.decode(Int.self, forKey: .position)
            )
        case "stage":
            switch try c.decode(String.self, forKey: .stage) {
            case "ready": self = .stage(.ready)
            case "signedOut": self = .stage(.signedOut)
            case "broken":
                self = .stage(.broken(try c.decodeIfPresent(String.self, forKey: .detail) ?? ""))
            default: self = .stage(.connecting)
            }
        case "rows":
            self = .rows(
                view: try c.decode(LibraryView.self, forKey: .view),
                entries: try c.decode([Entry].self, forKey: .entries),
                total: try c.decode(Int.self, forKey: .total)
            )
        case "results":
            self = .results(
                query: try c.decode(String.self, forKey: .query),
                entries: try c.decode([Entry].self, forKey: .entries),
                offset: try c.decode(Int.self, forKey: .offset),
                more: try c.decode(Bool.self, forKey: .more)
            )
        case "page":
            self = .page(
                route: PageRoute(
                    kind: try c.decode(PageKind.self, forKey: .kind),
                    id: try c.decode(String.self, forKey: .id)
                ),
                header: try c.decode(Entry.self, forKey: .header),
                entries: try c.decode([Entry].self, forKey: .entries)
            )
        case "discover":
            self = .discover(try Discover(from: decoder))
        case "libraryChanged":
            self = .libraryChanged
        case "libraryRefreshing":
            self = .libraryRefreshing(try c.decode(Bool.self, forKey: .refreshing))
        case "lyrics":
            self = .lyrics(
                id: try c.decode(String.self, forKey: .id),
                lyrics: try c.decodeIfPresent(Lyrics.self, forKey: .lyrics),
                error: try c.decodeIfPresent(String.self, forKey: .error)
            )
        case "error":
            self = .error(try c.decode(String.self, forKey: .detail))
        default:
            throw DecodingError.dataCorrupted(
                .init(codingPath: decoder.codingPath, debugDescription: "unknown event \(tag)")
            )
        }
    }
}

public enum WireDecoder {
    /// The decoder for everything the daemon sends: its keys are `snake_case`.
    public static func make() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }
}
