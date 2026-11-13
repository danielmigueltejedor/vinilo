// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
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
            // Space is handled by `SpaceToPlay`, not here: a menu's key equivalent is matched
            // before the text field that has focus, so a search could never contain a space.
            Button(store.snapshot.playing ? L10n.t(.pause) : L10n.t(.play)) { store.playPause() }
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

/// Space plays and pauses, as in any Mac player, unless a text field is being typed in.
@MainActor
enum SpaceToPlay {
    private static var monitor: Any?

    static func install(store: PlayerStore) {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            let plain = event.modifierFlags.intersection(.deviceIndependentFlagsMask).subtracting(
                [.capsLock, .numericPad, .function]
            ).isEmpty
            guard plain, event.charactersIgnoringModifiers == " " else { return event }
            let played = MainActor.assumeIsolated { () -> Bool in
                if NSApp.keyWindow?.firstResponder is NSText { return false }
                store.playPause()
                return true
            }
            return played ? nil : event
        }
    }
}
