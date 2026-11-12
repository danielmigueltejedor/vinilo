// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Testing
import ViniloKit

@testable import ViniloShared

@MainActor
@Suite("Pinned playlists")
struct PinTests {
    private func playlist(_ id: String) -> Entry {
        .playlist(Playlist(id: id, name: "List \(id)", artwork: Artwork(template: "https://x/{w}x{h}.jpg")))
    }

    @Test("pinning toggles, keeps the order and survives a new store")
    func pinning() throws {
        let suite = "vinilo.tests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        let store = PlayerStore(defaults: defaults)
        store.togglePin(playlist("a"))
        store.togglePin(playlist("b"))
        store.togglePin(playlist("c"))
        #expect(store.pins.map(\.id) == ["a", "b", "c"])
        #expect(store.isPinned(playlist("b")))

        store.movePins(from: IndexSet(integer: 2), to: 0)
        #expect(store.pins.map(\.id) == ["c", "a", "b"])
        store.togglePin(playlist("a"))
        #expect(store.pins.map(\.id) == ["c", "b"])

        let again = PlayerStore(defaults: defaults)
        #expect(again.pins.map(\.id) == ["c", "b"])
        #expect(again.pins.first?.name == "List c")
    }

    @Test("a song cannot be pinned; without defaults pins last only the session")
    func onlyPages() {
        let store = PlayerStore()
        store.togglePin(.song(Track(id: "1", catalogId: "1", title: "t", artist: "a", album: "b")))
        #expect(store.pins.isEmpty)
    }
}
