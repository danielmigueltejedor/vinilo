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

    /// A round transport button: the system's glass on macOS 26, a circle of the bar material
    /// before. `circle: false` keeps a capsule for a button with words in it.
    @ViewBuilder
    func transportButtonStyle(prominent: Bool = false, circle: Bool = true) -> some View {
        #if compiler(>=6.2)
            if #available(macOS 26.0, iOS 26.0, *) {
                Group {
                    if prominent {
                        self.buttonStyle(.glassProminent)
                    } else {
                        self.buttonStyle(.glass)
                    }
                }
                .buttonBorderShape(circle ? .circle : .capsule)
            } else {
                self.buttonStyle(RoundTransportStyle(prominent: prominent, circle: circle))
            }
        #else
            self.buttonStyle(RoundTransportStyle(prominent: prominent, circle: circle))
        #endif
    }
}

extension View {
    /// Content that scrolls under the toolbar and the player fades into them, as on iOS 26,
    /// instead of meeting a solid bar. Before macOS 26 the system's own edge stays.
    @ViewBuilder
    func softScrollEdges() -> some View {
        #if compiler(>=6.2)
            if #available(macOS 26.0, iOS 26.0, *) {
                self.scrollEdgeEffectStyle(.soft, for: .all)
            } else {
                self
            }
        #else
            self
        #endif
    }

    /// The window's toolbar gone while something covers the whole window, so none of its
    /// buttons act on views nobody can see.
    @ViewBuilder
    func hidesToolbar(_ hidden: Bool) -> some View {
        #if os(macOS)
            self.toolbar(hidden ? .hidden : .automatic, for: .windowToolbar)
        #else
            self.toolbar(hidden ? .hidden : .automatic, for: .navigationBar)
        #endif
    }

    /// A translucent, blurred fade under the toolbar once the content has scrolled beneath it,
    /// so the title never sits on top of the rows. At the top it is gone and the page's own
    /// background reaches the window's edge. Ours, because the system's edge effect is off
    /// while the toolbar's background is hidden.
    /// `height` is how far down from the window's top edge it reaches: more where a bar of
    /// the page's own (a filter, search scopes) sits under the toolbar.
    func scrolledToolbarFade(height: CGFloat = 96) -> some View {
        modifier(ScrolledToolbarFade(height: height))
    }

    /// A toolbar with nothing behind it while the page is at its top, so a page's own colour
    /// reaches the window's edge without a band of the bar's material across it. Scrolled,
    /// the soft edge from `softScrollEdges` is what separates the two.
    @ViewBuilder
    func clearToolbar() -> some View {
        #if os(macOS)
            if #available(macOS 15.0, *) {
                self.toolbarBackgroundVisibility(.hidden, for: .windowToolbar)
            } else {
                self.toolbarBackground(.hidden, for: .windowToolbar)
            }
        #else
            self.toolbarBackground(.hidden, for: .navigationBar)
        #endif
    }
}

/// A search field in the system's glass: a capsule with a magnifier, the words, and a way to
/// clear them. Ours rather than `.searchable`, so it never becomes a toolbar item.
struct GlassSearchField: View {
    @Binding var text: String
    var prompt: String
    var symbol = "magnifyingglass"
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: symbol)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            TextField(prompt, text: $text)
                .textFieldStyle(.plain)
                .focused($focused)
                #if os(macOS)
                    .onExitCommand {
                        text = ""
                        focused = false
                    }
                #endif
            if !text.isEmpty {
                Button {
                    text = ""
                } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(L10n.t(.dismiss))
            }
        }
        .padding(.horizontal, 11)
        .padding(.vertical, 7)
        .glassSurface(in: Capsule())
        .contentShape(Capsule())
        .onTapGesture { focused = true }
    }
}

/// The round button before Liquid Glass: a disc of the bar material, the accent colour when it
/// is the main action, and a press that sinks a little.
struct RoundTransportStyle: ButtonStyle {
    var prominent: Bool
    var circle: Bool
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(circle ? 10 : 12)
            .padding(.horizontal, circle ? 0 : 6)
            .frame(minWidth: circle ? 36 : nil, minHeight: 36)
            .foregroundStyle(prominent ? AnyShapeStyle(.white) : AnyShapeStyle(.primary))
            .background {
                if prominent {
                    Capsule().fill(.tint)
                } else {
                    Capsule().fill(.bar)
                }
            }
            .clipShape(Capsule())
            .scaleEffect(configuration.isPressed ? 0.94 : 1)
            .opacity(isEnabled ? 1 : 0.4)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
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
    @Environment(PlayerStore.self) private var store
    var entry: Entry
    var size: CGFloat = 168
    @State private var hovering = false

    private var shape: ArtworkShape {
        if case .artist = entry { return .circle }
        return .rounded(10)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ArtworkView(artwork: entry.artwork, size: size, shape: shape)
                .shadow(color: .black.opacity(hovering ? 0.28 : 0.12), radius: hovering ? 14 : 6, y: hovering ? 8 : 3)
                .scaleEffect(hovering ? 1.03 : 1)
            Text(entry.title).font(.callout.weight(.semibold)).lineLimit(1)
            if !entry.subtitle.isEmpty {
                Text(entry.subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .frame(width: size, alignment: .leading)
        .contentShape(Rectangle())
        .onHover { inside in withAnimation(.smooth(duration: 0.2)) { hovering = inside } }
        .contextMenu {
            if canPin {
                Button(
                    store.isPinned(entry) ? L10n.t(.unpin) : L10n.t(.pin),
                    systemImage: store.isPinned(entry) ? "pin.slash" : "pin"
                ) { store.togglePin(entry) }
            }
        }
        .accessibilityElement(children: .combine)
    }

    /// Playlists and albums can live in the sidebar, as in Vinilo Nodalia.
    private var canPin: Bool {
        switch entry {
        case .playlist, .album: true
        default: false
        }
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
        // One click plays, as in Music; the context menu holds everything else.
        .onTapGesture { if track.isPlayable { onPlay() } }
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

/// The fade `scrolledToolbarFade()` draws: the system material under the toolbar, solid behind
/// the title and thinning to nothing below it, shown only while something is under it.
private struct ScrolledToolbarFade: ViewModifier {
    var height: CGFloat
    @State private var scrolled = false

    func body(content: Content) -> some View {
        content
            .modifier(ScrolledFlag(scrolled: $scrolled))
            .overlay(alignment: .top) {
                Rectangle()
                    .fill(.regularMaterial)
                    .mask(
                        LinearGradient(
                            stops: [
                                Gradient.Stop(color: .black, location: 0),
                                Gradient.Stop(color: .black, location: 0.5),
                                Gradient.Stop(color: .black.opacity(0.6), location: 0.72),
                                Gradient.Stop(color: .clear, location: 1),
                            ],
                            startPoint: .top, endPoint: .bottom)
                    )
                    .frame(height: height)
                    .ignoresSafeArea(edges: .top)
                    .opacity(scrolled ? 1 : 0)
                    .animation(.easeOut(duration: 0.2), value: scrolled)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
    }
}

/// Whether a scroll view has moved off its top. Needs macOS 15 / iOS 18 to read the offset;
/// before that the flag stays false and the toolbar keeps the system's own look.
private struct ScrolledFlag: ViewModifier {
    @Binding var scrolled: Bool

    @ViewBuilder
    func body(content: Content) -> some View {
        if #available(macOS 15.0, iOS 18.0, *) {
            content.onScrollGeometryChange(for: Bool.self) { geometry in
                geometry.contentOffset.y + geometry.contentInsets.top > 2
            } action: { _, isScrolled in
                scrolled = isScrolled
            }
        } else {
            content
        }
    }
}
