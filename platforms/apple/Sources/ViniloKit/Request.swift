// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A JSON value, so a request can be written down field by field exactly as the daemon's serde
/// types produce it (`snake_case` keys, explicit `null`s).
public enum JSONValue: Encodable, Equatable, Sendable {
    case string(String)
    case int(Int)
    case double(Double)
    case bool(Bool)
    case null
    case array([JSONValue])
    case object([String: JSONValue])

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let value): try container.encode(value)
        case .int(let value): try container.encode(value)
        case .double(let value): try container.encode(value)
        case .bool(let value): try container.encode(value)
        case .null: try container.encodeNil()
        case .array(let values): try container.encode(values)
        case .object(let values): try container.encode(values)
        }
    }
}

/// The transport verbs.
public enum Transport: Equatable, Sendable {
    case play, pause, playPause, next, previous
    case seek(positionMs: Int)
    case setVolume(Double)
    case setCrossfade(ms: Int)
    case setShuffle(Bool)
    case setRepeat(RepeatMode)

    fileprivate var fields: [String: JSONValue] {
        switch self {
        case .play: ["cmd": .string("play")]
        case .pause: ["cmd": .string("pause")]
        case .playPause: ["cmd": .string("playPause")]
        case .next: ["cmd": .string("next")]
        case .previous: ["cmd": .string("previous")]
        case .seek(let ms): ["cmd": .string("seek"), "position_ms": .int(ms)]
        case .setVolume(let volume): ["cmd": .string("setVolume"), "volume": .double(volume)]
        case .setCrossfade(let ms): ["cmd": .string("setCrossfade"), "ms": .int(ms)]
        case .setShuffle(let on): ["cmd": .string("setShuffle"), "shuffle": .bool(on)]
        case .setRepeat(let mode): ["cmd": .string("setRepeat"), "mode": .string(mode.rawValue)]
        }
    }
}

/// Client → daemon.
public enum Request: Equatable, Sendable {
    case subscribe
    /// Subscribe without counting as a window: the Now Playing bridge of Vinilo Nodalia.
    case observe
    case snapshot
    case queue
    case stage
    case signIn
    case signOut
    case discover
    case refresh
    case clearQueue
    case quit
    case transport(Transport)
    case jumpTo(Int)
    case browse(
        view: LibraryView, query: String = "", offset: Int = 0, limit: Int = 0,
        sort: SortBy = .title, reverse: Bool = false)
    case search(query: String, filter: CatalogFilter = .all, offset: Int = 0)
    case open(PageRoute)
    case enqueue(ids: [String], next: Bool = false)
    case removeFromQueue(Int)
    case moveInQueue(from: Int, to: Int)
    case write(action: WriteAction, id: String, playlistId: String? = nil, name: String? = nil)
    case play(ids: [String], index: Int = 0, mode: PlayMode = .clicked)
    /// Files or folders on this Mac, which the daemon's own decoder plays.
    case playFiles(paths: [String], index: Int = 0)
    case lyrics(id: String)

    /// The request as the daemon reads it: one JSON object.
    public var fields: [String: JSONValue] {
        switch self {
        case .subscribe: ["req": .string("subscribe")]
        case .observe: ["req": .string("observe")]
        case .snapshot: ["req": .string("snapshot")]
        case .queue: ["req": .string("queue")]
        case .stage: ["req": .string("stage")]
        case .signIn: ["req": .string("signIn")]
        case .signOut: ["req": .string("signOut")]
        case .discover: ["req": .string("discover")]
        case .refresh: ["req": .string("refresh")]
        case .clearQueue: ["req": .string("clearQueue")]
        case .quit: ["req": .string("quit")]
        case .transport(let transport):
            transport.fields.merging(["req": .string("transport")]) { $1 }
        case .jumpTo(let index):
            ["req": .string("jumpTo"), "index": .int(index)]
        case .browse(let view, let query, let offset, let limit, let sort, let reverse):
            [
                "req": .string("browse"), "view": .string(view.rawValue), "query": .string(query),
                "offset": .int(offset), "limit": .int(limit), "sort": .string(sort.rawValue),
                "reverse": .bool(reverse),
            ]
        case .search(let query, let filter, let offset):
            [
                "req": .string("search"), "query": .string(query),
                "filter": .string(filter.rawValue), "offset": .int(offset),
            ]
        case .open(let route):
            ["req": .string("open"), "kind": .string(route.kind.rawValue), "id": .string(route.id)]
        case .enqueue(let ids, let next):
            ["req": .string("enqueue"), "ids": .array(ids.map(JSONValue.string)), "next": .bool(next)]
        case .removeFromQueue(let index):
            ["req": .string("removeFromQueue"), "index": .int(index)]
        case .moveInQueue(let from, let to):
            ["req": .string("moveInQueue"), "from": .int(from), "to": .int(to)]
        case .write(let action, let id, let playlistId, let name):
            [
                "req": .string("write"), "action": .string(action.rawValue), "id": .string(id),
                "playlist_id": playlistId.map(JSONValue.string) ?? .null,
                "name": name.map(JSONValue.string) ?? .null,
            ]
        case .play(let ids, let index, let mode):
            [
                "req": .string("play"), "ids": .array(ids.map(JSONValue.string)),
                "index": .int(index), "start": .string(mode.rawValue),
            ]
        case .playFiles(let paths, let index):
            ["req": .string("playFiles"), "paths": .array(paths.map(JSONValue.string)), "index": .int(index)]
        case .lyrics(let id):
            ["req": .string("lyrics"), "id": .string(id)]
        }
    }

    /// One line of the stream: the JSON object and its newline.
    public func line() throws -> Data {
        var data = try JSONEncoder().encode(fields)
        data.append(0x0A)
        return data
    }
}
