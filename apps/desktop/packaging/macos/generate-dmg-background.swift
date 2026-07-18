import AppKit
import Foundation

let width = 660
let height = 420

guard let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: width,
    pixelsHigh: height,
    bitsPerSample: 8,
    samplesPerPixel: 4,
    hasAlpha: true,
    isPlanar: false,
    colorSpaceName: .deviceRGB,
    bytesPerRow: 0,
    bitsPerPixel: 0
) else {
    FileHandle.standardError.write(Data("failed to create DMG background bitmap\n".utf8))
    exit(1)
}

NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)

NSColor(red: 245 / 255, green: 245 / 255, blue: 244 / 255, alpha: 1).setFill()
NSRect(x: 0, y: 0, width: width, height: height).fill()

NSColor(red: 79 / 255, green: 83 / 255, blue: 89 / 255, alpha: 1).setStroke()

let swoop = NSBezierPath()
swoop.move(to: NSPoint(x: 246, y: 195))
swoop.curve(
    to: NSPoint(x: 397, y: 213),
    controlPoint1: NSPoint(x: 278, y: 240),
    controlPoint2: NSPoint(x: 337, y: 245)
)
swoop.lineWidth = 5.5
swoop.lineCapStyle = .round
swoop.lineJoinStyle = .round
swoop.stroke()

let arrowhead = NSBezierPath()
arrowhead.move(to: NSPoint(x: 384, y: 233))
arrowhead.curve(
    to: NSPoint(x: 401, y: 211),
    controlPoint1: NSPoint(x: 390, y: 225),
    controlPoint2: NSPoint(x: 396, y: 218)
)
arrowhead.curve(
    to: NSPoint(x: 372, y: 195),
    controlPoint1: NSPoint(x: 391, y: 208),
    controlPoint2: NSPoint(x: 381, y: 203)
)
arrowhead.lineWidth = 5.5
arrowhead.lineCapStyle = .round
arrowhead.lineJoinStyle = .round
arrowhead.stroke()

NSGraphicsContext.restoreGraphicsState()

guard let png = bitmap.representation(using: .png, properties: [:]) else {
    FileHandle.standardError.write(Data("failed to encode DMG background PNG\n".utf8))
    exit(1)
}

let output = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()
    .appendingPathComponent("dmg-background.png")

do {
    try png.write(to: output)
} catch {
    FileHandle.standardError.write(Data("failed to write \(output.path): \(error)\n".utf8))
    exit(1)
}
