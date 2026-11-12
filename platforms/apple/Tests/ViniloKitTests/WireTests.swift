// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Testing

@testable import ViniloKit

private func fixtureLines(_ name: String) throws -> [Data] {
    let url = try #require(
        Bundle.module.url(forResource: name, withExtension: "jsonl", subdirectory: "Fixtures"))
    return try Data(contentsOf: url).split(separator: 0x0A).map { Data($0) }
}

@Suite("The wire format")
struct WireTests {
    @Test("every event the daemon sends decodes")
    func everyEventDecodes() throws {
        let decoder = WireDecoder.make()
        let lines = try fixtureLines("events")
        #expect(lines.count == 14)
        for line in lines {
            _ = try decoder.decode(Event.self, from: line)
        }
    }

    @Test("a snapshot keeps what a bar draws")
    func snapshot() throws {
        let lines = try fixtureLines("events")
        let event = try WireDecoder.make().decode(Event.self, from: lines[0])
        guard case .snapshot(let snapshot) = event else {
            Issue.record("not a snapshot")
            return
        }
        #expect(snapshot.title == "Fixture Song")
        #expect(snapshot.positionMs == 61_000)
        #expect(snapshot.durationMs == 215_000)
        #expect(snapshot.playing)
        #expect(snapshot.shuffle)
        #expect(snapshot.repeatMode == .all)
        #expect(snapshot.canNext)
        #expect(!snapshot.canPrevious)
        #expect(snapshot.artPath == "/tmp/cover.jpg")
    }

    @Test("the four stages")
    func stages() throws {
        let lines = try fixtureLines("events")
        let decoder = WireDecoder.make()
        let stages = try lines[2...5].map { try decoder.decode(Event.self, from: $0) }
        #expect(
            stages == [
                .stage(.connecting), .stage(.ready), .stage(.signedOut),
                .stage(.broken("the sidecar did not start")),
            ])
    }

    @Test("rows carry songs, albums, artists and playlists")
    func rows() throws {
        let lines = try fixtureLines("events")
        let event = try WireDecoder.make().decode(Event.self, from: lines[6])
        guard case .rows(let view, let entries, let total) = event else {
            Issue.record("not rows")
            return
        }
        #expect(view == .songs)
        #expect(total == 535)
        #expect(entries.count == 4)
        guard case .song(let track) = entries[0] else {
            Issue.record("first row is not a song")
            return
        }
        #expect(track.isPlayable)
        #expect(track.libraryId == "i.1440857781")
        #expect(track.artwork?.url(size: 300)?.absoluteString == "https://example.invalid/art/300x300bb.jpg")
        // A library album opens the library page, never the catalog one.
        #expect(entries[1].pageTarget == PageRoute(kind: .libraryAlbum, id: "l.album1"))
        #expect(entries[2].pageTarget == PageRoute(kind: .libraryArtist, id: "r.artist1"))
        #expect(entries[3].pageTarget == PageRoute(kind: .libraryPlaylist, id: "p.list1"))
        #expect(entries[0].pageTarget == nil)
    }

    @Test("an empty Listen Now is empty, not an error")
    func discover() throws {
        let lines = try fixtureLines("events")
        let event = try WireDecoder.make().decode(Event.self, from: lines[13])
        guard case .discover(let discover) = event else {
            Issue.record("not a discover")
            return
        }
        #expect(discover.isEmpty)
    }

    @Test("a page names what it opened")
    func page() throws {
        let lines = try fixtureLines("events")
        let event = try WireDecoder.make().decode(Event.self, from: lines[8])
        guard case .page(let route, let header, let entries) = event else {
            Issue.record("not a page")
            return
        }
        #expect(route == PageRoute(kind: .libraryAlbum, id: "l.album1"))
        #expect(header.title == "Fixture Album")
        #expect(entries.count == 1)
    }

    @Test("every request encodes exactly as the daemon's own types write it")
    func requests() throws {
        let sent: [Request] = [
            .subscribe, .snapshot, .queue, .stage,
            .transport(.play), .transport(.playPause), .transport(.next), .transport(.previous),
            .transport(.seek(positionMs: 4200)), .transport(.setVolume(0.5)),
            .transport(.setShuffle(true)), .transport(.setRepeat(.one)),
            .jumpTo(3), .signIn, .signOut,
            .browse(view: .albums, query: "abc", offset: 40, limit: 40, sort: .artist, reverse: true),
            .search(query: "fixture", filter: .songs, offset: 25),
            .open(PageRoute(kind: .libraryArtist, id: "r.artist1")),
            .discover, .enqueue(ids: ["1", "2"], next: true), .removeFromQueue(1),
            .moveInQueue(from: 1, to: 4), .clearQueue,
            .write(action: .addToLibrary, id: "1440857781"),
            .refresh, .play(ids: ["1", "2", "3"], index: 1, mode: .shuffled),
            .lyrics(id: "1440857781"), .quit,
        ]
        let expected = try fixtureLines("requests")
        #expect(sent.count == expected.count)
        for (request, line) in zip(sent, expected) {
            let ours = try JSONSerialization.jsonObject(with: request.line()) as? NSDictionary
            let theirs = try JSONSerialization.jsonObject(with: line) as? NSDictionary
            #expect(ours == theirs, "\(request)")
        }
    }

    @Test("a request ends with a newline and has no other")
    func oneLine() throws {
        let line = try Request.transport(.seek(positionMs: 10)).line()
        #expect(line.last == 0x0A)
        #expect(line.filter { $0 == 0x0A }.count == 1)
    }
}

@Suite("Cutting the stream into lines")
struct LineSplitterTests {
    @Test("a chunk that ends mid-line waits for the rest")
    func splitAcrossChunks() throws {
        var splitter = LineSplitter()
        #expect(try splitter.feed(Data("{\"a\":1}\n{\"b\"".utf8)) == [Data("{\"a\":1}".utf8)])
        #expect(try splitter.feed(Data(":2}\n".utf8)) == [Data("{\"b\":2}".utf8)])
    }

    @Test("several lines in one chunk, and blank ones skipped")
    func several() throws {
        var splitter = LineSplitter()
        let lines = try splitter.feed(Data("a\n\nb\nc\n".utf8))
        #expect(lines.map { String(decoding: $0, as: UTF8.self) } == ["a", "b", "c"])
    }

    @Test("a stream that never ends a line is dropped, not grown")
    func tooLong() {
        var splitter = LineSplitter()
        let chunk = Data(repeating: 0x61, count: 1 << 20)
        #expect(throws: LineSplitter.Failure.self) {
            for _ in 0...(LineSplitter.maxLineBytes >> 20) + 1 {
                _ = try splitter.feed(chunk)
            }
        }
    }
}

@Suite("Where the daemon listens")
struct PathTests {
    @Test("each edition has a socket of its own")
    func editionsDiffer() {
        let environment = ["TMPDIR": "/var/folders/xx/T/"]
        #expect(DaemonPaths.socket(edition: .native, environment: environment) == "/var/folders/xx/T/vinilo-native.sock")
        #expect(DaemonPaths.socket(edition: .nodalia, environment: environment) == "/var/folders/xx/T/vinilo.sock")
    }

    @Test("the per-user runtime directory wins, and nothing means nowhere")
    func runtimeDirectory() {
        #expect(
            DaemonPaths.socket(edition: .native, environment: ["XDG_RUNTIME_DIR": "/run/user/1", "TMPDIR": "/tmp"])
                == "/run/user/1/vinilo-native.sock")
        #expect(DaemonPaths.socket(edition: .native, environment: [:]) == nil)
        #expect(DaemonPaths.socket(edition: .native, environment: ["TMPDIR": ""]) == nil)
    }

    @Test("only the default edition leaves the daemon's environment alone")
    func environment() {
        #expect(Edition.nodalia.environmentValue == nil)
        #expect(Edition.native.environmentValue == "native")
        #expect(Edition.native.stem == "vinilo-native")
    }
}
