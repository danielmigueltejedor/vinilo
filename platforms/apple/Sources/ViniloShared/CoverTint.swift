// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import CoreGraphics
import Foundation
import ImageIO

/// A colour as three channels from 0 to 1.
public struct RGB: Equatable, Hashable, Sendable {
    public var red: Double
    public var green: Double
    public var blue: Double

    public init(red: Double, green: Double, blue: Double) {
        self.red = red
        self.green = green
        self.blue = blue
    }
}

/// The colour of a sleeve, for the player to wear: the cover's most coloured pixels, averaged,
/// then set to a depth white text reads on in either appearance.
public enum CoverTint {
    /// Reads `file` at 16×16. A cover that cannot be read gives no colour, never a wrong one.
    public static func of(file: String) -> RGB? {
        let url = URL(fileURLWithPath: file) as CFURL
        let options =
            [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceThumbnailMaxPixelSize: 32,
            ] as CFDictionary
        guard let source = CGImageSourceCreateWithURL(url, nil),
            let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options)
        else { return nil }
        return average(of: image).map(toned)
    }

    /// The same, from an image already in memory (a cover fetched over the network).
    public static func of(data: Data) -> RGB? {
        let options =
            [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceThumbnailMaxPixelSize: 32,
            ] as CFDictionary
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
            let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options)
        else { return nil }
        return average(of: image).map(toned)
    }

    /// Weighted by saturation, so a vivid detail is not lost in a grey field.
    static func average(of image: CGImage) -> RGB? {
        let side = 16
        var pixels = [UInt8](repeating: 0, count: side * side * 4)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard
                let context = CGContext(
                    data: buffer.baseAddress, width: side, height: side, bitsPerComponent: 8,
                    bytesPerRow: side * 4, space: CGColorSpaceCreateDeviceRGB(),
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            else { return false }
            context.interpolationQuality = .medium
            context.draw(image, in: CGRect(x: 0, y: 0, width: side, height: side))
            return true
        }
        guard drawn else { return nil }
        var sum = (red: 0.0, green: 0.0, blue: 0.0, weight: 0.0)
        for index in stride(from: 0, to: pixels.count, by: 4) {
            let red = Double(pixels[index]) / 255
            let green = Double(pixels[index + 1]) / 255
            let blue = Double(pixels[index + 2]) / 255
            let weight = 0.08 + hsl(RGB(red: red, green: green, blue: blue)).saturation
            sum = (
                sum.red + red * weight, sum.green + green * weight, sum.blue + blue * weight,
                sum.weight + weight
            )
        }
        guard sum.weight > 0 else { return nil }
        return RGB(
            red: sum.red / sum.weight, green: sum.green / sum.weight, blue: sum.blue / sum.weight)
    }

    /// The hue kept, the saturation held between washed out and garish, the lightness fixed.
    public static func toned(_ colour: RGB) -> RGB {
        let (hue, saturation, _) = hsl(colour)
        let kept = saturation < 0.06 ? saturation : min(0.72, max(0.38, saturation))
        return rgb(hue: hue, saturation: kept, lightness: 0.34)
    }

    static func hsl(_ colour: RGB) -> (hue: Double, saturation: Double, lightness: Double) {
        let high = max(colour.red, colour.green, colour.blue)
        let low = min(colour.red, colour.green, colour.blue)
        let lightness = (high + low) / 2
        let delta = high - low
        guard delta > 0.0001 else { return (0, 0, lightness) }
        let saturation = delta / (1 - abs(2 * lightness - 1))
        var hue: Double
        switch high {
        case colour.red: hue = ((colour.green - colour.blue) / delta).truncatingRemainder(dividingBy: 6)
        case colour.green: hue = (colour.blue - colour.red) / delta + 2
        default: hue = (colour.red - colour.green) / delta + 4
        }
        hue /= 6
        if hue < 0 { hue += 1 }
        return (hue, min(1, saturation), lightness)
    }

    static func rgb(hue: Double, saturation: Double, lightness: Double) -> RGB {
        let chroma = (1 - abs(2 * lightness - 1)) * saturation
        let sector = hue * 6
        let second = chroma * (1 - abs(sector.truncatingRemainder(dividingBy: 2) - 1))
        let (red, green, blue): (Double, Double, Double) =
            switch Int(sector) % 6 {
            case 0: (chroma, second, 0)
            case 1: (second, chroma, 0)
            case 2: (0, chroma, second)
            case 3: (0, second, chroma)
            case 4: (second, 0, chroma)
            default: (chroma, 0, second)
            }
        let offset = lightness - chroma / 2
        return RGB(red: red + offset, green: green + offset, blue: blue + offset)
    }
}
