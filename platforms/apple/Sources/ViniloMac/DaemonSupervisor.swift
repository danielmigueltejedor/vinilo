// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import ViniloKit
import ViniloShared

/// Keeps the app and the engine joined. The engine is started if it is not running, and started
/// again, with a growing pause, if it goes: a dead engine must never look like a healthy, silent
/// player.
@MainActor
final class DaemonSupervisor {
    private var connection: DaemonConnection?
    private var terminating = false
    private var observer: (any NSObjectProtocol)?

    init() {
        observer = NotificationCenter.default.addObserver(
            forName: NSApplication.willTerminateNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.shutdown() }
        }
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    /// The engine inside the app, or one named by `VINILOD_PATH` for a build run from the tree.
    private func launcher() -> DaemonLauncher? {
        if let bundled = DaemonLauncher.bundled(edition: .native) { return bundled }
        if let path = ProcessInfo.processInfo.environment["VINILOD_PATH"] {
            return DaemonLauncher(executable: URL(fileURLWithPath: path), edition: .native)
        }
        return nil
    }

    func run(store: PlayerStore) async {
        guard let launcher = launcher() else {
            store.detach(
                reason: "vinilod is not inside the app (set VINILOD_PATH when running from source)")
            return
        }
        var pause = 1.0
        while !Task.isCancelled && !terminating {
            store.connectionAttempt()
            do {
                let connection = try await launcher.connectOrSpawn()
                self.connection = connection
                store.attach(ConnectionLink(connection: connection))
                pause = 1
                for await incoming in connection.incoming {
                    store.apply(incoming)
                }
            } catch is CancellationError {
                return
            } catch {
                store.detach(reason: error.localizedDescription)
            }
            connection = nil
            if terminating { return }
            try? await Task.sleep(for: .seconds(pause))
            pause = min(pause * 2, 15)
        }
    }

    /// Quitting the app ends the music, as in any player. The engine refuses while another client
    /// holds it, so this cannot take a player from a window that is still open elsewhere.
    func shutdown() {
        terminating = true
        connection?.send(.quit)
        connection?.close()
    }
}

struct ConnectionLink: DaemonLink {
    let connection: DaemonConnection

    func send(_ request: Request) {
        connection.send(request)
    }
}
