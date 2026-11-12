// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import ViniloKit

extension MusicSource {
    /// The service's own name, or what files are called in the person's language.
    public var displayName: String {
        brandName ?? L10n.t(.localFiles)
    }

    /// "Beta" or "Alfa" beside a source that is not finished; nothing beside one that is.
    public var badge: String? {
        switch maturity {
        case .ready: nil
        case .beta: L10n.t(.beta)
        case .alpha: L10n.t(.alpha)
        }
    }
}
