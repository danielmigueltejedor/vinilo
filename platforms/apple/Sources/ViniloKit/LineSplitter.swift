// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Cuts a byte stream into the newline-delimited lines the daemon speaks. A chunk can end in the
/// middle of a line, or hold several, so what is left over waits for the next one.
public struct LineSplitter: Sendable {
    /// A line longer than this is not the daemon: the stream is dropped rather than grown.
    public static let maxLineBytes = 32 * 1024 * 1024

    private var buffer = Data()

    public init() {}

    public enum Failure: Error, Equatable {
        case lineTooLong
    }

    /// The complete lines in `chunk` (without their newline), in order.
    public mutating func feed(_ chunk: Data) throws -> [Data] {
        buffer.append(chunk)
        var lines: [Data] = []
        while let newline = buffer.firstIndex(of: 0x0A) {
            let line = buffer[buffer.startIndex..<newline]
            if !line.isEmpty {
                lines.append(Data(line))
            }
            buffer.removeSubrange(buffer.startIndex...newline)
        }
        if buffer.count > Self.maxLineBytes {
            buffer.removeAll()
            throw Failure.lineTooLong
        }
        return lines
    }
}
