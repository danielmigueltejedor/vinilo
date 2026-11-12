// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// The player at the bottom of every screen: what is playing, the transport, the position, the
/// volume. It is a floating surface of the system's glass over the content, so the music stays
/// the subject and the controls stay within reach.
struct PlayerBar: View {
    @Environment(PlayerStore.self) private var store
    var openNowPlaying: () -> Void

    var body: some View {
        HStack(spacing: 18) {
            Button(action: openNowPlaying) {
                HStack(spacing: 10) {
                    ArtworkView(file: store.snapshot.artPath, size: 44, shape: .rounded(6))
                    VStack(alignment: .leading, spacing: 1) {
                        Text(store.snapshot.hasTrack ? store.snapshot.title : L10n.t(.nothingPlaying))
                            .font(.callout.weight(.medium)).lineLimit(1)
                        if !store.snapshot.artist.isEmpty {
                            Text(store.snapshot.artist).font(.caption).foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }
                    .frame(minWidth: 120, idealWidth: 200, maxWidth: 240, alignment: .leading)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(!store.snapshot.hasTrack)
            .accessibilityLabel(L10n.t(.expand))
            .accessibilityValue(store.snapshot.title)

            TransportControls(size: 16)
            ProgressScrubber().frame(minWidth: 160)
            ModeControls()
            VolumeControl()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .glassSurface(in: RoundedRectangle(cornerRadius: 22, style: .continuous))
        .padding(.horizontal, 12)
        .padding(.bottom, 10)
    }
}
