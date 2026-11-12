// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// Previous, play or pause, next. The state is the engine's: a button that cannot do anything
/// is disabled because the engine said so, not because the interface guessed.
struct TransportControls: View {
    @Environment(PlayerStore.self) private var store
    var size: CGFloat = 20

    var body: some View {
        HStack(spacing: 10) {
            Button { store.previous() } label: {
                Image(systemName: "backward.fill").font(.system(size: size * 0.8))
            }
            .disabled(!store.snapshot.canPrevious)
            .accessibilityLabel(L10n.t(.previous))

            Button { store.playPause() } label: {
                Image(systemName: store.snapshot.playing ? "pause.fill" : "play.fill")
                    .font(.system(size: size))
                    .frame(width: size * 1.2)
            }
            .transportButtonStyle(prominent: true)
            .accessibilityLabel(store.snapshot.playing ? L10n.t(.pause) : L10n.t(.play))

            Button { store.next() } label: {
                Image(systemName: "forward.fill").font(.system(size: size * 0.8))
            }
            .disabled(!store.snapshot.canNext)
            .accessibilityLabel(L10n.t(.next))
        }
        .transportButtonStyle()
        .controlSize(.large)
    }
}

/// Shuffle and repeat, which show the engine's modes.
struct ModeControls: View {
    @Environment(PlayerStore.self) private var store

    var body: some View {
        HStack(spacing: 4) {
            Button { store.toggleShuffle() } label: {
                Image(systemName: "shuffle")
                    .foregroundStyle(store.snapshot.shuffle ? Color.accentColor : Color.secondary)
            }
            .accessibilityLabel(L10n.t(.shuffle))
            .accessibilityValue(store.snapshot.shuffle ? "On" : "Off")

            Button { store.cycleRepeat() } label: {
                Image(systemName: store.snapshot.repeatMode == .one ? "repeat.1" : "repeat")
                    .foregroundStyle(
                        store.snapshot.repeatMode == .none ? Color.secondary : Color.accentColor)
            }
            .accessibilityLabel(L10n.t(.repeatMode))
            .accessibilityValue(store.snapshot.repeatMode.rawValue)
        }
        .buttonStyle(.borderless)
    }
}

/// Where the song is, and a way to move it. Moves smoothly between the engine's snapshots only
/// while playing; a paused player does no work.
struct ProgressScrubber: View {
    @Environment(PlayerStore.self) private var store
    @State private var dragging: Double?

    var body: some View {
        let duration = store.snapshot.durationMs
        HStack(spacing: 8) {
            Ticking(active: store.snapshot.playing && !store.snapshot.busy) { date in
                let position = displayed(at: date, duration: duration)
                HStack(spacing: 8) {
                    Text(L10n.clock(ms: Int(position))).monospacedDigit()
                        .font(.caption).foregroundStyle(.secondary)
                        .frame(width: 44, alignment: .trailing)
                    Slider(
                        value: Binding(
                            get: { dragging ?? position },
                            set: { dragging = $0 }),
                        in: 0...Double(max(duration, 1)),
                        onEditingChanged: { editing in
                            if !editing, let value = dragging {
                                store.seek(toMs: Int(value))
                                dragging = nil
                            }
                        }
                    )
                    .disabled(duration <= 0)
                    .accessibilityLabel(L10n.t(.progress))
                    .accessibilityValue(
                        "\(L10n.clock(ms: Int(position))) / \(L10n.clock(ms: duration))")
                }
            }
            Text(L10n.clock(ms: duration)).monospacedDigit()
                .font(.caption).foregroundStyle(.secondary).frame(width: 44, alignment: .leading)
        }
    }

    private func displayed(at date: Date, duration: Int) -> Double {
        Double(min(store.position(at: date), max(duration, 0)))
    }
}

/// Re-draws its content twice a second while `active`, and not at all otherwise: a paused
/// player does no work.
struct Ticking<Content: View>: View {
    var active: Bool
    @ViewBuilder var content: (Date) -> Content

    var body: some View {
        if active {
            TimelineView(.periodic(from: .now, by: 0.5)) { context in
                content(context.date)
            }
        } else {
            content(Date())
        }
    }
}

struct VolumeControl: View {
    @Environment(PlayerStore.self) private var store

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "speaker.fill").font(.caption).foregroundStyle(.secondary)
                .accessibilityHidden(true)
            Slider(
                value: Binding(
                    get: { store.snapshot.volume }, set: { store.setVolume($0) }),
                in: 0...1
            )
            .frame(width: 96)
            .accessibilityLabel(L10n.t(.volume))
            Image(systemName: "speaker.wave.3.fill").font(.caption).foregroundStyle(.secondary)
                .accessibilityHidden(true)
        }
    }
}
