// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

#if canImport(Darwin)
    import Darwin
#elseif canImport(Glibc)
    import Glibc
#endif

public enum DaemonError: Error, LocalizedError, Equatable {
    case noRuntimeDirectory
    case pathTooLong
    case socket(String)
    case executableMissing(String)
    case didNotStart

    public var errorDescription: String? {
        switch self {
        case .noRuntimeDirectory:
            "There is no per-user runtime directory (TMPDIR), so there is nowhere to put the socket."
        case .pathTooLong: "The socket path is longer than a Unix socket allows."
        case .socket(let detail): "The connection to the Vinilo engine failed: \(detail)"
        case .executableMissing(let path): "The Vinilo engine is missing from the app: \(path)"
        case .didNotStart: "The Vinilo engine did not start listening."
        }
    }
}

/// What a connection reports upward.
public enum Incoming: Sendable {
    case event(Event)
    /// A line that is not an event this build knows: the engine and the app disagree on the wire
    /// format, which is version skew rather than a transient error.
    case unparsed(String)
    /// The connection ended. Always the last message.
    case lost(String)
}

/// One connection to the daemon's Unix socket: requests out, events in.
///
/// Reading blocks a thread of its own, which is the honest shape of a socket read; everything
/// that leaves it is a value on an `AsyncStream`, so nothing blocking reaches the interface.
public final class DaemonConnection: @unchecked Sendable {
    private let descriptor: Int32
    private let writeLock = NSLock()
    private var closed = false
    public let incoming: AsyncStream<Incoming>

    private init(descriptor: Int32, incoming: AsyncStream<Incoming>) {
        self.descriptor = descriptor
        self.incoming = incoming
    }

    /// Opens the socket at `path`. Throws if nothing is listening there.
    public static func connect(path: String) throws -> DaemonConnection {
        let descriptor = socket(AF_UNIX, streamSocket, 0)
        guard descriptor >= 0 else { throw DaemonError.socket(String(cString: strerror(errno))) }

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard path.utf8.count < capacity else {
            posixClose(descriptor)
            throw DaemonError.pathTooLong
        }
        withUnsafeMutablePointer(to: &address.sun_path) { pointer in
            pointer.withMemoryRebound(to: CChar.self, capacity: capacity) { destination in
                _ = path.withCString { strncpy(destination, $0, capacity - 1) }
            }
        }
        #if canImport(Darwin)
            // A write to a daemon that has gone must be an error, not a signal.
            var one: Int32 = 1
            setsockopt(
                descriptor, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size))
        #endif
        let length = socklen_t(MemoryLayout<sockaddr_un>.size)
        let result = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                posixConnect(descriptor, $0, length)
            }
        }
        guard result == 0 else {
            let detail = String(cString: strerror(errno))
            posixClose(descriptor)
            throw DaemonError.socket(detail)
        }

        let (stream, continuation) = AsyncStream<Incoming>.makeStream(
            bufferingPolicy: .unbounded)
        let connection = DaemonConnection(descriptor: descriptor, incoming: stream)
        Thread.detachNewThread {
            Self.read(descriptor: descriptor, into: continuation)
        }
        return connection
    }

    private static func read(descriptor: Int32, into continuation: AsyncStream<Incoming>.Continuation) {
        let decoder = WireDecoder.make()
        var splitter = LineSplitter()
        var buffer = [UInt8](repeating: 0, count: 64 * 1024)
        while true {
            let count = buffer.withUnsafeMutableBytes { posixRead(descriptor, $0.baseAddress, $0.count) }
            if count < 0 && errno == EINTR { continue }
            if count <= 0 {
                let why = count == 0 ? "the engine closed the connection" : String(cString: strerror(errno))
                continuation.yield(.lost(why))
                continuation.finish()
                return
            }
            do {
                for line in try splitter.feed(Data(buffer[0..<count])) {
                    if let event = try? decoder.decode(Event.self, from: line) {
                        continuation.yield(.event(event))
                    } else {
                        continuation.yield(.unparsed(String(decoding: line, as: UTF8.self)))
                    }
                }
            } catch {
                continuation.yield(.lost("the engine sent a line that is too long"))
                continuation.finish()
                return
            }
        }
    }

    /// Writes a request. A request is fire-and-forget: its answer is an event.
    public func send(_ request: Request) {
        guard let line = try? request.line() else { return }
        writeLock.lock()
        defer { writeLock.unlock() }
        guard !closed else { return }
        var offset = 0
        line.withUnsafeBytes { bytes in
            while offset < bytes.count {
                let written = posixWrite(
                    descriptor, bytes.baseAddress.map { $0 + offset }, bytes.count - offset)
                if written < 0 && errno == EINTR { continue }
                if written <= 0 { return }
                offset += written
            }
        }
    }

    /// Ends the connection. The stream finishes after a last `.lost`.
    public func close() {
        writeLock.lock()
        defer { writeLock.unlock() }
        guard !closed else { return }
        closed = true
        shutdown(descriptor, shutBoth)
    }

    deinit {
        // The reader thread may still be inside `read`; `shutdown` wakes it, and only then is
        // the descriptor released.
        if !closed { shutdown(descriptor, shutBoth) }
        posixClose(descriptor)
    }
}

// SOCK_STREAM and SHUT_RDWR are macros whose Swift type differs between Darwin and Glibc; their
// values do not.
private let streamSocket: Int32 = 1
private let shutBoth: Int32 = 2

// `connect`, `read`, `write` and `close` are also the names of this type's own members, which
// would shadow the C functions inside it. These are the C functions by another name.
private func posixConnect(
    _ fd: Int32, _ address: UnsafePointer<sockaddr>, _ length: socklen_t
) -> Int32 {
    connect(fd, address, length)
}

private func posixRead(_ fd: Int32, _ buffer: UnsafeMutableRawPointer?, _ count: Int) -> Int {
    read(fd, buffer, count)
}

private func posixWrite(_ fd: Int32, _ buffer: UnsafeRawPointer?, _ count: Int) -> Int {
    write(fd, buffer, count)
}

private func posixClose(_ fd: Int32) {
    _ = close(fd)
}
