// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import ViniloKit
import ViniloShared

/// Stands where the daemon would, and writes down what it was asked.
final class Recorder: DaemonLink, @unchecked Sendable {
    private let lock = NSLock()
    private var requests: [Request] = []

    func send(_ request: Request) {
        lock.lock()
        requests.append(request)
        lock.unlock()
    }

    /// What arrived so far, and forgets it, so a test reads only what its own step caused.
    func drain() -> [Request] {
        lock.lock()
        defer { lock.unlock() }
        let taken = requests
        requests = []
        return taken
    }
}

func song(_ id: String?, title: String = "Song") -> Track {
    Track(id: id ?? "library-only", catalogId: id, title: title, artist: "Artist", album: "Album")
}
