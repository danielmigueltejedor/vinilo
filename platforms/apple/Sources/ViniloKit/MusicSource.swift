// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Where the music comes from. The daemon plays one source at a time and reads which from a file
/// in its configuration folder when it starts, so these are the names `provider.rs` writes.
public enum MusicSource: String, CaseIterable, Identifiable, Sendable {
    case appleMusic = "apple-music"
    case local
    case spotify
    case youtubeMusic = "youtube-music"
    case tidal

    public var id: String { rawValue }

    /// Search-and-play catalogues that sign in through a web page of their own.
    public var isCatalog: Bool {
        switch self {
        case .spotify, .youtubeMusic, .tidal: true
        case .appleMusic, .local: false
        }
    }

    /// Names, the same in every language. `local` is the one that is a sentence, so the app
    /// translates it.
    public var brandName: String? {
        switch self {
        case .appleMusic: "Apple Music"
        case .spotify: "Spotify"
        case .youtubeMusic: "YouTube Music"
        case .tidal: "Tidal"
        case .local: nil
        }
    }

    public var symbol: String {
        switch self {
        case .appleMusic: "apple.logo"
        case .local: "folder"
        case .spotify: "dot.radiowaves.left.and.right"
        case .youtubeMusic: "play.rectangle"
        case .tidal: "water.waves"
        }
    }

    public enum Maturity: Sendable { case ready, beta, alpha }

    /// As `provider.rs` badges them.
    public var maturity: Maturity {
        switch self {
        case .appleMusic, .local, .spotify: .ready
        case .youtubeMusic: .beta
        case .tidal: .alpha
        }
    }

    /// Whether the source plays through `yt-dlp`, which a Mac has only from Homebrew.
    public var needsYtDlp: Bool {
        self == .youtubeMusic || self == .tidal
    }

    /// The page the sign-in window opens, as `setup.rs` has it.
    public var loginURL: URL? {
        switch self {
        case .spotify:
            URL(string: "https://accounts.spotify.com/en/login?continue=https%3A%2F%2Fopen.spotify.com%2F")
        case .youtubeMusic:
            URL(string: "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F")
        case .tidal:
            URL(string: "https://listen.tidal.com/login")
        case .appleMusic, .local:
            nil
        }
    }

    /// The sites whose cookies are the session. A cookie for any of these (or a subdomain of
    /// one) is kept; everything else the login touched is not.
    public var cookieDomains: [String] {
        switch self {
        case .spotify: ["spotify.com"]
        case .youtubeMusic: ["youtube.com", "google.com"]
        case .tidal: ["tidal.com"]
        case .appleMusic, .local: []
        }
    }

    /// Where the sign-in window may go: the service and the identity providers its page offers
    /// (Google, Apple, Facebook). Anything else, and any scheme but https, is refused.
    public var loginDomains: [String] {
        let shared = ["google.com", "gstatic.com", "googleusercontent.com", "apple.com", "icloud.com",
            "facebook.com", "fbcdn.net"]
        switch self {
        case .spotify: return ["spotify.com", "spotifycdn.com", "scdn.co"] + shared
        case .youtubeMusic: return ["youtube.com", "ytimg.com"] + shared
        case .tidal: return ["tidal.com", "tidalhifi.com"] + shared
        case .appleMusic, .local: return []
        }
    }

    public func allowsLoginNavigation(to url: URL) -> Bool {
        if url.scheme == "about" { return true }
        guard url.scheme == "https", let host = url.host?.lowercased() else { return false }
        return loginDomains.contains { host == $0 || host.hasSuffix("." + $0) }
    }

    public func ownsCookie(domain: String) -> Bool {
        let host = domain.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "."))
        return cookieDomains.contains { host == $0 || host.hasSuffix("." + $0) }
    }

    /// True once the login window has landed on the service's own site (`uri_looks_signed_in`).
    public func looksSignedIn(url: URL) -> Bool {
        let uri = url.absoluteString.lowercased()
        switch self {
        case .spotify:
            return uri.contains("open.spotify.com") && !uri.contains("accounts.spotify.com")
                && !uri.contains("/login")
        case .youtubeMusic:
            return uri.contains("music.youtube.com") && !uri.contains("accounts.google")
                && !uri.contains("servicelogin") && !uri.contains("/signin")
        case .tidal:
            return (uri.contains("listen.tidal.com") || uri.contains("tidal.com/browse"))
                && !uri.contains("/login") && !uri.contains("login.tidal")
                && !uri.contains("accounts.tidal")
        case .appleMusic, .local:
            return false
        }
    }

    /// Cookie names that mean a real session rather than an anonymous visit (`looks_signed_in`).
    public func looksSignedIn(cookieNames names: [String]) -> Bool {
        func has(_ wanted: [String]) -> Bool {
            names.contains { name in wanted.contains { $0.caseInsensitiveCompare(name) == .orderedSame } }
        }
        switch self {
        case .spotify:
            return has(["sp_dc"])
        case .youtubeMusic:
            return has(["SID", "SAPISID", "__Secure-1PSID", "__Secure-3PSID", "LOGIN_INFO"])
        case .tidal:
            if has([
                "sid", "token", "_token", "refresh_token", "tidal_sid", "access_token", "id_token",
                "authorization", "refresh", "sessionid", "userid", "user_id", "playback",
            ]) {
                return true
            }
            return names.contains { raw in
                let name = raw.lowercased()
                if name.contains("csrf") || name.contains("xsrf") || name.hasPrefix("_ga") {
                    return false
                }
                return (name.contains("tidal")
                    && (name.contains("sid") || name.contains("token") || name.contains("auth")))
                    || name.hasSuffix("_token") || name.hasSuffix("-token")
            }
        case .appleMusic, .local:
            return false
        }
    }
}

/// One cookie as `yt-dlp --cookies` reads it.
public struct NetscapeCookie: Equatable, Sendable {
    public var domain: String
    public var hostOnly: Bool
    public var path: String
    public var secure: Bool
    public var httpOnly: Bool
    /// Seconds since 1970; zero for a session cookie.
    public var expires: Int
    public var name: String
    public var value: String

    public init(
        domain: String, hostOnly: Bool, path: String, secure: Bool, httpOnly: Bool, expires: Int,
        name: String, value: String
    ) {
        self.domain = domain
        self.hostOnly = hostOnly
        self.path = path
        self.secure = secure
        self.httpOnly = httpOnly
        self.expires = expires
        self.name = name
        self.value = value
    }

    /// The file, exactly as `setup::write_netscape` writes it.
    public static func file(_ cookies: [NetscapeCookie]) -> String {
        var out = "# Netscape HTTP Cookie File\n# This file was generated by Vinilo.\n"
        for cookie in cookies where !cookie.name.isEmpty && !cookie.domain.isEmpty {
            var domain = cookie.domain
            if !cookie.hostOnly && !domain.hasPrefix(".") { domain = "." + domain }
            let path = cookie.path.isEmpty ? "/" : cookie.path
            if cookie.httpOnly { out += "#HttpOnly_" }
            out += [
                domain, cookie.hostOnly ? "FALSE" : "TRUE", path, cookie.secure ? "TRUE" : "FALSE",
                String(cookie.expires), cookie.name, cookie.value,
            ].joined(separator: "\t")
            out += "\n"
        }
        return out
    }
}

/// The daemon's configuration folder for an edition, and the few files in it that choose and
/// set up a source. Every name matches `paths.rs`, `provider.rs` and `setup.rs`.
public struct SourceConfig: Sendable {
    public let directory: URL

    public init(directory: URL) {
        self.directory = directory
    }

    /// `$XDG_CONFIG_HOME/<stem>`, else `~/.config/<stem>`, as `paths::config_dir` resolves it.
    public init?(
        edition: Edition, environment: [String: String] = ProcessInfo.processInfo.environment
    ) {
        let base: URL
        if let xdg = environment["XDG_CONFIG_HOME"], !xdg.isEmpty {
            base = URL(fileURLWithPath: xdg)
        } else if let home = environment["HOME"], !home.isEmpty {
            base = URL(fileURLWithPath: home).appendingPathComponent(".config")
        } else {
            return nil
        }
        self.init(directory: base.appendingPathComponent(edition.stem))
    }

    private var providerFile: URL { directory.appendingPathComponent("provider") }
    private var configuredFile: URL { directory.appendingPathComponent("configured") }

    public func cookiesFile(for source: MusicSource) -> URL? {
        source.isCatalog ? directory.appendingPathComponent("cookies-\(source.rawValue).txt") : nil
    }

    /// The chosen source; Apple Music until one is chosen, as the daemon defaults.
    public func current() -> MusicSource {
        guard let text = try? String(contentsOf: providerFile, encoding: .utf8) else {
            return .appleMusic
        }
        return MusicSource(rawValue: text.trimmingCharacters(in: .whitespacesAndNewlines))
            ?? .appleMusic
    }

    public func choose(_ source: MusicSource) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try "\(source.rawValue)\n".write(to: providerFile, atomically: true, encoding: .utf8)
    }

    /// Apple Music and files need nothing; a catalogue needs its sign-in done once.
    public func isConfigured(_ source: MusicSource) -> Bool {
        guard source.isCatalog else { return true }
        guard let text = try? String(contentsOf: configuredFile, encoding: .utf8) else {
            return false
        }
        return text.split(whereSeparator: \.isNewline).contains {
            $0.trimmingCharacters(in: .whitespaces) == source.rawValue
        }
    }

    /// Saves the session the sign-in window ended with and marks the source as set up.
    public func saveSession(_ cookies: [NetscapeCookie], for source: MusicSource) throws {
        guard let file = cookiesFile(for: source) else { return }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try NetscapeCookie.file(cookies).write(to: file, atomically: true, encoding: .utf8)
        // The session is a password in all but name.
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        var ids = Set(
            ((try? String(contentsOf: configuredFile, encoding: .utf8)) ?? "")
                .split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { MusicSource(rawValue: $0)?.isCatalog == true })
        ids.insert(source.rawValue)
        try (ids.sorted().joined(separator: "\n") + "\n")
            .write(to: configuredFile, atomically: true, encoding: .utf8)
    }
}
