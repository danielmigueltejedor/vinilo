// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Observation
import ViniloKit

/// Where requests go. The macOS app hands the store a real connection; a test hands it a recorder.
public protocol DaemonLink: Sendable {
    func send(_ request: Request)
}

/// What the interface draws, and what it asks for.
///
/// **A mirror, not a source of truth.** The daemon (and MusicKit behind it) owns the queue and the
/// player; this holds what they last said. An intent sends a request and waits for the echo, so
/// the screen never shows a state the player is not in.
@MainActor
@Observable
public final class PlayerStore {
    public enum Connection: Equatable, Sendable {
        case connecting
        case connected
        case lost(String)
    }

    /// One library section, or one page: what was asked for and what came back.
    public enum Load<Value: Equatable & Sendable>: Equatable, Sendable {
        case idle
        case loading
        case loaded(Value)
    }

    public struct Rows: Equatable, Sendable {
        public var entries: [Entry]
        public var total: Int
    }

    public struct Page: Equatable, Sendable {
        public var header: Entry
        public var entries: [Entry]
    }

    public struct SearchResults: Equatable, Sendable {
        public var query: String
        public var entries: [Entry]
        public var more: Bool
        public var loadingMore: Bool
    }

    public private(set) var connection: Connection = .connecting
    public private(set) var stage: Stage = .connecting
    public private(set) var snapshot: Snapshot = .idle
    /// When `snapshot` arrived, so a progress bar can move between events.
    public private(set) var snapshotReceivedAt: Date = .distantPast
    public private(set) var queue: [QueueItem] = []
    public private(set) var queuePosition: Int = 0
    public private(set) var library: [LibraryView: Load<Rows>] = [:]
    public private(set) var libraryRefreshing = false
    public private(set) var discover: Load<Discover> = .idle
    public private(set) var search: SearchResults?
    public private(set) var searching = false
    public private(set) var pages: [PageRoute: Load<Page>] = [:]
    /// The last thing a person should be told. Cleared by `dismissError`.
    public private(set) var error: String?
    /// Lines the app could not read: the engine and the app disagree on the wire format.
    public private(set) var unreadableLines = 0
    /// Where the music comes from. The engine reads it when it starts.
    public private(set) var source: MusicSource
    /// The colour of the cover playing now, once it has been read.
    public private(set) var tint: RGB?
    /// Set to ask the window for its file chooser, from a menu or a button.
    public var choosingFiles = false
    /// The words of the track playing now: nil until asked for, then what the engine found.
    public private(set) var lyrics: LyricsState = .none

    public enum LyricsState: Equatable, Sendable {
        case none
        case loading(trackId: String)
        case loaded(trackId: String, Lyrics)
        case missing(trackId: String)
    }

    @ObservationIgnored private var link: (any DaemonLink)?
    @ObservationIgnored private var wantedSearch = ""
    /// What a search asks the catalogue for. Changing it searches again.
    public var searchFilter: CatalogFilter = .all {
        didSet { if searchFilter != oldValue { searchCatalog(wantedSearch) } }
    }
    /// How each library section is ordered, as the engine sorts it.
    public private(set) var librarySort: [LibraryView: (by: SortBy, reverse: Bool)] = [:]
    @ObservationIgnored private let sources: SourceConfig?

    /// Called after a snapshot is applied. The macOS app hangs the system's Now Playing on it.
    @ObservationIgnored public var onSnapshot: (@MainActor (Snapshot, _ trackChanged: Bool) -> Void)?

    /// `sources` is the engine's configuration folder; without one the store stays on Apple
    /// Music and cannot switch.
    public init(sources: SourceConfig? = nil) {
        self.sources = sources
        source = sources?.current() ?? .appleMusic
    }

    // MARK: Connection

    public func attach(_ link: any DaemonLink) {
        self.link = link
        connection = .connected
        // Subscribed first, so nothing asked for can be answered before it is listened for.
        link.send(.subscribe)
        link.send(.stage)
        link.send(.snapshot)
        link.send(.queue)
        // Whatever a screen was waiting for when the connection dropped is asked for again.
        for (view, state) in library where state == .loading { requestLibrary(view) }
        if discover == .loading { link.send(.discover) }
        for (route, state) in pages where state == .loading { link.send(.open(route)) }
    }

    public func detach(reason: String) {
        link = nil
        connection = .lost(reason)
    }

    public func connectionAttempt() {
        connection = .connecting
    }

    public func dismissError() {
        error = nil
    }

    private func send(_ request: Request) {
        link?.send(request)
    }

    // MARK: Events

    public func apply(_ incoming: Incoming) {
        switch incoming {
        case .event(let event): apply(event)
        case .unparsed: unreadableLines += 1
        case .lost(let reason): detach(reason: reason)
        }
    }

    public func apply(_ event: Event) {
        switch event {
        case .snapshot(let next):
            let trackChanged = next.trackId != snapshot.trackId
            let coverChanged = next.artPath != snapshot.artPath
            snapshot = next
            if coverChanged { readTint(next.artPath) }
            snapshotReceivedAt = Date()
            onSnapshot?(next, trackChanged)
        case .queue(let items, let position):
            queue = items
            queuePosition = position
        case .stage(let next):
            stage = next
            if next == .ready { loadAfterReady() }
        case .rows(let view, let entries, let total):
            library[view] = .loaded(Rows(entries: entries, total: total))
        case .results(let query, let entries, let offset, let more):
            // A result for two keystrokes ago must not replace one for the word in the box.
            guard query == wantedSearch else { return }
            searching = false
            if offset > 0, var current = search, current.query == query {
                current.entries += entries
                current.more = more
                current.loadingMore = false
                search = current
            } else {
                search = SearchResults(query: query, entries: entries, more: more, loadingMore: false)
            }
        case .page(let route, let header, let entries):
            pages[route] = .loaded(Page(header: header, entries: entries))
        case .discover(let value):
            discover = .loaded(value)
        case .libraryChanged:
            // An invalidation, not the rows: ask again only for what a screen already holds.
            for view in library.keys { requestLibrary(view) }
        case .libraryRefreshing(let on):
            libraryRefreshing = on
        case .lyrics(let id, let found, _):
            // An answer for the previous track must not land on this one.
            guard id == snapshot.trackId else { return }
            lyrics = found.map { .loaded(trackId: id, $0) } ?? .missing(trackId: id)
        case .error(let detail):
            error = detail
            // A failed request must not leave a screen spinning forever.
            for (view, state) in library where state == .loading { library[view] = .idle }
            if discover == .loading { discover = .idle }
            for (route, state) in pages where state == .loading { pages[route] = .idle }
            searching = false
        }
    }

    private func loadAfterReady() {
        for (view, state) in library where state == .loading { requestLibrary(view) }
        if discover == .loading { send(.discover) }
        for (route, state) in pages where state == .loading { send(.open(route)) }
    }

    // MARK: Asking for things

    /// Position now, moving between snapshots while playing.
    public func position(at date: Date) -> Int {
        guard snapshot.playing, !snapshot.busy else { return snapshot.positionMs }
        let elapsed = Int(date.timeIntervalSince(snapshotReceivedAt) * 1000)
        let moved = snapshot.positionMs + max(0, elapsed)
        return snapshot.durationMs > 0 ? min(moved, snapshot.durationMs) : moved
    }

    public func loadLibrary(_ view: LibraryView, force: Bool = false) {
        if !force, library[view] != nil { return }
        if library[view] == nil { library[view] = .loading }
        requestLibrary(view)
    }

    private func requestLibrary(_ view: LibraryView) {
        let sort = librarySort[view] ?? (.title, false)
        send(.browse(view: view, sort: sort.by, reverse: sort.reverse))
    }

    public func sortLibrary(_ view: LibraryView, by: SortBy, reverse: Bool) {
        librarySort[view] = (by, reverse)
        requestLibrary(view)
    }

    /// Library and favourite writes: Apple answers "accepted", so the library is asked again
    /// rather than edited here.
    public func write(_ action: WriteAction, _ entry: Entry) {
        send(.write(action: action, id: entry.id))
        refreshLibrary()
    }

    public func loadDiscover(force: Bool = false) {
        if !force, discover != .idle { return }
        discover = .loading
        send(.discover)
    }

    public func refreshLibrary() {
        send(.refresh)
    }

    public func searchCatalog(_ query: String) {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        wantedSearch = trimmed
        guard !trimmed.isEmpty else {
            search = nil
            searching = false
            return
        }
        searching = true
        send(.search(query: trimmed, filter: searchFilter))
    }

    public func loadMoreSearch() {
        guard var current = search, current.more, !current.loadingMore, current.query == wantedSearch
        else { return }
        current.loadingMore = true
        search = current
        send(.search(query: current.query, filter: searchFilter, offset: current.entries.count))
    }

    public func open(_ route: PageRoute) {
        if case .loaded = pages[route] { return }
        pages[route] = .loading
        send(.open(route))
    }

    // MARK: Playing

    /// Plays `entries` from `entry`: the whole list becomes the queue, so what follows is what
    /// is on screen. Never a one-track queue, which would cost the gapless transition.
    public func play(_ entry: Entry, in entries: [Entry]) {
        let songs = entries.compactMap { row -> String? in
            if case .song(let track) = row { return track.catalogId }
            return nil
        }
        guard case .song(let chosen) = entry, let id = chosen.catalogId,
            let index = songs.firstIndex(of: id)
        else { return }
        send(.play(ids: songs, index: index, mode: .clicked))
    }

    public func playAll(_ entries: [Entry], shuffled: Bool) {
        let songs = entries.compactMap { row -> String? in
            if case .song(let track) = row { return track.catalogId }
            return nil
        }
        guard !songs.isEmpty else { return }
        send(.play(ids: songs, index: 0, mode: shuffled ? .shuffled : .inOrder))
    }

    public func playPause() { send(.transport(.playPause)) }
    public func play() { send(.transport(.play)) }
    public func pause() { send(.transport(.pause)) }
    public func next() { send(.transport(.next)) }
    public func previous() { send(.transport(.previous)) }
    public func seek(toMs position: Int) { send(.transport(.seek(positionMs: max(0, position)))) }
    public func setVolume(_ volume: Double) { send(.transport(.setVolume(min(1, max(0, volume))))) }
    public func toggleShuffle() { send(.transport(.setShuffle(!snapshot.shuffle))) }

    public func cycleRepeat() {
        let next: RepeatMode =
            switch snapshot.repeatMode {
            case .none: .all
            case .all: .one
            case .one: .none
            }
        send(.transport(.setRepeat(next)))
    }

    public func jump(to index: Int) { send(.jumpTo(index)) }
    public func removeFromQueue(_ index: Int) { send(.removeFromQueue(index)) }
    public func moveInQueue(from: Int, to: Int) { send(.moveInQueue(from: from, to: to)) }
    public func clearQueue() { send(.clearQueue) }

    public func enqueue(_ entry: Entry, next: Bool) {
        guard case .song(let track) = entry, let id = track.catalogId else { return }
        send(.enqueue(ids: [id], next: next))
    }

    public func playFiles(_ urls: [URL]) {
        let paths = urls.filter(\.isFileURL).map(\.path)
        guard !paths.isEmpty else { return }
        send(.playFiles(paths: paths))
    }

    // MARK: Source

    /// Writes the choice and restarts the engine on it: it reads the source only when it starts.
    public func choose(_ next: MusicSource) throws {
        guard let sources else { return }
        try sources.choose(next)
        source = next
        restartEngine()
    }

    public func isConfigured(_ candidate: MusicSource) -> Bool {
        sources?.isConfigured(candidate) ?? (candidate == .appleMusic)
    }

    /// What was drawn belonged to the engine that is going; the next one answers afresh.
    private func restartEngine() {
        library = [:]
        discover = .idle
        pages = [:]
        search = nil
        searching = false
        stage = .connecting
        send(.quit)
    }

    private func readTint(_ path: String?) {
        guard let path else {
            tint = nil
            return
        }
        Task {
            let colour = await Task.detached(priority: .utility) { CoverTint.of(file: path) }.value
            if snapshot.artPath == path { tint = colour }
        }
    }

    /// Asks once per track; the panel calls it whenever it shows.
    public func loadLyrics() {
        guard let id = snapshot.trackId else {
            lyrics = .none
            return
        }
        switch lyrics {
        case .loading(let current) where current == id, .loaded(let current, _) where current == id,
            .missing(let current) where current == id:
            return
        default:
            lyrics = .loading(trackId: id)
            send(.lyrics(id: id))
        }
    }

    public func signIn() { send(.signIn) }
    public func signOut() { send(.signOut) }
}
