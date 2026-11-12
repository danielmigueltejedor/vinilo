// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import MediaPlayer
import UserNotifications
import ViniloKit
import ViniloShared

/// What macOS already provides, and Vinilo only has to tell it about: the media keys, the
/// Now Playing widget in Control Center and on the lock screen, and notifications.
///
/// None of it asks for a permission by itself. Notifications ask the first time a person turns
/// them on.
@MainActor
public final class SystemIntegration {
    public init() {}

    private var started = false
    private var remote: RemoteCommandBridge?
    private var artworkPath: String?
    private var artwork: MPMediaItemArtwork?

    public func start(store: PlayerStore) {
        guard !started else { return }
        started = true
        remote = RemoteCommandBridge(store: store)
        store.onSnapshot = { [weak self] snapshot, trackChanged in
            self?.update(snapshot, trackChanged: trackChanged)
        }
    }

    private func update(_ snapshot: Snapshot, trackChanged: Bool) {
        let center = MPNowPlayingInfoCenter.default()
        guard snapshot.hasTrack else {
            center.nowPlayingInfo = nil
            center.playbackState = .stopped
            return
        }
        if artworkPath != snapshot.artPath {
            artworkPath = snapshot.artPath
            artwork = snapshot.artPath.map(Self.makeArtwork)
        }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: snapshot.title,
            MPMediaItemPropertyArtist: snapshot.artist,
            MPMediaItemPropertyAlbumTitle: snapshot.album,
            MPMediaItemPropertyPlaybackDuration: Double(snapshot.durationMs) / 1000,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: Double(snapshot.positionMs) / 1000,
            MPNowPlayingInfoPropertyPlaybackRate: snapshot.playing ? 1.0 : 0.0,
        ]
        if let artwork { info[MPMediaItemPropertyArtwork] = artwork }
        center.nowPlayingInfo = info
        center.playbackState = snapshot.playing ? .playing : .paused

        let commands = MPRemoteCommandCenter.shared()
        commands.nextTrackCommand.isEnabled = snapshot.canNext
        commands.previousTrackCommand.isEnabled = snapshot.canPrevious

        if trackChanged { TrackNotifier.announce(snapshot) }
    }

    /// Built outside any actor on purpose: the system calls the handler on its own queue, and a
    /// closure that inherited the main actor's isolation would trap when it did.
    private nonisolated static func makeArtwork(path: String) -> MPMediaItemArtwork {
        MPMediaItemArtwork(boundsSize: CGSize(width: 600, height: 600)) { _ in
            NSImage(contentsOfFile: path) ?? NSImage()
        }
    }
}

/// The media keys and the system's own transport. Not main-actor isolated, because the system
/// calls these handlers from its own queue; each hops to the main actor to act.
private final class RemoteCommandBridge: Sendable {
    init(store: PlayerStore) {
        let center = MPRemoteCommandCenter.shared()
        func command(_ command: MPRemoteCommand, _ action: @escaping @Sendable @MainActor (PlayerStore) -> Void) {
            command.isEnabled = true
            command.addTarget { _ in
                Task { @MainActor in action(store) }
                return .success
            }
        }
        command(center.playCommand) { $0.play() }
        command(center.pauseCommand) { $0.pause() }
        command(center.togglePlayPauseCommand) { $0.playPause() }
        command(center.nextTrackCommand) { $0.next() }
        command(center.previousTrackCommand) { $0.previous() }
        center.changePlaybackPositionCommand.isEnabled = true
        center.changePlaybackPositionCommand.addTarget { event in
            guard let event = event as? MPChangePlaybackPositionCommandEvent else {
                return .commandFailed
            }
            let seconds = event.positionTime
            Task { @MainActor in store.seek(toMs: Int(seconds * 1000)) }
            return .success
        }
    }
}

/// A notification when the song changes, only if asked for and only while Vinilo is behind
/// another app: in front, the player already says so.
public enum TrackNotifier {
    public static let defaultsKey = "notifyOnTrackChange"

    /// Notifications need a real app bundle; a build run from the tree has none.
    private static var available: Bool {
        Bundle.main.bundleIdentifier != nil && Bundle.main.bundlePath.hasSuffix(".app")
    }

    @MainActor
    static func announce(_ snapshot: Snapshot) {
        guard available, UserDefaults.standard.bool(forKey: defaultsKey), !NSApp.isActive,
            snapshot.hasTrack, !snapshot.title.isEmpty
        else { return }
        let content = UNMutableNotificationContent()
        content.title = snapshot.title
        content.body = [snapshot.artist, snapshot.album].filter { !$0.isEmpty }.joined(separator: " — ")
        content.threadIdentifier = "now-playing"
        let request = UNNotificationRequest(identifier: "now-playing", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    /// Asked when a person turns the setting on, never before.
    public static func requestAuthorization() async -> Bool {
        guard available else { return false }
        return (try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert])) ?? false
    }
}
