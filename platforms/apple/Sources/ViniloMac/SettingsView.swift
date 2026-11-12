// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit
import ViniloShared

struct SettingsView: View {
    @Environment(PlayerStore.self) private var store
    @AppStorage(TrackNotifier.defaultsKey) private var notify = false
    @State private var confirmSignOut = false
    /// A catalogue waiting for its sign-in before it becomes the source.
    @State private var signingIn: MusicSource?
    @State private var failure: String?
    private let config = SourceConfig(edition: .native)

    var body: some View {
        Form {
            Section {
                Picker(L10n.t(.source), selection: choice) {
                    ForEach(MusicSource.allCases) { source in
                        HStack(spacing: 6) {
                            Label(source.displayName, systemImage: source.symbol)
                            if let badge = source.badge {
                                Text(badge).font(.caption2.weight(.semibold))
                                    .padding(.horizontal, 6).padding(.vertical, 1)
                                    .background(.tint.opacity(0.18), in: Capsule())
                            }
                        }
                        .tag(source)
                    }
                }
                .pickerStyle(.radioGroup)
                if store.source.isCatalog {
                    Button(L10n.t(.signInAgain)) { signingIn = store.source }
                }
                if store.source.needsYtDlp {
                    Text(L10n.t(.needsYtDlp)).font(.caption).foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                if let failure {
                    Text(failure).font(.caption).foregroundStyle(.red)
                }
            } header: {
                Text(L10n.t(.source))
            } footer: {
                Text(L10n.t(.sourceBody)).font(.caption).foregroundStyle(.secondary)
            }
            Section {
                Toggle(L10n.t(.notifications), isOn: $notify)
                Text(L10n.t(.notificationsBody)).font(.caption).foregroundStyle(.secondary)
            }
            Section(L10n.t(.settingsLibrary)) {
                Button(L10n.t(.reloadLibrary)) { store.refreshLibrary() }
                if store.source == .appleMusic {
                    Button(L10n.t(.signOut), role: .destructive) { confirmSignOut = true }
                }
            }
            Section(L10n.t(.edition)) {
                Text(L10n.t(.editionBody)).font(.callout).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 500)
        .fixedSize(horizontal: false, vertical: true)
        .onChange(of: notify) { _, isOn in
            // Asked now, when a person has just said they want it, and not at launch.
            if isOn {
                Task {
                    if await TrackNotifier.requestAuthorization() == false { notify = false }
                }
            }
        }
        .confirmationDialog(L10n.t(.signOut), isPresented: $confirmSignOut) {
            Button(L10n.t(.signOut), role: .destructive) { store.signOut() }
        }
        .sheet(item: $signingIn) { source in
            if let config {
                SourceLoginView(source: source, config: config) { signedIn in
                    signingIn = nil
                    if signedIn { use(source) }
                }
            }
        }
    }

    /// Choosing a catalogue that has never been signed in to asks for the sign-in first.
    private var choice: Binding<MusicSource> {
        Binding(
            get: { store.source },
            set: { next in
                guard next != store.source else { return }
                if next.isCatalog && !store.isConfigured(next) {
                    signingIn = next
                } else {
                    use(next)
                }
            })
    }

    private func use(_ source: MusicSource) {
        do {
            try store.choose(source)
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
    }
}
