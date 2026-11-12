// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

// MARK: Liquid Glass
//
// Vinilo adopts the system's glass rather than drawing something like it. Toolbars, sidebars and
// standard controls pick it up from the SDK on their own; the few surfaces that are ours (the
// player bar, the transport buttons) ask for it by name where the system has it, and use the
// system's ordinary materials where it does not. The cover's colour reaches the glass through the
// system's own tint.

extension View {
    /// A surface of the system's glass on macOS 26 and iOS 26, the system's bar material before.
    /// `tint` colours the glass the way the system tints it, never with a paint of our own.
    @ViewBuilder
    func glassSurface<S: Shape>(in shape: S, tint: Color? = nil) -> some View {
        #if compiler(>=6.2)
            if #available(macOS 26.0, iOS 26.0, *) {
                self.glassEffect(tint.map { Glass.regular.tint($0.opacity(0.55)) } ?? Glass.regular, in: shape)
            } else {
                self.background(.bar, in: shape)
                    .background((tint ?? .clear).opacity(0.35), in: shape)
            }
        #else
            self.background(.bar, in: shape)
                .background((tint ?? .clear).opacity(0.35), in: shape)
        #endif
    }

    /// A round transport button.
    @ViewBuilder
    func transportButtonStyle(prominent: Bool = false) -> some View {
        #if compiler(>=6.2)
            if #available(macOS 26.0, iOS 26.0, *) {
                if prominent {
                    self.buttonStyle(.glassProminent)
                } else {
                    self.buttonStyle(.glass)
                }
            } else {
                self.buttonStyle(.borderless)
            }
        #else
            self.buttonStyle(.borderless)
        #endif
    }
}

// MARK: Artwork

enum ArtworkShape {
    case rounded(CGFloat)
    case circle

    var shape: AnyShape {
        switch self {
        case .rounded(let radius): AnyShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
        case .circle: AnyShape(Circle())
        }
    }
}

/// A cover, drawn at `size` points and fetched at twice that. Loading is lazy and cached by the
/// URL loading system, whose cache the app sizes at launch.
struct ArtworkView: View {
    var url: URL?
    var size: CGFloat
    var shape: ArtworkShape = .rounded(8)

    var body: some View {
        AsyncImage(url: url) { phase in
            switch phase {
            case .success(let image):
                image.resizable().scaledToFill()
            default:
                Rectangle()
                    .fill(.quaternary)
                    .overlay {
                        Image(systemName: "music.note")
                            .font(.system(size: size * 0.32))
                            .foregroundStyle(.secondary)
                    }
            }
        }
        .frame(width: size, height: size)
        .clipShape(shape.shape)
        .overlay { shape.shape.stroke(.separator, lineWidth: 0.5) }
        .accessibilityHidden(true)
    }

    init(artwork: Artwork?, size: CGFloat, shape: ArtworkShape = .rounded(8)) {
        self.url = artwork?.url(size: Int(size * 2))
        self.size = size
        self.shape = shape
    }

    /// A cover the daemon already saved to disk.
    init(file path: String?, size: CGFloat, shape: ArtworkShape = .rounded(8)) {
        self.url = path.map { URL(fileURLWithPath: $0) }
        self.size = size
        self.shape = shape
    }
}

// MARK: States

/// A screen that has nothing to list: loading, empty, failed, or waiting for a person.
struct StatusView<Actions: View>: View {
    var symbol: String
    var title: String
    var message: String?
    var busy = false
    @ViewBuilder var actions: Actions

    var body: some View {
        VStack(spacing: 14) {
            if busy {
                ProgressView().controlSize(.large)
            } else {
                Image(systemName: symbol)
                    .font(.system(size: 40, weight: .regular))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
            }
            Text(title).font(.title2.weight(.semibold)).multilineTextAlignment(.center)
            if let message {
                Text(message)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 420)
            }
            actions
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

extension StatusView where Actions == EmptyView {
    init(symbol: String, title: String, message: String? = nil, busy: Bool = false) {
        self.init(symbol: symbol, title: title, message: message, busy: busy) { EmptyView() }
    }
}

// MARK: Rows and tiles

/// An album, artist or playlist in a grid.
struct EntryTile: View {
    var entry: Entry
    var size: CGFloat = 168

    private var shape: ArtworkShape {
        if case .artist = entry { return .circle }
        return .rounded(10)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ArtworkView(artwork: entry.artwork, size: size, shape: shape)
            Text(entry.title).font(.callout.weight(.medium)).lineLimit(1)
            if !entry.subtitle.isEmpty {
                Text(entry.subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .frame(width: size, alignment: .leading)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

/// A song in a list. Playing is a double click on a Mac and a tap on a touch screen.
struct SongRow: View {
    @Environment(PlayerStore.self) private var store
    var track: Track
    var showsArtwork = true
    var onPlay: () -> Void

    private var isCurrent: Bool {
        track.catalogId != nil && track.catalogId == store.snapshot.trackId
    }

    var body: some View {
        HStack(spacing: 12) {
            if showsArtwork {
                ArtworkView(artwork: track.artwork, size: 36, shape: .rounded(5))
            }
            if isCurrent {
                Image(systemName: store.snapshot.playing ? "speaker.wave.2.fill" : "speaker.fill")
                    .foregroundStyle(.tint)
                    .frame(width: 18)
                    .accessibilityHidden(true)
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(track.title).lineLimit(1)
                Text(Entry.song(track).subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 8)
            if !track.isPlayable {
                Image(systemName: "icloud.slash")
                    .foregroundStyle(.secondary)
                    .help(L10n.t(.unavailable))
                    .accessibilityLabel(L10n.t(.unavailable))
            }
            if track.durationMs > 0 {
                Text(L10n.clock(ms: track.durationMs))
                    .font(.callout.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .opacity(track.isPlayable ? 1 : 0.55)
        .contentShape(Rectangle())
        #if os(macOS)
            .onTapGesture(count: 2) { if track.isPlayable { onPlay() } }
        #else
            .onTapGesture { if track.isPlayable { onPlay() } }
        #endif
        .contextMenu {
            if track.isPlayable {
                Button(L10n.t(.play), systemImage: "play.fill", action: onPlay)
                Button(L10n.t(.playNext), systemImage: "text.insert") {
                    store.enqueue(.song(track), next: true)
                }
                Button(L10n.t(.addToQueue), systemImage: "text.append") {
                    store.enqueue(.song(track), next: false)
                }
                if store.source == .appleMusic {
                    Divider()
                    Button(L10n.t(.addToLibrary), systemImage: "plus.circle") {
                        store.write(.addToLibrary, .song(track))
                    }
                    Button(L10n.t(.favorite), systemImage: "star") {
                        store.write(.favorite, .song(track))
                    }
                }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityAction(named: L10n.t(.play)) { if track.isPlayable { onPlay() } }
    }
}

/// Albums, artists and playlists as a grid; songs, if any, as rows above it.
struct EntryCollection: View {
    @Environment(PlayerStore.self) private var store
    var entries: [Entry]
    /// Asked for when the end of the list scrolls into view, which is how a search pages.
    var onReachEnd: (() -> Void)?

    init(entries: [Entry], onReachEnd: (() -> Void)? = nil) {
        self.entries = entries
        self.onReachEnd = onReachEnd
    }

    private var songs: [Entry] {
        entries.filter { if case .song = $0 { true } else { false } }
    }
    private var pages: [Entry] {
        entries.filter { $0.pageTarget != nil }
    }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 20, pinnedViews: []) {
                if !pages.isEmpty {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 168, maximum: 220), spacing: 20, alignment: .top)], alignment: .leading, spacing: 22) {
                        ForEach(Array(pages.enumerated()), id: \.offset) { _, entry in
                            if let route = entry.pageTarget {
                                NavigationLink(value: route) { EntryTile(entry: entry) }
                                    .buttonStyle(.plain)
                            }
                        }
                    }
                }
                if !songs.isEmpty {
                    LazyVStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(songs.enumerated()), id: \.offset) { _, entry in
                            if case .song(let track) = entry {
                                SongRow(track: track) { store.play(entry, in: songs) }
                                    .padding(.horizontal, 8).padding(.vertical, 3)
                            }
                        }
                    }
                }
                if let onReachEnd {
                    Color.clear.frame(height: 1).onAppear(perform: onReachEnd)
                }
            }
            .padding(20)
        }
    }
}
