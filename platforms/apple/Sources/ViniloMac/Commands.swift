// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloShared

/// The Playback menu, with the shortcuts a Mac music player has.
struct PlaybackCommands: Commands {
    var store: PlayerStore

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button(L10n.t(.openFiles)) { store.choosingFiles = true }
                .keyboardShortcut("o")
        }
        CommandMenu(L10n.t(.playback)) {
            Button(store.snapshot.playing ? L10n.t(.pause) : L10n.t(.play)) { store.playPause() }
                .keyboardShortcut(.space, modifiers: [])
            Divider()
            Button(L10n.t(.next)) { store.next() }
                .keyboardShortcut(.rightArrow, modifiers: .command)
                .disabled(!store.snapshot.canNext)
            Button(L10n.t(.previous)) { store.previous() }
                .keyboardShortcut(.leftArrow, modifiers: .command)
                .disabled(!store.snapshot.canPrevious)
            Divider()
            Button(L10n.t(.shuffle)) { store.toggleShuffle() }
                .keyboardShortcut("s", modifiers: .command)
            Button(L10n.t(.repeatMode)) { store.cycleRepeat() }
                .keyboardShortcut("r", modifiers: .command)
            Divider()
            Button(L10n.t(.volumeUp)) { store.setVolume(store.snapshot.volume + 0.1) }
                .keyboardShortcut(.upArrow, modifiers: .command)
            Button(L10n.t(.volumeDown)) { store.setVolume(store.snapshot.volume - 0.1) }
                .keyboardShortcut(.downArrow, modifiers: .command)
        }
    }
}
