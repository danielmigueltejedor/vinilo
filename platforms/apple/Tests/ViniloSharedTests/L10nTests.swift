// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Testing
import ViniloShared

@Suite("L10n")
struct L10nTests {
    /// Words that are the same in both languages because they are names, not sentences.
    private static let sameInBoth: Set<L10n.Key> = [.appName, .appleMusic, .edition]

    @Test("every key has English words, never the key's own name")
    func englishIsComplete() {
        for key in L10n.Key.allCases {
            let text = L10n.t(key, languageCode: "en")
            #expect(!text.isEmpty, "\(key) is empty in English")
            #expect(text != key.rawValue, "\(key) falls through to its raw name in English")
        }
    }

    @Test("every key is translated to Spanish rather than falling back to English")
    func spanishIsComplete() {
        for key in L10n.Key.allCases where !Self.sameInBoth.contains(key) {
            let es = L10n.t(key, languageCode: "es")
            #expect(!es.isEmpty, "\(key) is empty in Spanish")
            #expect(es != L10n.t(key, languageCode: "en"), "\(key) has no Spanish text")
        }
    }

    @Test("any other language gets English")
    func otherLanguagesFallBack() {
        #expect(L10n.t(.search, languageCode: "fr") == L10n.t(.search, languageCode: "en"))
        #expect(L10n.t(.search, languageCode: nil) == L10n.t(.search, languageCode: "en"))
    }

    @Test("the clock reads minutes and seconds, and hours only when there are some")
    func clock() {
        #expect(L10n.clock(ms: 0) == "0:00")
        #expect(L10n.clock(ms: 5_000) == "0:05")
        #expect(L10n.clock(ms: 185_000) == "3:05")
        #expect(L10n.clock(ms: 3_723_000) == "1:02:03")
    }

    @Test("a negative position reads as zero")
    func clockNeverGoesNegative() {
        #expect(L10n.clock(ms: -1) == "0:00")
        #expect(L10n.clock(ms: Int.min) == "0:00")
    }
}
