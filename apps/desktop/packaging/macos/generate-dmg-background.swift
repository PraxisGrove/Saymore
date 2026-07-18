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
swoop.move(to: NSPoint(x: 345, y: 195))
swoop.curve(
    to: NSPoint(x: 425, y: 213),
    controlPoint1: NSPoint(x: 365, y: 230),
    controlPoint2: NSPoint(x: 395, y: 235)
)
swoop.lineWidth = 5.5
swoop.lineCapStyle = .round
swoop.lineJoinStyle = .round
swoop.stroke()

let arrowhead = NSBezierPath()
arrowhead.move(to: NSPoint(x: 412, y: 232))
arrowhead.curve(
    to: NSPoint(x: 429, y: 210),
    controlPoint1: NSPoint(x: 418, y: 224),
    controlPoint2: NSPoint(x: 424, y: 217)
)
arrowhead.curve(
    to: NSPoint(x: 405, y: 194),
    controlPoint1: NSPoint(x: 421, y: 207),
    controlPoint2: NSPoint(x: 413, y: 202)
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
