// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// Search across all of Apple Music, not just the library.
struct SearchView: View {
    @Environment(PlayerStore.self) private var store
    @Binding var text: String

    var body: some View {
        Group {
            if text.trimmingCharacters(in: .whitespaces).isEmpty {
                StatusView(
                    symbol: "magnifyingglass", title: L10n.t(.searchPrompt),
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
        .searchable(text: $text, prompt: L10n.t(.searchPrompt))
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

    private var trimmed: String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
