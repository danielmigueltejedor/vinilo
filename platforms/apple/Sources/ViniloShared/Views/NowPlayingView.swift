// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// The expanded player: the cover large, the song, and the controls, over the window.
///
/// It is the window's own background and the system's controls; the cover is the only colour.
/// Nothing animates continuously, so it costs nothing while it is open.
struct NowPlayingView: View {
    @Environment(PlayerStore.self) private var store
    var close: () -> Void

    var body: some View {
        VStack(spacing: 22) {
            HStack {
                Button(action: close) {
                    Image(systemName: "chevron.down").font(.title3.weight(.semibold))
                }
                .transportButtonStyle()
                .keyboardShortcut(.cancelAction)
                .accessibilityLabel(L10n.t(.close))
                Spacer()
                Text(L10n.t(.nowPlaying)).font(.headline).foregroundStyle(.secondary)
                Spacer()
                Color.clear.frame(width: 36, height: 1)
            }

            Spacer(minLength: 0)

            ArtworkView(file: store.snapshot.artPath, size: 360, shape: .rounded(18))
                .shadow(color: .black.opacity(0.25), radius: 24, y: 12)

            VStack(spacing: 4) {
                Text(store.snapshot.title).font(.title.weight(.bold)).lineLimit(2)
                    .multilineTextAlignment(.center)
                Text(store.snapshot.artist).font(.title3).foregroundStyle(.secondary).lineLimit(1)
                if !store.snapshot.album.isEmpty {
                    Text(store.snapshot.album).font(.callout).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            .accessibilityElement(children: .combine)

            ProgressScrubber().frame(maxWidth: 520)

            TransportControls(size: 26)
            HStack(spacing: 28) {
                ModeControls()
                VolumeControl()
            }

            Spacer(minLength: 0)
        }
        .padding(28)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.background)
    }
}
