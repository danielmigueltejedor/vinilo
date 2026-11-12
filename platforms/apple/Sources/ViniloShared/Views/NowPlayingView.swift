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
    @AppStorage("vinilo.lyricsOpen") private var lyricsOpen = false
    var close: () -> Void

    var body: some View {
        VStack(spacing: 22) {
            header
            HStack(spacing: 40) {
                player
                if lyricsOpen {
                    LyricsPanel()
                        .frame(maxWidth: 460)
                        .transition(.move(edge: .trailing).combined(with: .opacity))
                }
            }
            .animation(.smooth(duration: 0.3), value: lyricsOpen)
        }
        .padding(28)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .foregroundStyle(.white)
        .environment(\.colorScheme, .dark)
        .background { Backdrop(tint: store.tint, artPath: store.snapshot.artPath) }
        .animation(.easeInOut(duration: 0.6), value: store.tint)
    }

    private var header: some View {
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
            Button {
                lyricsOpen.toggle()
            } label: {
                Image(systemName: lyricsOpen ? "quote.bubble.fill" : "quote.bubble")
                    .font(.title3.weight(.semibold))
            }
            .transportButtonStyle()
            .help(L10n.t(.lyrics))
            .accessibilityLabel(L10n.t(.lyrics))
        }
    }

    private var player: some View {
        VStack(spacing: 22) {
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

            TransportControls(size: 24)
            HStack(spacing: 28) {
                ModeControls()
                VolumeControl()
            }

            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity)
    }
}

/// The words, following the song when they are timed: the current line bright, the rest
/// dimmed, and the list kept on the current line as it moves.
private struct LyricsPanel: View {
    @Environment(PlayerStore.self) private var store

    var body: some View {
        Group {
            switch store.lyrics {
            case .loaded(_, let lyrics) where !lyrics.lines.isEmpty:
                if lyrics.synced {
                    TimelineView(.periodic(from: .now, by: 0.25)) { context in
                        lines(lyrics, current: current(in: lyrics, at: context.date))
                    }
                } else {
                    lines(lyrics, current: nil)
                }
            case .loading:
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            default:
                Text(L10n.t(.noLyrics))
                    .font(.title3)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .onAppear { store.loadLyrics() }
        .onChange(of: store.snapshot.trackId) { store.loadLyrics() }
    }

    private func current(in lyrics: Lyrics, at date: Date) -> Int? {
        let position = store.position(at: date)
        return lyrics.lines.lastIndex { $0.startMs <= position }
    }

    private func lines(_ lyrics: Lyrics, current: Int?) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    ForEach(Array(lyrics.lines.enumerated()), id: \.offset) { index, line in
                        Text(line.text.isEmpty ? "♪" : line.text)
                            .font(.title2.weight(.bold))
                            .foregroundStyle(.white.opacity(current == nil || index == current ? 1 : 0.38))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .id(index)
                    }
                }
                .padding(.vertical, 120)
            }
            .scrollIndicators(.hidden)
            .mask(
                LinearGradient(
                    stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.12),
                        .init(color: .black, location: 0.88), .init(color: .clear, location: 1)],
                    startPoint: .top, endPoint: .bottom))
            .onChange(of: current) { _, line in
                guard let line else { return }
                withAnimation(.smooth(duration: 0.4)) { proxy.scrollTo(line, anchor: .center) }
            }
        }
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
