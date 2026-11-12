# Vinilo on macOS: two editions

Vinilo ships two editions for macOS. They share one engine, `vinilod`, and differ only in the
interface that talks to it.

| | Vinilo Native | Vinilo Nodalia |
| --- | --- | --- |
| Interface | SwiftUI (`platforms/apple`) | GTK 4 in the Nodalia Design Language (`crates/vinilo`) |
| Window controls | the system's | the system's (the Nodalia ones are Linux-only) |
| Bundle | `Vinilo Native.app`, `dev.danielmiguelt.Vinilo.Native` | `Vinilo Nodalia.app`, `dev.danielmiguelt.Vinilo` |
| Settings and caches | `vinilo-native` | the names every earlier install already has |
| DMG | `Vinilo-<version>-native-macos-arm64.dmg` | `Vinilo-<version>-nodalia-macos-arm64.dmg` |
| Built by | `platforms/apple/build.sh` | `packaging/macos/bundle.sh` |

The edition is a launch-time setting, `VINILO_EDITION`. It names the daemon's socket, its
settings, its caches and the sidecar's browser profile, so two editions running at once never
open each other's files. Leaving it unset gives the Nodalia names, which is what keeps existing
installs where they were.

## How Vinilo Native is built

Three layers, each depending only on the one above it in this list:

- **`ViniloKit`**: what any client needs and nothing that draws. The wire format, the socket
  and starting the daemon. Foundation only.
- **`ViniloShared`**: the state (`PlayerStore`, a mirror of what the daemon last said, never a
  second source of truth) and the views. Written for macOS and iPadOS alike, no AppKit.
- **`ViniloMac`**: what only macOS has. The app, the menu bar, media keys, Now Playing, and
  notifications.

The queue stays with the player. The app sends a request and waits for the echo, so the screen
never shows a state the player is not in; that is also what keeps transitions gapless.

Liquid Glass is used on macOS 26 and later, behind `#available`, and compiled in only when the
SDK has it (`compiler(>=6.2)`). On macOS 14 and 15 the same views use the system's materials.

## What is verified, and what is not

Checked by CI on every change: the Swift tests (`swift test`), that the bundle builds, and that nothing the
binaries link to lies outside it.

Not verified, because it needs a Mac and a castLabs account:

- **Apple Music playing full songs.** Apple's licence server answers an unsigned Widevine player
  with 90-second previews. Release builds VMP-sign the Electron through castLabs EVS
  (`packaging/macos/vmp_sign.sh`, before the code signature). It needs the repository secrets
  `EVS_ACCOUNT_NAME` and `EVS_PASSWD`. Without them the build warns and stays unsigned, and Vinilo
  does not offer Apple Music on macOS rather than offer a player that cannot finish a song.
- **Frame rate and memory** of the interface. No figure is claimed until it has been measured
  on a Mac.

## Signing

Both DMGs are signed ad-hoc: enough to run, not enough to avoid Gatekeeper's first-open prompt
(right-click, **Open**). Developer ID signing and notarization will be added when the
certificates exist as repository secrets; until then nothing here claims them.

## Running the Swift tests

```bash
swift test --package-path platforms/apple
```

The tests cover the wire format against recorded daemon lines, the strings in both languages,
and the store's behaviour against a recording stand-in for the daemon.
