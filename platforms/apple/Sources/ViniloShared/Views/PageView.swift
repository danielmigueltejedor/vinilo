// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// An album, an artist or a playlist: the thing itself above what is in it. An artist's page
/// holds their albums, not their tracks, which is a different request to Apple and a different
/// shape here.
struct PageView: View {
    @Environment(PlayerStore.self) private var store
    var route: PageRoute
    /// The cover's colour, washed down from the top of the page as in Vinilo Nodalia.
    @State private var wash: RGB?

    var body: some View {
        Group {
            switch store.pages[route] {
            case .none, .some(.idle), .some(.loading):
                StatusView(symbol: "music.note.list", title: L10n.t(.loading), busy: true)
            case .some(.loaded(let page)):
                content(page)
            }
        }
        .navigationTitle(title)
        .task(id: route) { store.open(route) }
        .task(id: headerArtwork?.template) {
            wash = await store.tint(for: headerArtwork)
        }
    }

    private var headerArtwork: Artwork? {
        if case .loaded(let page) = store.pages[route] { return page.header.artwork }
        return nil
    }

    private var title: String {
        if case .loaded(let page) = store.pages[route] { return page.header.title }
        return ""
    }

    private func content(_ page: PlayerStore.Page) -> some View {
        let songs = page.entries.filter { if case .song = $0 { true } else { false } }
        return ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header(page.header, songs: songs)
                if songs.isEmpty {
                    EntryCollection(entries: page.entries)
                        .frame(minHeight: 200)
                } else {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(songs.enumerated()), id: \.offset) { index, entry in
                            if case .song(let track) = entry {
                                HStack(spacing: 8) {
                                    Text("\(index + 1)")
                                        .font(.callout.monospacedDigit())
                                        .foregroundStyle(.secondary)
                                        .frame(width: 28, alignment: .trailing)
                                        .accessibilityHidden(true)
                                    SongRow(track: track, showsArtwork: false) {
                                        store.play(entry, in: songs)
                                    }
                                }
                                .padding(.vertical, 3)
                                if index < songs.count - 1 { Divider() }
                            }
                        }
                    }
                }
            }
            .padding(24)
            // Inside the scroll, so the colour leaves with the header rather than staying
            // behind the list as a band under the toolbar.
            .background(alignment: .top) { HeaderWash(colour: wash, artwork: page.header.artwork) }
        }
        .softScrollEdges()
        .clearToolbar()
    }

    private func header(_ entry: Entry, songs: [Entry]) -> some View {
        HStack(alignment: .bottom, spacing: 28) {
            ArtworkView(artwork: entry.artwork, size: 232, shape: headerShape(entry))
                .shadow(color: .black.opacity(0.35), radius: 24, y: 12)
            VStack(alignment: .leading, spacing: 8) {
                Text(kindLabel(entry)).font(.caption.weight(.bold))
                    .foregroundStyle(wash == nil ? AnyShapeStyle(.secondary) : AnyShapeStyle(.white.opacity(0.75)))
                    .textCase(.uppercase)
                    .tracking(1.2)
                Text(entry.title).font(.system(size: 40, weight: .heavy)).lineLimit(3)
                    .foregroundStyle(wash == nil ? AnyShapeStyle(.primary) : AnyShapeStyle(.white))
                if !entry.subtitle.isEmpty {
                    Text(entry.subtitle).font(.title3.weight(.medium))
                        .foregroundStyle(wash == nil ? AnyShapeStyle(.secondary) : AnyShapeStyle(.white.opacity(0.8)))
                }
                if !songs.isEmpty {
                    HStack {
                        Button {
                            store.playAll(songs, shuffled: false)
                        } label: {
                            Label(L10n.t(.playAll), systemImage: "play.fill")
                        }
                        .transportButtonStyle(prominent: true, circle: false)
                        Button {
                            store.playAll(songs, shuffled: true)
                        } label: {
                            Label(L10n.t(.shuffleAll), systemImage: "shuffle")
                        }
                        .transportButtonStyle(circle: false)
                        if entry.pageTarget != nil, !isArtist(entry) {
                            Button {
                                store.togglePin(entry)
                            } label: {
                                Image(systemName: store.isPinned(entry) ? "pin.fill" : "pin")
                            }
                            .transportButtonStyle()
                            .help(store.isPinned(entry) ? L10n.t(.unpin) : L10n.t(.pin))
                            .accessibilityLabel(store.isPinned(entry) ? L10n.t(.unpin) : L10n.t(.pin))
                        }
                    }
                    .controlSize(.large)
                    // The page's own colour, not the playing song's: these act on this page.
                    .tint(wash.map(Color.init(rgb:)))
                    .padding(.top, 6)
                }
            }
            Spacer(minLength: 0)
        }
    }

    private func isArtist(_ entry: Entry) -> Bool {
        if case .artist = entry { return true }
        return false
    }

    private func headerShape(_ entry: Entry) -> ArtworkShape {
        if case .artist = entry { return .circle }
        return .rounded(14)
    }

    private func kindLabel(_ entry: Entry) -> String {
        switch entry {
        case .song: L10n.t(.song)
        case .album: L10n.t(.album)
        case .artist: L10n.t(.artist)
        case .playlist: L10n.t(.playlist)
        }
    }
}

/// The top of a page in its cover's colour: the cover itself enlarged and out of focus, the
/// colour over it, and a long fade into the window so the list below reads on the ordinary
/// background. The fade is eased, many stops rather than two, so it has no visible edge where
/// it ends. Without a colour yet, nothing is drawn.
private struct HeaderWash: View {
    var colour: RGB?
    var artwork: Artwork?

    /// An ease-out from opaque to clear: most of the colour stays near the header, and the
    /// last of it thins slowly enough that the eye cannot find where it stops.
    private var fade: [Gradient.Stop] {
        (0...12).map { step in
            let t = Double(step) / 12
            let remaining = 1 - t
            return Gradient.Stop(
                color: .black.opacity(remaining * remaining * (3 - 2 * remaining)), location: t)
        }
    }

    /// Solid for the header itself, then the fade over the rest of the height.
    private var maskStops: [Gradient.Stop] {
        let solid = [Gradient.Stop(color: .black, location: 0), Gradient.Stop(color: .black, location: 0.38)]
        let fading: [Gradient.Stop] = fade.map { stop in
            Gradient.Stop(color: stop.color, location: 0.38 + stop.location * 0.62)
        }
        return solid + fading
    }

    var body: some View {
        if let colour {
            let base = Color(rgb: colour)
            ZStack {
                base
                // A small cover scaled up: it is blurred anyway, so the large one would be wasted.
                ArtworkView(artwork: artwork, size: 160, shape: .rounded(0))
                    .scaleEffect(6)
                    .blur(radius: 60, opaque: true)
                    .saturation(1.25)
                    .opacity(0.5)
                // Darker towards the title, so white text holds on a pale cover.
                LinearGradient(
                    colors: [base.opacity(0.1), base.opacity(0.45), .black.opacity(0.18)],
                    startPoint: .top, endPoint: .bottom)
            }
            .frame(height: 520)
            .clipped()
            .mask(
                LinearGradient(
                    stops: maskStops,
                    startPoint: .top, endPoint: .bottom)
            )
            .ignoresSafeArea(edges: .top)
            .transition(.opacity)
            .accessibilityHidden(true)
        }
    }
}
