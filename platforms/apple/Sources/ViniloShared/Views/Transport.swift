// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit

/// Previous, play or pause, next. The state is the engine's: a button that cannot do anything
/// is disabled because the engine said so, not because the interface guessed.
///
/// `size` is the icon of play or pause; the buttons are drawn round it with room to breathe,
/// and play or pause is the largest of the three because it is the one people reach for.
/// `prominent` fills it with the tint, which the compact player leaves off so it stays neutral.
struct TransportControls: View {
    @Environment(PlayerStore.self) private var store
    var size: CGFloat = 20
    var prominent = true

    var body: some View {
        HStack(spacing: size * 0.5) {
            Button { store.previous() } label: {
                symbol("backward.fill", icon: size * 0.78, side: size * 1.9)
            }
            .disabled(!store.snapshot.canPrevious)
            .accessibilityLabel(L10n.t(.previous))

            Button { store.playPause() } label: {
                symbol(store.snapshot.playing ? "pause.fill" : "play.fill", icon: size, side: size * 2.5)
                    .contentTransition(.symbolEffect(.replace))
            }
            .transportButtonStyle(prominent: prominent)
            .accessibilityLabel(store.snapshot.playing ? L10n.t(.pause) : L10n.t(.play))

            Button { store.next() } label: {
                symbol("forward.fill", icon: size * 0.78, side: size * 1.9)
            }
            .disabled(!store.snapshot.canNext)
            .accessibilityLabel(L10n.t(.next))
        }
        .transportButtonStyle()
    }

    /// A symbol centred in a square of `side` points, so the glass round it is that big
    /// whatever the symbol's own width.
    private func symbol(_ name: String, icon: CGFloat, side: CGFloat) -> some View {
        Image(systemName: name)
            .font(.system(size: icon, weight: .semibold))
            .frame(width: side, height: side)
            .contentShape(Circle())
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

/// The volume as a slider beside two speakers, where there is room for one all the time.
struct VolumeControl: View {
    @Environment(PlayerStore.self) private var store

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "speaker.fill").font(.caption).foregroundStyle(.secondary)
                .accessibilityHidden(true)
            VolumeSlider(width: 96)
            Image(systemName: "speaker.wave.3.fill").font(.caption).foregroundStyle(.secondary)
                .accessibilityHidden(true)
        }
    }
}

/// Shuffle, repeat and the volume as Apple Music keeps them in a narrow bar: a speaker that,
/// pressed, opens a slider where shuffle and repeat were. It closes itself a few seconds after
/// the last change, or on a second press of the speaker.
///
/// Both states share one slot of a fixed width with the speaker at the same place, so opening
/// and closing never resize the bar or push anything out of it, and the slot is drawn as part
/// of the bar's own glass rather than as a second pane of glass on top of it.
struct ExpandingVolumeControl: View {
    @Environment(PlayerStore.self) private var store
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Binding var open: Bool
    /// Moved by every change, so the timer that closes the slider starts again.
    @State private var touched = 0

    private static let slot: CGFloat = 172
    private static let speaker: CGFloat = 32

    var body: some View {
        HStack(spacing: 6) {
            ZStack(alignment: .trailing) {
                ModeControls()
                    .opacity(open ? 0 : 1)
                    .allowsHitTesting(!open)
                    .accessibilityHidden(open)
                if open {
                    VolumeSlider(width: Self.slot - Self.speaker - 26) { touched &+= 1 }
                        .padding(.leading, 12)
                        .transition(.opacity)
                }
            }
            .frame(maxWidth: .infinity, alignment: .trailing)

            Button {
                withAnimation(reduceMotion ? nil : .smooth(duration: 0.28)) { open.toggle() }
            } label: {
                Image(systemName: speakerSymbol)
                    .font(.system(size: 14, weight: .semibold))
                    .contentTransition(.symbolEffect(.replace))
                    .frame(width: Self.speaker, height: Self.speaker)
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(open ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
            .accessibilityLabel(L10n.t(.volume))
            .accessibilityValue("\(Int((store.snapshot.volume * 100).rounded())) %")
        }
        .frame(width: Self.slot, height: Self.speaker + 4)
        .background {
            Capsule()
                .fill(.primary.opacity(open ? 0.08 : 0))
        }
        .clipShape(Capsule())
        .task(id: TimerKey(open: open, touched: touched)) {
            guard open else { return }
            do {
                try await Task.sleep(for: .seconds(4))
            } catch {
                return
            }
            withAnimation(reduceMotion ? nil : .smooth(duration: 0.28)) { open = false }
        }
    }

    private struct TimerKey: Equatable {
        var open: Bool
        var touched: Int
    }

    private var speakerSymbol: String {
        let volume = store.snapshot.volume
        if volume <= 0.001 { return "speaker.slash.fill" }
        if volume < 0.34 { return "speaker.wave.1.fill" }
        if volume < 0.67 { return "speaker.wave.2.fill" }
        return "speaker.wave.3.fill"
    }
}

private struct VolumeSlider: View {
    @Environment(PlayerStore.self) private var store
    var width: CGFloat
    var changed: () -> Void = {}

    var body: some View {
        Slider(
            value: Binding(
                get: { store.snapshot.volume },
                set: {
                    store.setVolume($0)
                    changed()
                }),
            in: 0...1
        )
        .frame(width: width)
        .accessibilityLabel(L10n.t(.volume))
    }
}
