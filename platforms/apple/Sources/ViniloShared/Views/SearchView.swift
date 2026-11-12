// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// Search across the whole catalogue of the source, not just the library.
struct SearchView: View {
    @Environment(PlayerStore.self) private var store
    @Binding var text: String

    var body: some View {
        Group {
            if text.trimmingCharacters(in: .whitespaces).isEmpty {
                StatusView(
                    symbol: "magnifyingglass", title: prompt,
                    message: L10n.t(.searchHint))
            } else if let results = store.search, results.query == trimmed {
                if results.entries.isEmpty {
                    StatusView(symbol: "magnifyingglass", title: L10n.t(.emptyResults))
                } else {
                    EntryCollection(entries: results.entries) {
                        if results.more { store.loadMoreSearch() }
                    }
                }
            } else {
                StatusView(symbol: "magnifyingglass", title: L10n.t(.loading), busy: true)
            }
        }
        .navigationTitle(L10n.t(.search))
        .searchable(text: $text, prompt: prompt)
        .searchScopes(Bindable(store).searchFilter) {
            Text(L10n.t(.all)).tag(CatalogFilter.all)
            Text(L10n.t(.songs)).tag(CatalogFilter.songs)
            Text(L10n.t(.albums)).tag(CatalogFilter.albums)
            Text(L10n.t(.artists)).tag(CatalogFilter.artists)
            Text(L10n.t(.playlists)).tag(CatalogFilter.playlists)
        }
        // Typing faster than Apple answers must not send a request per keystroke.
        .task(id: text) {
            do {
                try await Task.sleep(for: .milliseconds(300))
            } catch {
                return
            }
            store.searchCatalog(text)
        }
    }

    private var prompt: String {
        String(format: L10n.t(.searchIn), store.source.displayName)
    }

    private var trimmed: String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
