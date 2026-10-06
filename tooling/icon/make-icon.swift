// Draws the app icon, tooling/icon/sound-tools.png, 1024 x 1024, and a copy at 512 x 512.
// Run it after a change, from the repository root: swift tooling/icon/make-icon.swift
// The macOS bundle script turns the large PNG into AppIcon.icns. The Linux one ships the small one.
// Windows builds sound-tools.ico into the program (crates/runtime/build.rs). Make it again with
// python3 -c "from PIL import Image; Image.open('tooling/icon/sound-tools.png').save('tooling/icon/sound-tools.ico', sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])"
//
// The rounded square of a macOS icon in the window colours of DESIGN.md, with two sine waves
// woven over and under each other: the composer in the text colour, the agent in lavender.

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

// Two mirrored sine waves of one and a half periods. They cross twice inside: at the first
// crossing the composer's wave lies on top, at the second the agent's, each with a gap in the
// tile colour at the middle line so the other passes under. Both waves stop short of the ends,
// where they would meet, so their round ends sit apart by the same gap.
let left: CGFloat = 218
let right: CGFloat = 806
let amplitude: CGFloat = 140
let periods: CGFloat = 1.5
let lineWidth: CGFloat = 40
let gap: CGFloat = 14
let middle = colour(0x131519)
// Where the two waves are one line width and a gap apart.
let trim = asin((lineWidth + gap) / (2 * amplitude)) / (periods * 2 * .pi)

func wave(_ sign: CGFloat, from start: CGFloat = 0, to end: CGFloat = 1) -> CGPath {
    let path = CGMutablePath()
    let steps = 200
    for step in 0...steps {
        let t = start + (end - start) * CGFloat(step) / CGFloat(steps)
        let point = CGPoint(
            x: left + (right - left) * t,
            y: CGFloat(size) / 2 + sign * amplitude * sin(t * periods * 2 * .pi))
        if step == 0 { path.move(to: point) } else { path.addLine(to: point) }
    }
    return path
}

func stroke(_ path: CGPath, _ fill: CGColor, width: CGFloat, cap: CGLineCap) {
    context.addPath(path)
    context.setStrokeColor(fill)
    context.setLineWidth(width)
    context.setLineCap(cap)
    context.setLineJoin(.round)
    context.strokePath()
}

let composer = colour(0xe9ebef)
let agent = colour(0xa9b1ff)
stroke(wave(1, from: trim, to: 1 - trim), composer, width: lineWidth, cap: .round)
stroke(wave(-1, from: trim, to: 1 - trim), agent, width: lineWidth, cap: .round)
// The gap is shorter than the piece over it, so the ends of the piece land on its own colour
// and leave no seam.
for (sign, fill, crossing) in [(CGFloat(1), composer, CGFloat(1) / 3), (-1, agent, 2 / 3)] {
    stroke(
        wave(sign, from: crossing - 0.06, to: crossing + 0.06), middle,
        width: lineWidth + 2 * gap, cap: .butt)
    stroke(wave(sign, from: crossing - 0.08, to: crossing + 0.08), fill, width: lineWidth, cap: .butt)
}

// 1024 for macOS, and 512 for Linux, the largest size of the hicolor icon theme.
func write(_ image: CGImage, _ path: String) {
    let output = URL(fileURLWithPath: path)
    let destination = CGImageDestinationCreateWithURL(
        output as CFURL, "public.png" as CFString, 1, nil)!
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else { fatalError("could not write \(path)") }
    print("wrote \(path)")
}

let image = context.makeImage()!
write(image, "tooling/icon/sound-tools.png")

let half = size / 2
let small = CGContext(
    data: nil, width: half, height: half, bitsPerComponent: 8, bytesPerRow: 0, space: space,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
small.interpolationQuality = .high
small.draw(image, in: CGRect(x: 0, y: 0, width: half, height: half))
write(small.makeImage()!, "tooling/icon/sound-tools-512.png")
