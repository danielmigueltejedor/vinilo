// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import CryptoKit
import SwiftUI
import ViniloKit

/// In-app updates for Vinilo Native: look at Vinilo's releases, download this edition's disk
/// image, check it against the SHA-256 GitHub publishes, and hand it to the helper that swaps the
/// app once it has quit.
@MainActor
@Observable
final class AppUpdater {
    enum Phase: Equatable {
        case idle
        case checking
        case upToDate
        case available(UpdateRelease)
        case downloading(UpdateRelease)
        case installing
        case failed(String)
        /// The last install did not go through; said once, at the next start.
        case lastFailed(UpdateOutcome)
    }

    private(set) var phase: Phase = .idle
    /// Whether the update sheet is up.
    var presented = false

    private static let lastCheckKey = "update.lastCheck"
    private static let every: TimeInterval = 24 * 60 * 60

    /// The version of the app that is running, or nil when it is not a packaged app.
    private let current: String? = {
        guard Bundle.main.bundleURL.lastPathComponent == UpdateHelper.bundleName else { return nil }
        return Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
    }()

    var currentVersion: String { current ?? "" }

    /// Whether this copy can be replaced where it is. A copy run straight from the disk image, or
    /// from a folder that cannot be written to, is only pointed at the download.
    var canInstallInPlace: Bool {
        guard current != nil else { return false }
        let bundle = Bundle.main.bundleURL
        if bundle.pathComponents.contains("AppTranslocation") { return false }
        return FileManager.default.isWritableFile(atPath: bundle.deletingLastPathComponent().path)
    }

    private var busy: Bool {
        switch phase {
        case .checking, .downloading, .installing: true
        default: false
        }
    }

    /// At launch: say why the last install failed, if it did, and look for a new version once a
    /// day. Silent unless there is something to say.
    func start() {
        guard current != nil else { return }
        if let outcome = Self.takeLastOutcome(), outcome != .installed {
            phase = .lastFailed(outcome)
            presented = true
            return
        }
        let last = UserDefaults.standard.double(forKey: Self.lastCheckKey)
        if Date().timeIntervalSince1970 - last >= Self.every { check(userAsked: false) }
    }

    func check(userAsked: Bool) {
        if userAsked { presented = true }
        guard !busy else { return }
        guard let current, let suffix = UpdateFeed.packageSuffix else {
            // Run from the source tree, or on a Mac no disk image is published for.
            if userAsked { phase = .failed(Words.notPackaged) }
            return
        }
        phase = .checking
        Task {
            do {
                let data = try await Self.fetchReleases()
                UserDefaults.standard.set(Date().timeIntervalSince1970, forKey: Self.lastCheckKey)
                if let release = UpdateFeed.newest(in: data, after: current, packageSuffix: suffix) {
                    phase = .available(release)
                    presented = true
                } else {
                    phase = .upToDate
                }
            } catch {
                phase = .failed(error.localizedDescription)
            }
        }
    }

    func install(_ release: UpdateRelease) {
        guard !busy else { return }
        guard canInstallInPlace, let expected = release.sha256 else {
            NSWorkspace.shared.open(release.page)
            presented = false
            return
        }
        phase = .downloading(release)
        let bundle = Bundle.main.bundleURL
        let pid = ProcessInfo.processInfo.processIdentifier
        Task {
            do {
                let folder = try Self.updateFolder()
                let image = try await Self.download(release, expected: expected, into: folder)
                try Self.startHelper(image: image, bundle: bundle, pid: pid, expected: expected, folder: folder)
                phase = .installing
                // The helper waits for this process to end before it touches the app.
                await quit()
            } catch {
                phase = .failed(error.localizedDescription)
            }
        }
    }

    /// AppKit ignores `terminate` while a sheet is attached, and the "Installing…" sheet is one,
    /// so it goes first. Should the app still be here after that, it leaves on its own: the
    /// helper gives up a minute after it started.
    private func quit() async {
        presented = false
        try? await Task.sleep(for: .milliseconds(400))
        NSApp.terminate(nil)
        try? await Task.sleep(for: .seconds(5))
        NotificationCenter.default.post(name: NSApplication.willTerminateNotification, object: NSApp)
        exit(0)
    }

    func dismiss() {
        presented = false
        if !busy { phase = .idle }
    }

    // MARK: - Work off the main actor

    private struct Failure: LocalizedError {
        let errorDescription: String?
    }

    private nonisolated static func fetchReleases() async throws -> Data {
        guard let url = UpdateFeed.url else { throw Failure(errorDescription: Words.checkFailed) }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 20)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue("Vinilo", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await URLSession.shared.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else {
            throw Failure(errorDescription: Words.checkFailed)
        }
        return data
    }

    /// `~/Library/Caches/Vinilo Native/updates`, private to the user.
    private nonisolated static func updateFolder() throws -> URL {
        let caches = try FileManager.default.url(
            for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let folder = caches.appendingPathComponent("Vinilo Native/updates", isDirectory: true)
        try FileManager.default.createDirectory(
            at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        return folder
    }

    private nonisolated static func download(_ release: UpdateRelease, expected: String, into folder: URL)
        async throws -> URL
    {
        let (temporary, response) = try await URLSession.shared.download(from: release.package)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else {
            throw Failure(errorDescription: Words.downloadFailed)
        }
        let image = folder.appendingPathComponent("update.dmg")
        try? FileManager.default.removeItem(at: image)
        try FileManager.default.moveItem(at: temporary, to: image)
        guard try sha256(of: image) == expected else {
            try? FileManager.default.removeItem(at: image)
            throw Failure(errorDescription: Words.badDigest)
        }
        return image
    }

    private nonisolated static func sha256(of file: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    private nonisolated static func startHelper(image: URL, bundle: URL, pid: Int32, expected: String, folder: URL)
        throws
    {
        let script = folder.appendingPathComponent("apply-update.sh")
        let result = folder.appendingPathComponent("result.txt")
        try? FileManager.default.removeItem(at: result)
        try Data(UpdateHelper.script.utf8).write(to: script, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: script.path)

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/bash")
        // Started in the background of a shell that exits at once, so the helper is nobody's
        // child by the time the app quits.
        process.arguments = [
            "-c", "/usr/bin/nohup /bin/bash \"$@\" >/dev/null 2>&1 &", "vinilo-update",
            script.path, image.path, folder.appendingPathComponent("mount").path, bundle.path, String(pid),
            folder.appendingPathComponent("update.log").path, result.path, expected,
        ]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { throw Failure(errorDescription: Words.helperFailed) }
    }

    /// What the helper wrote last time, read once.
    private nonisolated static func takeLastOutcome() -> UpdateOutcome? {
        guard let result = try? updateFolder().appendingPathComponent("result.txt"),
            let text = try? String(contentsOf: result, encoding: .utf8)
        else { return nil }
        try? FileManager.default.removeItem(at: result)
        return UpdateOutcome(result: text)
    }
}

/// "Check for Updates…" under the app's own menu, where a Mac app keeps it.
struct UpdateCommands: Commands {
    var updater: AppUpdater

    var body: some Commands {
        CommandGroup(after: .appInfo) {
            Button(Words.checkMenu) { updater.check(userAsked: true) }
        }
    }
}

struct UpdateSheet: View {
    var updater: AppUpdater

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(title).font(.title3.weight(.semibold))
            detail
            HStack {
                Spacer()
                buttons
            }
        }
        .padding(22)
        .frame(width: 460)
    }

    private var title: String {
        switch updater.phase {
        case .idle, .checking: Words.checking
        case .upToDate: Words.upToDate
        case .available(let release), .downloading(let release): Words.available(release.version)
        case .installing: Words.installing
        case .failed: Words.failedTitle
        case .lastFailed: Words.lastFailedTitle
        }
    }

    @ViewBuilder private var detail: some View {
        switch updater.phase {
        case .idle, .checking:
            ProgressView().controlSize(.small)
        case .upToDate:
            Text(Words.youHave(updater.currentVersion)).foregroundStyle(.secondary)
        case .available(let release):
            Text(Words.youHave(updater.currentVersion)).foregroundStyle(.secondary)
            if !release.notes.isEmpty {
                ScrollView {
                    Text(release.notes).font(.callout).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 220)
            }
            if !updater.canInstallInPlace || release.sha256 == nil {
                Text(Words.manual).font(.caption).foregroundStyle(.secondary)
            }
        case .downloading:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(Words.downloading).foregroundStyle(.secondary)
            }
        case .installing:
            Text(Words.restarting).foregroundStyle(.secondary)
        case .failed(let reason):
            Text(reason).foregroundStyle(.secondary).textSelection(.enabled)
        case .lastFailed(let outcome):
            Text(Words.reason(outcome)).foregroundStyle(.secondary)
        }
    }

    @ViewBuilder private var buttons: some View {
        switch updater.phase {
        case .available(let release):
            Button(Words.later) { updater.dismiss() }.keyboardShortcut(.cancelAction)
            Button(updater.canInstallInPlace && release.sha256 != nil ? Words.install : Words.openDownload) {
                updater.install(release)
            }
            .keyboardShortcut(.defaultAction)
        case .downloading, .installing:
            EmptyView()
        case .lastFailed:
            Button(Words.close) { updater.dismiss() }.keyboardShortcut(.cancelAction)
            Button(Words.tryAgain) { updater.check(userAsked: true) }.keyboardShortcut(.defaultAction)
        default:
            Button(Words.close) { updater.dismiss() }.keyboardShortcut(.defaultAction)
        }
    }
}

/// The updater's words, in Vinilo's two languages.
enum Words {
    private static var spanish: Bool { Locale.current.language.languageCode?.identifier == "es" }
    private static func pick(_ es: String, _ en: String) -> String { spanish ? es : en }

    static var checkMenu: String { pick("Buscar actualizaciones…", "Check for Updates…") }
    static var checking: String { pick("Buscando actualizaciones…", "Checking for updates…") }
    static var upToDate: String { pick("Vinilo está al día", "Vinilo is up to date") }
    static func available(_ version: String) -> String {
        pick("Vinilo \(version) está disponible", "Vinilo \(version) is available")
    }
    static func youHave(_ version: String) -> String {
        pick("Tienes la versión \(version).", "You have version \(version).")
    }
    static var install: String { pick("Instalar y reiniciar", "Install and Restart") }
    static var openDownload: String { pick("Abrir la descarga", "Open the Download") }
    static var later: String { pick("Más tarde", "Later") }
    static var close: String { pick("Cerrar", "Close") }
    static var tryAgain: String { pick("Buscar de nuevo", "Check Again") }
    static var downloading: String { pick("Descargando la versión nueva…", "Downloading the new version…") }
    static var installing: String { pick("Instalando…", "Installing…") }
    static var restarting: String {
        pick("Vinilo se cierra y vuelve a abrirse con la versión nueva.", "Vinilo will quit and reopen with the new version.")
    }
    static var manual: String {
        pick(
            "Esta copia no puede sustituirse sola. Mueve Vinilo Native a Aplicaciones, o descarga la versión nueva.",
            "This copy cannot replace itself. Move Vinilo Native to Applications, or download the new version.")
    }
    static var failedTitle: String { pick("No se pudo actualizar", "The update did not work") }
    static var lastFailedTitle: String {
        pick("La última actualización no se instaló", "The last update was not installed")
    }
    static var checkFailed: String {
        pick("No se pudieron consultar las versiones de Vinilo.", "Vinilo's releases could not be read.")
    }
    static var downloadFailed: String { pick("No se pudo descargar la versión nueva.", "The new version could not be downloaded.") }
    static var badDigest: String {
        pick(
            "La descarga no coincide con la suma SHA-256 publicada, así que no se ha instalado.",
            "The download does not match the published SHA-256 sum, so it was not installed.")
    }
    static var helperFailed: String { pick("No se pudo iniciar la instalación.", "The install could not be started.") }
    static var notPackaged: String {
        pick("Esta copia de Vinilo no se actualiza sola.", "This copy of Vinilo does not update itself.")
    }
    static func reason(_ outcome: UpdateOutcome) -> String {
        switch outcome {
        case .installed: pick("Se instaló.", "It was installed.")
        case .stillRunning: pick("Vinilo seguía abierto.", "Vinilo was still running.")
        case .mount: pick("No se pudo abrir el disco de instalación.", "The disk image could not be opened.")
        case .contents: pick("El disco de instalación no era el esperado.", "The disk image was not the expected one.")
        case .copy: pick("No se pudo copiar la app nueva.", "The new app could not be copied.")
        case .signature: pick("La firma de la app nueva no era válida.", "The new app's signature was not valid.")
        case .replace: pick("No se pudo sustituir la app.", "The app could not be replaced.")
        case .launch: pick("La versión nueva no arrancó; sigue la anterior.", "The new version did not start; the previous one is back.")
        case .other: pick("Algo falló; no se cambió nada.", "Something failed; nothing was changed.")
        }
    }
}
