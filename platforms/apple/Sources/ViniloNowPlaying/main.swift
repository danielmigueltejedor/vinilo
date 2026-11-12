// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

// Vinilo Nodalia's bridge to macOS: the GTK app cannot speak to Now Playing and the media keys,
// so this small process, started by it from the same bundle, mirrors its player there. It joins
// the daemon as an observer (it never keeps the player from quitting), uses the same Now Playing
// code as Vinilo Native, and ends when the app that started it ends.

import AppKit
import ViniloKit
import ViniloShared
import ViniloSystem

@MainActor
private func mirror() async {
    let store = PlayerStore()
    let system = SystemIntegration()
    system.start(store: store)
    let parent = getppid()
    // Ends with the app, idle or not: a bridge left behind would show a player that is gone.
    Task {
        while true {
            try? await Task.sleep(for: .seconds(2))
            if getppid() != parent { exit(0) }
        }
    }
    while true {
        if let path = DaemonPaths.socket(edition: .nodalia),
            let connection = try? DaemonConnection.connect(path: path)
        {
            store.attach(ConnectionLink(connection: connection), observer: true)
            for await incoming in connection.incoming {
                store.apply(incoming)
            }
            connection.close()
        }
        // The app starts the daemon a moment after this; until then, and after a restart of it,
        // the socket is simply not there yet.
        try? await Task.sleep(for: .seconds(2))
    }
}

private struct ConnectionLink: DaemonLink {
    let connection: DaemonConnection
    func send(_ request: Request) { connection.send(request) }
}

// Now Playing and the media keys belong to an app, so this is one: without a window, a Dock
// icon or a menu, but with the app's own bundle and so its name and icon in Control Center.
let application = NSApplication.shared
application.setActivationPolicy(.prohibited)
Task { await mirror() }
application.run()
