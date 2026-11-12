// swift-tools-version: 6.0
// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import PackageDescription

// Vinilo Native: the SwiftUI client of the Vinilo daemon.
//
//   ViniloKit     what any client needs and nothing that draws: the wire format, the socket,
//                 starting the daemon. Foundation only.
//   ViniloShared  the state and the views, written for macOS and iPadOS alike. No AppKit.
//   ViniloSystem  macOS's Now Playing, media keys and notifications, shared by both editions.
//   ViniloMac     the macOS app: windows and menus.
//   ViniloNowPlaying  Vinilo Nodalia's bridge to Now Playing: no window, a mirror of its player.
let package = Package(
    name: "Vinilo",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "ViniloKit", targets: ["ViniloKit"]),
        .library(name: "ViniloShared", targets: ["ViniloShared"]),
        .executable(name: "ViniloMac", targets: ["ViniloMac"]),
        .executable(name: "ViniloNowPlaying", targets: ["ViniloNowPlaying"]),
    ],
    targets: [
        .target(name: "ViniloKit"),
        .target(name: "ViniloShared", dependencies: ["ViniloKit"]),
        .target(name: "ViniloSystem", dependencies: ["ViniloShared", "ViniloKit"]),
        .executableTarget(name: "ViniloMac", dependencies: ["ViniloShared", "ViniloKit", "ViniloSystem"]),
        .executableTarget(
            name: "ViniloNowPlaying", dependencies: ["ViniloShared", "ViniloKit", "ViniloSystem"]),
        .testTarget(
            name: "ViniloKitTests",
            dependencies: ["ViniloKit"],
            resources: [.copy("Fixtures")]
        ),
        .testTarget(name: "ViniloSharedTests", dependencies: ["ViniloShared", "ViniloKit"]),
    ]
)
