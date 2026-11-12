// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import ViniloKit
import ViniloShared
import WebKit

/// The service's own sign-in page, in a sheet. Once it lands on the player the session is saved
/// where the engine reads it, in the same file the Linux edition writes, and the sheet closes.
struct SourceLoginView: View {
    let source: MusicSource
    let config: SourceConfig
    var finish: (_ signedIn: Bool) -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: source.symbol).font(.title2).foregroundStyle(.tint)
                VStack(alignment: .leading, spacing: 2) {
                    Text(String(format: L10n.t(.loginTitle), source.displayName)).font(.headline)
                    Text(L10n.t(.loginBody)).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button(L10n.t(.cancel)) { finish(false) }
                    .keyboardShortcut(.cancelAction)
            }
            .padding(14)
            Divider()
            LoginWebView(source: source, config: config) { finish(true) }
        }
        .frame(width: 540, height: 700)
    }
}

private struct LoginWebView: NSViewRepresentable {
    let source: MusicSource
    let config: SourceConfig
    let signedIn: @MainActor () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(source: source, config: config, signedIn: signedIn)
    }

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        // A profile per service, kept between sign-ins like a browser's.
        configuration.websiteDataStore = WKWebsiteDataStore(forIdentifier: source.profile)
        let view = WKWebView(frame: .zero, configuration: configuration)
        // Google refuses sign-in from a page that does not say it is a browser.
        view.customUserAgent =
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15"
        view.navigationDelegate = context.coordinator
        if let url = source.loginURL { view.load(URLRequest(url: url)) }
        context.coordinator.watch(view)
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {}

    static func dismantleNSView(_ view: WKWebView, coordinator: Coordinator) {
        coordinator.stop()
    }

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate {
        private let source: MusicSource
        private let config: SourceConfig
        private let signedIn: @MainActor () -> Void
        private var saved = false
        private var poll: Task<Void, Never>?

        init(source: MusicSource, config: SourceConfig, signedIn: @escaping @MainActor () -> Void) {
            self.source = source
            self.config = config
            self.signedIn = signedIn
        }

        /// Single-page sites move without a navigation, so the address is also looked at on a timer.
        func watch(_ view: WKWebView) {
            poll = Task { [weak self, weak view] in
                while !Task.isCancelled {
                    try? await Task.sleep(for: .seconds(1.5))
                    guard let self, let view else { return }
                    await self.check(view)
                }
            }
        }

        func stop() {
            poll?.cancel()
        }

        /// Only the service's own pages and the sign-in providers it offers; no other site, and
        /// no app link, opens inside a window that holds a session.
        func webView(
            _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
        ) async -> WKNavigationActionPolicy {
            guard let url = navigationAction.request.url, source.allowsLoginNavigation(to: url) else {
                return .cancel
            }
            return .allow
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            Task { await check(webView) }
        }

        private func check(_ view: WKWebView) async {
            guard !saved, let url = view.url, source.looksSignedIn(url: url) else { return }
            let cookies = await view.configuration.websiteDataStore.httpCookieStore.allCookies()
                .filter { source.ownsCookie(domain: $0.domain) }
            guard !saved, source.looksSignedIn(cookieNames: cookies.map(\.name)) else { return }
            let jar = cookies.map { cookie in
                NetscapeCookie(
                    domain: cookie.domain, hostOnly: !cookie.domain.hasPrefix("."),
                    path: cookie.path, secure: cookie.isSecure, httpOnly: cookie.isHTTPOnly,
                    expires: Int(cookie.expiresDate?.timeIntervalSince1970 ?? 0),
                    name: cookie.name, value: cookie.value)
            }
            do {
                try config.saveSession(jar, for: source)
            } catch {
                return
            }
            saved = true
            stop()
            signedIn()
        }
    }
}

extension MusicSource {
    /// Fixed, so the same service finds its profile again.
    fileprivate var profile: UUID {
        let id =
            switch self {
            case .spotify: "5B1F0D3E-8C44-4E0B-9A51-2E6B7A0C1D01"
            case .youtubeMusic: "5B1F0D3E-8C44-4E0B-9A51-2E6B7A0C1D02"
            case .tidal: "5B1F0D3E-8C44-4E0B-9A51-2E6B7A0C1D03"
            case .appleMusic, .local: "5B1F0D3E-8C44-4E0B-9A51-2E6B7A0C1D00"
            }
        return UUID(uuidString: id) ?? UUID()
    }
}
