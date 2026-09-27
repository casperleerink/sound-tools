// Draws the app icon, tooling/macos/AppIcon.png, 1024 x 1024.
// Run it after a change, from the repository root: swift tooling/macos/make-icon.swift
// The bundle script turns the PNG into AppIcon.icns.
//
// The rounded square of a macOS icon in the window colours of DESIGN.md, with a waveform of
// seven bars in the text colour. The middle bar is lavender, the colour of the agent.

import AppKit

let size = 1024
func colour(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(
        srgbRed: CGFloat((hex >> 16) & 0xff) / 255,
        green: CGFloat((hex >> 8) & 0xff) / 255,
        blue: CGFloat(hex & 0xff) / 255,
        alpha: alpha)
}

let space = CGColorSpace(name: CGColorSpace.sRGB)!
let context = CGContext(
    data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: space,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

// The grid of a macOS icon: an 824 pt square with 185 pt corners, 100 pt in from each edge.
let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
let shape = CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)

// A soft shadow under the tile, as the icons of the system have.
context.saveGState()
context.setShadow(offset: CGSize(width: 0, height: -10), blur: 24, color: colour(0x000000, 0.35))
context.addPath(shape)
context.setFillColor(colour(0x121317))
context.fillPath()
context.restoreGState()

// gray-200 at the top to gray-50 at the bottom.
context.saveGState()
context.addPath(shape)
context.clip()
let gradient = CGGradient(
    colorsSpace: space, colors: [colour(0x1b1d23), colour(0x0c0d10)] as CFArray,
    locations: [0, 1])!
context.drawLinearGradient(
    gradient, start: CGPoint(x: 0, y: tile.maxY), end: CGPoint(x: 0, y: tile.minY), options: [])
context.restoreGState()

// The border of a card, alpha/6, at icon scale.
let border = CGPath(
    roundedRect: tile.insetBy(dx: 2, dy: 2), cornerWidth: 183, cornerHeight: 183, transform: nil)
context.addPath(border)
context.setStrokeColor(colour(0xffffff, 0.08))
context.setLineWidth(4)
context.strokePath()

// Seven bars, mirrored about the middle line, as the waveform of an audio clip is drawn.
let heights: [CGFloat] = [0.22, 0.46, 0.74, 1.0, 0.62, 0.38, 0.18]
let barWidth: CGFloat = 56
let gap: CGFloat = 36
let tallest: CGFloat = 470
let total = CGFloat(heights.count) * barWidth + CGFloat(heights.count - 1) * gap
var x = (CGFloat(size) - total) / 2
for (index, height) in heights.enumerated() {
    let barHeight = max(barWidth, tallest * height)
    let bar = CGRect(x: x, y: (CGFloat(size) - barHeight) / 2, width: barWidth, height: barHeight)
    let rounded = CGPath(
        roundedRect: bar, cornerWidth: barWidth / 2, cornerHeight: barWidth / 2, transform: nil)
    context.addPath(rounded)
    context.setFillColor(index == 3 ? colour(0xa9b1ff) : colour(0xe9ebef))
    context.fillPath()
    x += barWidth + gap
}

let image = context.makeImage()!
let output = URL(fileURLWithPath: "tooling/macos/AppIcon.png")
let destination = CGImageDestinationCreateWithURL(
    output as CFURL, "public.png" as CFString, 1, nil)!
CGImageDestinationAddImage(destination, image, nil)
guard CGImageDestinationFinalize(destination) else { fatalError("could not write \(output.path)") }
print("wrote \(output.path)")
