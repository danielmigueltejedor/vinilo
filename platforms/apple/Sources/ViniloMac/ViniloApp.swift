// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit
import ViniloShared
import ViniloSystem

@main
struct ViniloApp: App {
    @State private var store = PlayerStore(sources: SourceConfig(edition: .native), defaults: .standard)
    @State private var supervisor = DaemonSupervisor()
    @State private var system = SystemIntegration()
    @State private var updater = AppUpdater()

    init() {
        // Covers are the only thing the app caches itself, and a cache has a ceiling.
        URLCache.shared = URLCache(memoryCapacity: 48 << 20, diskCapacity: 256 << 20)
    }

    var body: some Scene {
        // One window: a second player window would only be a second view of the same engine.
        Window(L10n.t(.appName), id: "main") {
            RootView()
                .environment(store)
                .frame(minWidth: 920, minHeight: 600)
                .sheet(isPresented: Binding(get: { updater.presented }, set: { updater.presented = $0 })) {
                    UpdateSheet(updater: updater)
                }
                .task {
                    system.start(store: store)
                    updater.start()
                    SpaceToPlay.install(store: store)
                    await supervisor.run(store: store)
                }
        }
        .defaultSize(width: 1180, height: 760)
        .commands {
            PlaybackCommands(store: store)
            UpdateCommands(updater: updater)
        }

        Settings {
            SettingsView()
                .environment(store)
        }
    }
}
