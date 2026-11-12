// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Testing

@testable import ViniloShared

@Suite("Cover tint")
struct CoverTintTests {
    @Test("a vivid red stays red, at a depth white text reads on")
    func red() {
        let tone = CoverTint.toned(RGB(red: 1, green: 0.1, blue: 0.1))
        #expect(tone.red > tone.green && tone.red > tone.blue)
        #expect(abs(CoverTint.hsl(tone).lightness - 0.34) < 0.01)
    }

    @Test("a grey sleeve gives a grey, not an invented colour")
    func grey() {
        let tone = CoverTint.toned(RGB(red: 0.8, green: 0.8, blue: 0.8))
        #expect(abs(tone.red - tone.green) < 0.001 && abs(tone.green - tone.blue) < 0.001)
    }

    @Test("a cover that cannot be read gives no colour")
    func missing() {
        #expect(CoverTint.of(file: "/nonexistent/cover.jpg") == nil)
    }
}
