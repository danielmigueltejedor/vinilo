// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Finds the daemon, starting it if nobody has. Vinilo is an app you open, not a service you
/// enable: the first client to arrive starts the engine and the rest find it running.
public struct DaemonLauncher: Sendable {
    public let executable: URL
    public let edition: Edition
    /// How long to wait for the daemon to bind its socket. It does that before anything else, so
    /// this is the process coming up, not the sidecar's Chromium behind it.
    public let startupTimeout: TimeInterval

    public init(executable: URL, edition: Edition, startupTimeout: TimeInterval = 10) {
        self.executable = executable
        self.edition = edition
        self.startupTimeout = startupTimeout
    }

    /// The daemon beside the app's own executable, which is where the bundle script puts it.
    public static func bundled(edition: Edition, bundle: Bundle = .main) -> DaemonLauncher? {
        bundle.url(forAuxiliaryExecutable: "vinilod").map {
            DaemonLauncher(executable: $0, edition: edition)
        }
    }

    public func connectOrSpawn(
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) async throws -> DaemonConnection {
        guard let path = DaemonPaths.socket(edition: edition, environment: environment) else {
            throw DaemonError.noRuntimeDirectory
        }
        if let connection = try? DaemonConnection.connect(path: path) {
            return connection
        }
        guard FileManager.default.isExecutableFile(atPath: executable.path) else {
            throw DaemonError.executableMissing(executable.path)
        }

        let process = Process()
        process.executableURL = executable
        // The daemon and the app must agree on the socket's directory, so it is handed over
        // rather than hoped for, and on the edition, which names every file the daemon keeps.
        var daemonEnvironment = environment
        if let value = edition.environmentValue {
            daemonEnvironment["VINILO_EDITION"] = value
        } else {
            daemonEnvironment.removeValue(forKey: "VINILO_EDITION")
        }
        if daemonEnvironment["XDG_RUNTIME_DIR"].map(\.isEmpty) ?? true {
            daemonEnvironment["TMPDIR"] = environment["TMPDIR"] ?? NSTemporaryDirectory()
        }
        // Finder starts apps with a bare PATH; YouTube Music and Tidal play through Homebrew's
        // yt-dlp and ffmpeg.
        let searchPath = daemonEnvironment["PATH"].flatMap { $0.isEmpty ? nil : $0 } ?? "/usr/bin:/bin"
        daemonEnvironment["PATH"] = searchPath + ":/opt/homebrew/bin:/usr/local/bin"
        process.environment = daemonEnvironment
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            throw DaemonError.socket(error.localizedDescription)
        }

        let deadline = Date().addingTimeInterval(startupTimeout)
        while Date() < deadline {
            try await Task.sleep(for: .milliseconds(50))
            if let connection = try? DaemonConnection.connect(path: path) {
                return connection
            }
            if !process.isRunning {
                // Another client's daemon may have won the race for the socket; one more look.
                if let connection = try? DaemonConnection.connect(path: path) {
                    return connection
                }
                throw DaemonError.didNotStart
            }
        }
        throw DaemonError.didNotStart
    }
}
