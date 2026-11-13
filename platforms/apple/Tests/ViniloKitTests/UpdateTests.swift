// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Testing

@testable import ViniloKit

private let sum = String(repeating: "ab", count: 32)

private func asset(_ name: String, digest: String? = "sha256:" + sum) -> [String: Any] {
    var asset: [String: Any] = [
        "name": name,
        "browser_download_url": "https://github.com/getnodalia/vinilo/releases/download/x/\(name)",
    ]
    if let digest { asset["digest"] = digest }
    return asset
}

private func release(_ tag: String, _ assets: [[String: Any]], draft: Bool = false, prerelease: Bool = false)
    -> [String: Any]
{
    [
        "tag_name": tag, "draft": draft, "prerelease": prerelease, "body": "notes for \(tag)",
        "html_url": "https://github.com/getnodalia/vinilo/releases/tag/\(tag)", "assets": assets,
    ]
}

private func feed(_ releases: [[String: Any]]) throws -> Data {
    try JSONSerialization.data(withJSONObject: releases)
}

private let suffix = "-native-macos-arm64.dmg"

@Suite("In-app updates")
struct UpdateTests {
    @Test("versions order by number, not by text")
    func versions() {
        #expect(UpdateVersion("0.10.0") > UpdateVersion("0.9.9"))
        #expect(UpdateVersion("v0.2.0") == UpdateVersion("0.2"))
        #expect(UpdateVersion("1.0.0-rc.1") < UpdateVersion("1.0.0"))
        #expect(UpdateVersion("1.0.0+build5") == UpdateVersion("1.0.0"))
        #expect(UpdateVersion("junk") == UpdateVersion("0"))
    }

    @Test("this edition takes its own disk image, never Vinilo Nodalia's")
    func ownEdition() throws {
        let data = try feed([
            release(
                "v0.3.0",
                [
                    asset("Vinilo-0.3.0-nodalia-macos-arm64.dmg"), asset("Vinilo-0.3.0-native-macos-arm64.dmg.sha256"),
                    asset("Vinilo-0.3.0-native-macos-arm64.dmg"), asset("Vinilo-0.3.0-x86_64.AppImage"),
                ])
        ])
        let found = try #require(UpdateFeed.newest(in: data, after: "0.2.0", packageSuffix: suffix))
        #expect(found.packageName == "Vinilo-0.3.0-native-macos-arm64.dmg")
        #expect(found.version == "0.3.0")
        #expect(found.sha256 == sum)
        #expect(found.notes == "notes for v0.3.0")
    }

    @Test("the newest release that has this edition's image wins; drafts and pre-releases never do")
    func newestWithAPackage() throws {
        let data = try feed([
            release("v0.6.0", [asset("Vinilo-0.6.0-native-macos-arm64.dmg")], draft: true),
            release("v0.5.0", [asset("Vinilo-0.5.0-native-macos-arm64.dmg")], prerelease: true),
            release("v0.4.1", [asset("Vinilo-0.4.1-nodalia-macos-arm64.dmg")]),
            release("v0.3.0", [asset("Vinilo-0.3.0-native-macos-arm64.dmg")]),
            release("v0.4.0", [asset("Vinilo-0.4.0-native-macos-arm64.dmg")]),
        ])
        #expect(UpdateFeed.newest(in: data, after: "0.2.0", packageSuffix: suffix)?.version == "0.4.0")
        #expect(UpdateFeed.newest(in: data, after: "0.4.0", packageSuffix: suffix) == nil)
        #expect(UpdateFeed.newest(in: data, after: "0.9.0", packageSuffix: suffix) == nil)
    }

    @Test("hostile or empty lists offer nothing")
    func hostile() throws {
        #expect(UpdateFeed.newest(in: Data("not json".utf8), after: "0.1.0", packageSuffix: suffix) == nil)
        #expect(UpdateFeed.newest(in: Data("[]".utf8), after: "0.1.0", packageSuffix: suffix) == nil)
        #expect(UpdateFeed.newest(in: Data("{\"message\":\"rate limit\"}".utf8), after: "0.1.0", packageSuffix: suffix) == nil)
        var elsewhere = asset("Vinilo-9.0.0-native-macos-arm64.dmg")
        elsewhere["browser_download_url"] = "https://example.com/Vinilo-9.0.0-native-macos-arm64.dmg"
        let data = try feed([release("v9.0.0", [elsewhere])])
        #expect(UpdateFeed.newest(in: data, after: "0.1.0", packageSuffix: suffix) == nil)
    }

    @Test("a digest is 64 hex digits of SHA-256 or it is nothing")
    func digests() throws {
        #expect(UpdateFeed.digest("sha256:" + sum.uppercased()) == sum)
        #expect(UpdateFeed.digest("sha512:" + sum) == nil)
        #expect(UpdateFeed.digest("sha256:abc") == nil)
        #expect(UpdateFeed.digest(nil) == nil)
        let data = try feed([release("v0.3.0", [asset("Vinilo-0.3.0-native-macos-arm64.dmg", digest: nil)])])
        let found = try #require(UpdateFeed.newest(in: data, after: "0.2.0", packageSuffix: suffix))
        #expect(found.sha256 == nil)
    }

    @Test("the helper's result is read back")
    func outcomes() {
        #expect(UpdateOutcome(result: "ok\n") == .installed)
        #expect(UpdateOutcome(result: "signature\n") == .signature)
        #expect(UpdateOutcome(result: "still-running") == .stillRunning)
        #expect(UpdateOutcome(result: "anything else") == .other)
    }

    @Test("the helper checks the copy it installs, without codesign's strict mode")
    func helper() throws {
        let script = UpdateHelper.script
        #expect(script.hasPrefix("#!/bin/bash\n"))
        #expect(script.contains("/*/\"Vinilo Native.app\") ;;"))
        #expect(script.contains("SOURCE=\"$MOUNT/Vinilo Native.app\""))
        // Strict mode refuses the Finder metadata every file from a disk image carries.
        #expect(script.contains("/usr/bin/codesign --verify --deep \"$NEW\""))
        #expect(!script.contains("--strict"))
        // A shipped helper never honours the switches tests use to skip its checks.
        #expect(script.contains("unset NODALIA_UPDATE_OPEN NODALIA_UPDATE_WAIT_STEPS"))
        let hash = try #require(script.range(of: "shasum -a 256"))
        let mount = try #require(script.range(of: "hdiutil attach"))
        let swap = try #require(script.range(of: "/bin/mv \"$BUNDLE\" \"$OLD\""))
        #expect(hash.lowerBound < mount.lowerBound && mount.lowerBound < swap.lowerBound)
        #expect(script.contains("printf 'ok\\n' >\"$RESULT\""))
    }

    @Test("the disk image name is the one the release workflow gives it")
    func releaseWorkflowName() throws {
        // platforms/apple/Tests/ViniloKitTests/UpdateTests.swift -> the repository root.
        let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let workflow = try String(contentsOf: root.appendingPathComponent(".github/workflows/release.yml"), encoding: .utf8)
        #expect(
            workflow.contains(
                "\"dist/macos/Vinilo Native.app\" \"dist/Vinilo-${VERSION}-native-macos-arm64.dmg\""))
        #expect("Vinilo-1.0.0-native-macos-arm64.dmg".hasSuffix(suffix))
        #expect(UpdateHelper.bundleName == "Vinilo Native.app")
    }
}

@Suite("Update notes")
struct UpdateNotesTests {
    static let notes = """
        Una línea de resumen.
        *A summary line.*

        ## Español

        ### Correcciones
        - Al pulsar **Instalar y reiniciar**, nada.

        ## English

        ### Fixes
        - After **Install and Restart**, nothing.

        ## Descargas · Downloads

        | Sistema · System | Archivo · File |
        |---|---|
        | Linux | `Vinilo-0.2.3-x86_64.AppImage` |
        """

    @Test("only the reader's language, without the downloads table")
    func language() {
        #expect(UpdateNotes.inline(Self.notes, spanish: true) == "**Correcciones**\n•  Al pulsar **Instalar y reiniciar**, nada.")
        #expect(UpdateNotes.inline(Self.notes, spanish: false) == "**Fixes**\n•  After **Install and Restart**, nothing.")
    }

    @Test("notes without language sections come back whole")
    func whole() {
        #expect(UpdateNotes.inline("Just text", spanish: true) == "Just text")
    }
}
