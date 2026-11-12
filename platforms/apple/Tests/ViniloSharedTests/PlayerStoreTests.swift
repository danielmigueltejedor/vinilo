// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Testing
import ViniloKit
import ViniloShared

@MainActor
@Suite("PlayerStore")
struct PlayerStoreTests {
    let recorder = Recorder()
    let store = PlayerStore()

    init() {
        store.attach(recorder)
        _ = recorder.drain()
    }

    // MARK: Connection

    @Test("attaching subscribes first, then asks for everything the screen needs")
    func attachSubscribesFirst() {
        let fresh = PlayerStore()
        let link = Recorder()
        fresh.attach(link)
        #expect(link.drain() == [.subscribe, .stage, .snapshot, .queue])
        #expect(fresh.connection == .connected)
    }

    @Test("what a screen was waiting for is asked again when the connection returns")
    func attachRepeatsPendingLoads() {
        let fresh = PlayerStore()
        fresh.loadLibrary(.songs)
        fresh.loadDiscover()
        fresh.open(PageRoute(kind: .album, id: "a1"))
        let link = Recorder()
        fresh.attach(link)
        let sent = link.drain()
        #expect(sent.contains(.browse(view: .songs)))
        #expect(sent.contains(.discover))
        #expect(sent.contains(.open(PageRoute(kind: .album, id: "a1"))))
    }

    @Test("after the daemon is lost nothing is sent and the reason is kept")
    func detachStopsSending() {
        store.detach(reason: "gone")
        store.playPause()
        #expect(recorder.drain().isEmpty)
        #expect(store.connection == .lost("gone"))
    }

    @Test("a line that could not be read is counted, not fatal")
    func unreadableLines() {
        store.apply(.unparsed("{"))
        store.apply(.unparsed("{"))
        #expect(store.unreadableLines == 2)
    }

    // MARK: Mirror

    @Test("a snapshot replaces the mirror and says whether the track changed")
    func snapshotReportsTrackChange() {
        var changes: [Bool] = []
        store.onSnapshot = { _, trackChanged in changes.append(trackChanged) }
        store.apply(.snapshot(Snapshot(trackId: "1", title: "One")))
        store.apply(.snapshot(Snapshot(trackId: "1", title: "One", positionMs: 4_000)))
        store.apply(.snapshot(Snapshot(trackId: "2", title: "Two")))
        #expect(changes == [true, false, true])
        #expect(store.snapshot.title == "Two")
    }

    @Test("the queue is whatever the daemon last said")
    func queueMirrors() {
        let items = [QueueItem(id: "1", title: "A", artist: "X", album: "Y", durationMs: 1_000)]
        store.apply(.queue(items: items, position: 0))
        #expect(store.queue == items)
        #expect(store.queuePosition == 0)
    }

    @Test("the position moves between snapshots while playing, and stops at the end")
    func positionAdvances() {
        store.apply(
            .snapshot(Snapshot(trackId: "1", positionMs: 10_000, durationMs: 12_000, playing: true)))
        let received = store.snapshotReceivedAt
        #expect(store.position(at: received) == 10_000)
        #expect(store.position(at: received.addingTimeInterval(1)) == 11_000)
        #expect(store.position(at: received.addingTimeInterval(60)) == 12_000)
        #expect(store.position(at: received.addingTimeInterval(-5)) == 10_000)
    }

    @Test("a paused or buffering player does not move")
    func positionHoldsWhenNotPlaying() {
        store.apply(.snapshot(Snapshot(trackId: "1", positionMs: 3_000, durationMs: 9_000, playing: false)))
        #expect(store.position(at: store.snapshotReceivedAt.addingTimeInterval(5)) == 3_000)
        store.apply(
            .snapshot(Snapshot(trackId: "1", positionMs: 3_000, durationMs: 9_000, playing: true, busy: true)))
        #expect(store.position(at: store.snapshotReceivedAt.addingTimeInterval(5)) == 3_000)
    }

    // MARK: Asking for things

    @Test("a section is asked for once, and again only when forced")
    func libraryLoadsOnce() {
        store.loadLibrary(.albums)
        store.loadLibrary(.albums)
        #expect(recorder.drain() == [.browse(view: .albums)])
        store.apply(.rows(view: .albums, entries: [], total: 0))
        store.loadLibrary(.albums, force: true)
        #expect(recorder.drain() == [.browse(view: .albums)])
    }

    @Test("an invalidation asks again only for the sections already held")
    func libraryChangedAsksForHeldSections() {
        store.loadLibrary(.songs)
        _ = recorder.drain()
        store.apply(.libraryChanged)
        #expect(recorder.drain() == [.browse(view: .songs)])
    }

    @Test("a failed request does not leave a screen spinning")
    func errorEndsLoading() {
        store.loadLibrary(.songs)
        store.loadDiscover()
        store.open(PageRoute(kind: .artist, id: "r"))
        store.searchCatalog("abc")
        store.apply(.error("nope"))
        #expect(store.library[.songs] == .some(.idle))
        #expect(store.discover == .idle)
        #expect(store.pages[PageRoute(kind: .artist, id: "r")] == .some(.idle))
        #expect(!store.searching)
        #expect(store.error == "nope")
        store.dismissError()
        #expect(store.error == nil)
    }

    @Test("a result for an older word never replaces the one in the box")
    func staleSearchIsDropped() {
        store.searchCatalog("ab")
        store.apply(.results(query: "a", entries: [.song(song("1"))], offset: 0, more: false))
        #expect(store.search == nil)
        store.apply(.results(query: "ab", entries: [.song(song("2"))], offset: 0, more: true))
        #expect(store.search?.entries.count == 1)
        #expect(!store.searching)
    }

    @Test("more results are appended to the page being shown")
    func searchPaginates() {
        store.searchCatalog("ab")
        store.apply(.results(query: "ab", entries: [.song(song("1"))], offset: 0, more: true))
        _ = recorder.drain()
        store.loadMoreSearch()
        #expect(recorder.drain() == [.search(query: "ab", offset: 1)])
        store.loadMoreSearch()
        #expect(recorder.drain().isEmpty, "a second ask while one is in flight is dropped")
        store.apply(.results(query: "ab", entries: [.song(song("2"))], offset: 1, more: false))
        #expect(store.search?.entries.count == 2)
        #expect(store.search?.more == false)
    }

    @Test("an empty query clears the results and sends nothing")
    func emptySearch() {
        store.searchCatalog("  \n")
        #expect(store.search == nil)
        #expect(recorder.drain().isEmpty)
    }

    // MARK: Playing

    @Test("playing a row queues the whole list from that row, never one track")
    func playQueuesTheList() {
        let entries: [Entry] = [
            .song(song("a")),
            .album(Album(id: "al", name: "Not a song", artist: "X")),
            .song(song("b")),
            .song(song("c")),
        ]
        store.play(entries[2], in: entries)
        #expect(recorder.drain() == [.play(ids: ["a", "b", "c"], index: 1, mode: .clicked)])
    }

    @Test("a track Apple will not stream is never sent to the player")
    func unplayableIsNotSent() {
        let entries: [Entry] = [.song(song(nil))]
        store.play(entries[0], in: entries)
        store.enqueue(entries[0], next: false)
        store.playAll(entries, shuffled: false)
        #expect(recorder.drain().isEmpty)
    }

    @Test("play all and shuffle all differ only in the start mode")
    func playAllModes() {
        let entries: [Entry] = [.song(song("a")), .song(song("b"))]
        store.playAll(entries, shuffled: false)
        store.playAll(entries, shuffled: true)
        #expect(
            recorder.drain() == [
                .play(ids: ["a", "b"], index: 0, mode: .inOrder),
                .play(ids: ["a", "b"], index: 0, mode: .shuffled),
            ])
    }

    @Test("seek and volume are kept inside what the player accepts")
    func transportIsClamped() {
        store.seek(toMs: -40)
        store.setVolume(3)
        store.setVolume(-1)
        #expect(
            recorder.drain() == [
                .transport(.seek(positionMs: 0)),
                .transport(.setVolume(1)),
                .transport(.setVolume(0)),
            ])
    }

    @Test("repeat cycles none, all, one, none from what the player says it is")
    func repeatCycles() {
        for (now, next) in [(RepeatMode.none, RepeatMode.all), (.all, .one), (.one, .none)] {
            store.apply(.snapshot(Snapshot(repeatMode: now)))
            store.cycleRepeat()
            #expect(recorder.drain() == [.transport(.setRepeat(next))])
        }
    }

    @Test("shuffle asks for the opposite of what the player reports")
    func shuffleToggles() {
        store.apply(.snapshot(Snapshot(shuffle: true)))
        store.toggleShuffle()
        #expect(recorder.drain() == [.transport(.setShuffle(false))])
    }

    @Test("play next and add to queue name the song and the place")
    func enqueue() {
        store.enqueue(.song(song("a")), next: true)
        store.enqueue(.song(song("b")), next: false)
        #expect(
            recorder.drain() == [
                .enqueue(ids: ["a"], next: true),
                .enqueue(ids: ["b"], next: false),
            ])
    }

    @Test("the queue is edited by asking, never by changing the mirror")
    func queueEditsAreRequests() {
        store.apply(.queue(items: [QueueItem(id: "1", title: "A", artist: "", album: "", durationMs: 0)], position: 0))
        store.removeFromQueue(0)
        store.moveInQueue(from: 0, to: 2)
        store.jump(to: 1)
        store.clearQueue()
        #expect(
            recorder.drain() == [.removeFromQueue(0), .moveInQueue(from: 0, to: 2), .jumpTo(1), .clearQueue])
        #expect(store.queue.count == 1)
    }
}
