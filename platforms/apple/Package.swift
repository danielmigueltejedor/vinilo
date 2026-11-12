// swift-tools-version: 6.0
// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import PackageDescription

// Vinilo Native: the SwiftUI client of the Vinilo daemon.
//
//   ViniloKit     what any client needs and nothing that draws: the wire format, the socket,
//                 starting the daemon. Foundation only.
//   ViniloShared  the state and the views, written for macOS and iPadOS alike. No AppKit.
//   ViniloMac     what only macOS has: the app, the menu bar, media keys, notifications.
let package = Package(
    name: "Vinilo",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "ViniloKit", targets: ["ViniloKit"]),
        .library(name: "ViniloShared", targets: ["ViniloShared"]),
        .executable(name: "ViniloMac", targets: ["ViniloMac"]),
    ],
    targets: [
        .target(name: "ViniloKit"),
        .target(name: "ViniloShared", dependencies: ["ViniloKit"]),
        .executableTarget(name: "ViniloMac", dependencies: ["ViniloShared", "ViniloKit"]),
        .testTarget(
            name: "ViniloKitTests",
            dependencies: ["ViniloKit"],
            resources: [.copy("Fixtures")]
        ),
        .testTarget(name: "ViniloSharedTests", dependencies: ["ViniloShared", "ViniloKit"]),
    ]
)
