// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import UniformTypeIdentifiers
import ViniloKit

/// The window: sidebar, content, the player at the bottom, the queue on the side, and Now
/// Playing over everything.
///
/// The system draws the chrome. `NavigationSplitView`, the toolbar and the inspector are the
/// platform's own and take on Liquid Glass with the SDK; what is drawn here is the music.
public struct RootView: View {
    @Environment(PlayerStore.self) private var store
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// Restored with the window.
    @SceneStorage("vinilo.sidebar") private var selectionName = SidebarItem.home.rawValue
    @SceneStorage("vinilo.queueOpen") private var queueOpen = false
    @State private var nowPlayingOpen = false
    @State private var searchText = ""

    public init() {}

    private var item: SidebarItem {
        SidebarItem(rawValue: selectionName) ?? .home
    }

    /// A pinned playlist or album, when that is what the sidebar has selected.
    private var pinned: PlayerStore.Pin? {
        store.pins.first { Sidebar.tag(for: $0) == selectionName }
    }

    private var selection: Binding<String?> {
        Binding(
            get: { selectionName },
            set: { if let value = $0 { selectionName = value } }
        )
    }

    public var body: some View {
        ZStack {
            NavigationSplitView {
                Sidebar(selection: selection)
            } detail: {
                Detail(item: item, pinned: pinned, searchText: $searchText)
                    .safeAreaInset(edge: .bottom, spacing: 0) {
                        PlayerBar(openNowPlaying: toggleNowPlaying)
                    }
            }
            .inspector(isPresented: $queueOpen) {
                QueueView()
                    .inspectorColumnWidth(min: 260, ideal: 320, max: 420)
            }
            // The search field of the whole window, in the sidebar where the system draws it in
            // its glass. Typing in it from any section goes to Search.
            .searchable(text: $searchText, placement: .sidebar, prompt: Text(searchPrompt))
            .onChange(of: searchText) { _, text in
                if !text.trimmingCharacters(in: .whitespaces).isEmpty, item != .search {
                    selectionName = SidebarItem.search.rawValue
                }
            }
            .softScrollEdges()
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        queueOpen.toggle()
                    } label: {
                        Label(L10n.t(.queue), systemImage: "list.bullet")
                    }
                    .help(L10n.t(.queue))
                }
            }

            if nowPlayingOpen {
                NowPlayingView(close: toggleNowPlaying)
                    .transition(reduceMotion ? .opacity : .move(edge: .bottom).combined(with: .opacity))
                    .zIndex(1)
            }
        }
        .overlay(alignment: .top) { Banner() }
        .tint(store.tint.map(Color.init(rgb:)))
        .fileImporter(
            isPresented: Bindable(store).choosingFiles, allowedContentTypes: [.audio, .folder],
            allowsMultipleSelection: true
        ) { result in
            if case .success(let urls) = result { store.playFiles(urls) }
        }
    }

    private var searchPrompt: String {
        String(format: L10n.t(.searchIn), store.source.displayName)
    }

    private func toggleNowPlaying() {
        withAnimation(reduceMotion ? nil : .smooth(duration: 0.35)) {
            nowPlayingOpen.toggle()
        }
    }
}

/// Said once and out of the way: the engine was lost, or Apple refused something.
private struct Banner: View {
    @Environment(PlayerStore.self) private var store

    var body: some View {
        VStack(spacing: 8) {
            if case .lost = store.connection {
                row(
                    symbol: "bolt.horizontal.circle", title: L10n.t(.engineLost),
                    message: L10n.t(.engineLostBody), dismiss: nil)
            }
            if let error = store.error {
                row(
                    symbol: "exclamationmark.triangle", title: error, message: nil,
                    dismiss: { store.dismissError() })
            }
        }
        .padding(12)
        .frame(maxWidth: 520)
    }

    private func row(symbol: String, title: String, message: String?, dismiss: (() -> Void)?)
        -> some View
    {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: symbol).foregroundStyle(.secondary).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.callout.weight(.medium))
                if let message { Text(message).font(.caption).foregroundStyle(.secondary) }
            }
            Spacer(minLength: 0)
            if let dismiss {
                Button(L10n.t(.dismiss), systemImage: "xmark", action: dismiss)
                    .labelStyle(.iconOnly)
                    .buttonStyle(.borderless)
            }
        }
        .padding(12)
        .glassSurface(in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .accessibilityElement(children: .combine)
    }
}

private struct Sidebar: View {
    @Environment(PlayerStore.self) private var store
    @Binding var selection: String?

    static func tag(for pin: PlayerStore.Pin) -> String { "pin:\(pin.kind.rawValue):\(pin.id)" }

    var body: some View {
        List(selection: $selection) {
            Section {
                row(.home)
                row(.search)
            }
            Section(L10n.t(.library)) {
                row(.artists)
                row(.albums)
                row(.songs)
                row(.playlists)
            }
            if !store.pins.isEmpty {
                Section(L10n.t(.pinned)) {
                    ForEach(store.pins) { pin in
                        HStack(spacing: 8) {
                            ArtworkView(artwork: pin.artwork.map(Artwork.init(template:)), size: 22, shape: .rounded(5))
                            Text(pin.name).lineLimit(1)
                        }
                        .tag(Self.tag(for: pin))
                        .contextMenu {
                            Button(L10n.t(.unpin), systemImage: "pin.slash") { store.unpin(pin) }
                        }
                    }
                    .onMove { store.movePins(from: $0, to: $1) }
                }
            }
            Section(L10n.t(.source)) {
                Label(store.source.displayName, systemImage: store.source.symbol)
                if store.source == .local {
                    Button {
                        store.choosingFiles = true
                    } label: {
                        Label(L10n.t(.openFiles), systemImage: "plus.circle")
                    }
                    .buttonStyle(.plain)
                }
                ChangeSourceButton()
            }
        }
        .navigationTitle(L10n.t(.appName))
        .navigationSplitViewColumnWidth(min: 190, ideal: 220, max: 300)
    }

    private func row(_ item: SidebarItem) -> some View {
        Label(item.title, systemImage: item.symbol).tag(item.rawValue)
    }
}

/// What the selected sidebar item shows, behind the engine's own states: starting, signed out,
/// broken. Nothing is drawn from data the engine has not sent.
private struct Detail: View {
    @Environment(PlayerStore.self) private var store
    var item: SidebarItem
    var pinned: PlayerStore.Pin?
    @Binding var searchText: String

    var body: some View {
        switch store.stage {
        case .connecting:
            StatusView(symbol: "hourglass", title: L10n.t(.connecting), busy: true)
        case .signedOut where store.source != .appleMusic:
            StatusView(
                symbol: store.source.symbol,
                title: String(format: L10n.t(.loginTitle), store.source.displayName),
                message: L10n.t(.sourceBody), busy: false
            ) {
                ChangeSourceButton().buttonStyle(.borderedProminent).controlSize(.large)
            }
        case .signedOut:
            StatusView(
                symbol: "person.crop.circle.badge.questionmark", title: L10n.t(.signedOutTitle),
                message: L10n.t(.signedOutBody), busy: false
            ) {
                Button(L10n.t(.signIn)) { store.signIn() }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
            }
        case .broken(let detail):
            StatusView(
                symbol: "exclamationmark.triangle", title: L10n.t(.brokenTitle), message: detail)
        case .ready:
            NavigationStack {
                Group {
                    if let pinned {
                        PageView(route: pinned.route)
                    } else {
                        content
                    }
                }
                .navigationDestination(for: PageRoute.self) { PageView(route: $0) }
            }
            // A new section starts a new stack, not a page pushed onto the old one.
            .id(pinned.map(Sidebar.tag(for:)) ?? item.rawValue)
        }
    }

    @ViewBuilder private var content: some View {
        switch item {
        case .home where store.source == .local: LocalHome()
        case .home: HomeView()
        case .search: SearchView(text: $searchText)
        case .artists, .albums, .songs, .playlists:
            if let view = item.libraryView { LibrarySection(view: view, title: item.title) }
        }
    }
}

/// Files have no shelves to recommend: the home of the local source is the way to pick them.
private struct LocalHome: View {
    @Environment(PlayerStore.self) private var store

    var body: some View {
        StatusView(
            symbol: "folder", title: L10n.t(.localFiles), message: L10n.t(.localBody), busy: false
        ) {
            Button(L10n.t(.openFiles)) { store.choosingFiles = true }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
        }
        .navigationTitle(L10n.t(.home))
    }
}

/// Sources are chosen in Settings, where signing in to one has room.
struct ChangeSourceButton: View {
    #if os(macOS)
        @Environment(\.openSettings) private var openSettings
    #endif

    var body: some View {
        #if os(macOS)
            Button {
                openSettings()
            } label: {
                Label(L10n.t(.changeSource), systemImage: "arrow.triangle.2.circlepath")
            }
        #else
            EmptyView()
        #endif
    }
}

extension Color {
    init(rgb: RGB) {
        self.init(red: rgb.red, green: rgb.green, blue: rgb.blue)
    }
}
