// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// The player at the bottom of every screen: what is playing, the transport, the position, the
/// volume. It is an island of the system's glass floating over the content, a capsule held
/// clear of the window's edges, so the music stays the subject and the controls stay in reach.
///
/// It is not tinted: the cover's colour belongs to the expanded player, and a bar that changed
/// colour with every song would pull the eye away from the page above it.
struct PlayerBar: View {
    @Environment(PlayerStore.self) private var store
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    var openNowPlaying: () -> Void
    @State private var volumeOpen = false

    var body: some View {
        HStack(spacing: 16) {
            Button(action: openNowPlaying) {
                HStack(spacing: 12) {
                    // Concentric with the capsule: a round cover in a round end.
                    ArtworkView(file: store.snapshot.artPath, size: 46, shape: .circle)
                        .shadow(color: .black.opacity(0.25), radius: 5, y: 2)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(store.snapshot.hasTrack ? store.snapshot.title : L10n.t(.nothingPlaying))
                            .font(.callout.weight(.semibold)).lineLimit(1)
                        if !store.snapshot.artist.isEmpty {
                            Text(store.snapshot.artist).font(.caption).foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }
                    .frame(minWidth: 110, idealWidth: 190, maxWidth: 220, alignment: .leading)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(!store.snapshot.hasTrack)
            .accessibilityLabel(L10n.t(.expand))
            .accessibilityValue(store.snapshot.title)

            TransportControls(size: 17, prominent: false)
            ProgressScrubber().frame(minWidth: 140)
            ExpandingVolumeControl(open: $volumeOpen)
        }
        .padding(.leading, 8)
        .padding(.trailing, 12)
        .padding(.vertical, 8)
        .glassSurface(in: Capsule())
        .shadow(color: .black.opacity(0.12), radius: 16, y: 6)
        .tint(Color?.none)
        .animation(reduceMotion ? nil : .smooth(duration: 0.32), value: volumeOpen)
        .frame(maxWidth: 1040)
        .padding(.horizontal, 22)
        .padding(.bottom, 14)
    }
}
