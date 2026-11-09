// Draws the app icon (polymeter rings + bars) into a PNG: swift make-icon.swift out.png
import AppKit

let size: CGFloat = 1024
let img = NSImage(size: NSSize(width: size, height: size))
img.lockFocus()
let ctx = NSGraphicsContext.current!.cgContext
let rect = CGRect(x: 0, y: 0, width: size, height: size).insetBy(dx: 90, dy: 90)
let path = CGPath(roundedRect: rect, cornerWidth: 190, cornerHeight: 190, transform: nil)
ctx.addPath(path)
ctx.clip()
let colors = [NSColor(calibratedRed: 0.10, green: 0.07, blue: 0.22, alpha: 1).cgColor,
              NSColor(calibratedRed: 0.28, green: 0.10, blue: 0.45, alpha: 1).cgColor] as CFArray
let grad = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors, locations: [0, 1])!
ctx.drawLinearGradient(grad, start: CGPoint(x: 0, y: size), end: CGPoint(x: size, y: 0), options: [])
let c = CGPoint(x: size / 2, y: size / 2)
let rings: [(CGFloat, Int, NSColor)] = [(300, 7, .systemPink), (225, 5, .systemOrange), (150, 8, .systemTeal)]
for (r, n, col) in rings {
    ctx.setStrokeColor(col.withAlphaComponent(0.35).cgColor)
    ctx.setLineWidth(10)
    ctx.strokeEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r))
    for i in 0..<n {
        let a = CGFloat(i) / CGFloat(n) * 2 * .pi + .pi / 2
        let p = CGPoint(x: c.x + cos(a) * r, y: c.y + sin(a) * r)
        ctx.setFillColor((i == 0 ? NSColor.white : col).cgColor)
        let d: CGFloat = i == 0 ? 54 : 40
        ctx.fillEllipse(in: CGRect(x: p.x - d / 2, y: p.y - d / 2, width: d, height: d))
    }
}
ctx.setFillColor(NSColor.white.withAlphaComponent(0.9).cgColor)
for (i, h) in [0.35, 0.7, 1.0, 0.55, 0.8].enumerated() {
    let w: CGFloat = 22, gap: CGFloat = 16
    let x = c.x - (5 * w + 4 * gap) / 2 + CGFloat(i) * (w + gap)
    let hh = 110 * CGFloat(h)
    ctx.addPath(CGPath(roundedRect: CGRect(x: x, y: c.y - hh / 2, width: w, height: hh), cornerWidth: 11, cornerHeight: 11, transform: nil))
    ctx.fillPath()
}
img.unlockFocus()
let rep = NSBitmapImageRep(data: img.tiffRepresentation!)!
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
