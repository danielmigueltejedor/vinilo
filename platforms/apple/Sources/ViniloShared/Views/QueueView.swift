// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// The queue the engine holds, drawn as it is. A click asks the engine to jump; the row that
/// plays changes when the engine says it did.
struct QueueView: View {
    @Environment(PlayerStore.self) private var store

    var body: some View {
        Group {
            if store.queue.isEmpty {
                StatusView(symbol: "list.bullet", title: L10n.t(.queueEmpty))
            } else {
                List {
                    ForEach(Array(store.queue.enumerated()), id: \.offset) { index, item in
                        row(item, index: index)
                    }
                    .onMove { from, to in
                        // `to` follows List's convention (insert before); the engine's is the
                        // landing position once the track has been taken out.
                        guard let source = from.first else { return }
                        store.moveInQueue(from: source, to: to > source ? to - 1 : to)
                    }
                    .onDelete { offsets in
                        for index in offsets.sorted(by: >) { store.removeFromQueue(index) }
                    }
                }
                .listStyle(.inset)
            }
        }
        .navigationTitle(L10n.t(.queue))
        .toolbar {
            ToolbarItem {
                Button(L10n.t(.clearQueue), systemImage: "trash") { store.clearQueue() }
                    .disabled(store.queue.isEmpty)
            }
        }
    }

    private func row(_ item: QueueItem, index: Int) -> some View {
        let current = index == store.queuePosition
        return HStack(spacing: 10) {
            if current {
                Image(systemName: store.snapshot.playing ? "speaker.wave.2.fill" : "speaker.fill")
                    .foregroundStyle(.tint).frame(width: 18).accessibilityHidden(true)
            } else {
                Text("\(index + 1)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    .frame(width: 18).accessibilityHidden(true)
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(item.title).lineLimit(1).fontWeight(current ? .semibold : .regular)
                Text(item.artist).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            Text(L10n.clock(ms: item.durationMs)).font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
        }
        .contentShape(Rectangle())
        .onTapGesture { store.jump(to: index) }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(current ? [.isButton, .isSelected] : .isButton)
    }
}
