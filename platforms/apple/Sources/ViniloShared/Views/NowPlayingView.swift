// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// The expanded player: the cover large, the song, and the controls, over the window.
///
/// The sleeve dresses it: its colour from top to bottom, the cover itself enlarged and blurred
/// behind, and the text in white over both. Nothing animates continuously, so it costs nothing
/// while it is open.
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
        .foregroundStyle(.white)
        .environment(\.colorScheme, .dark)
        .background { Backdrop(tint: store.tint, artPath: store.snapshot.artPath) }
        .animation(.easeInOut(duration: 0.6), value: store.tint)
    }
}

/// The cover's colour, and the cover itself filling the window, out of focus.
private struct Backdrop: View {
    var tint: RGB?
    var artPath: String?

    var body: some View {
        let colour = tint.map(Color.init(rgb:)) ?? Color(white: 0.16)
        ZStack {
            colour
            if let artPath {
                GeometryReader { proxy in
                    let side = max(proxy.size.width, proxy.size.height) * 1.2
                    ArtworkView(file: artPath, size: side, shape: .rounded(0))
                        .frame(width: proxy.size.width, height: proxy.size.height)
                        .blur(radius: 70, opaque: true)
                        .opacity(0.55)
                }
            }
            LinearGradient(
                colors: [colour.opacity(0.35), colour.opacity(0.75), .black.opacity(0.55)],
                startPoint: .top, endPoint: .bottom)
        }
        .ignoresSafeArea()
        .accessibilityHidden(true)
    }
}
