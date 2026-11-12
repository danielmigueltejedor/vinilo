// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloShared

struct SettingsView: View {
    @Environment(PlayerStore.self) private var store
    @AppStorage(TrackNotifier.defaultsKey) private var notify = false
    @State private var confirmSignOut = false

    var body: some View {
        Form {
            Section {
                Toggle(L10n.t(.notifications), isOn: $notify)
                Text(L10n.t(.notificationsBody)).font(.caption).foregroundStyle(.secondary)
            }
            Section(L10n.t(.settingsLibrary)) {
                Button(L10n.t(.reloadLibrary)) { store.refreshLibrary() }
                Button(L10n.t(.signOut), role: .destructive) { confirmSignOut = true }
            }
            Section(L10n.t(.edition)) {
                Text(L10n.t(.editionBody)).font(.callout).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 480)
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
    }
}
