// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Which build of Vinilo a process belongs to. The two macOS editions share the daemon's code and
/// are installed side by side, so every file one writes or locks is named after it.
public enum Edition: String, Sendable {
    /// The GTK interface. Keeps the names it has always had.
    case nodalia
    /// The SwiftUI interface.
    case native

    /// What `VINILO_EDITION` is set to for the daemon. The default edition sets nothing.
    public var environmentValue: String? {
        self == .nodalia ? nil : rawValue
    }

    /// `vinilo` or `vinilo-native`: the folder and runtime-file stem, as `edition.rs` spells it.
    public var stem: String {
        self == .nodalia ? "vinilo" : "vinilo-\(rawValue)"
    }
}

public enum DaemonPaths {
    /// Where the daemon listens, which must be exactly where `ipc.rs` binds it: the per-user
    /// runtime directory, or `TMPDIR` on macOS, which has no `XDG_RUNTIME_DIR`.
    public static func socket(
        edition: Edition, environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> String? {
        func value(_ name: String) -> String? {
            environment[name].flatMap { $0.isEmpty ? nil : $0 }
        }
        guard let directory = value("XDG_RUNTIME_DIR") ?? value("TMPDIR") else { return nil }
        return URL(fileURLWithPath: directory).appendingPathComponent("\(edition.stem).sock").path
    }
}
