import SwiftUI

/// Loading animation for `.agbsong` files, cyberpunk style: on a rainy neon
/// street a hovering android carries a cassette to a tape deck, drops it in
/// and presses play. Pure function of `progress` (0…1) so it can be rendered
/// frame by frame.
struct TapeLoadingScene: View {
    let progress: Double
    /// Written on the cassette label.
    let title: String

    static let duration: Double = 4.0
    /// Moment the play button goes down.
    static let pressAt: Double = 0.84

    // Scene coordinates: 640 × 400.
    private static let well = CGPoint(x: 480, y: 214)        // cassette resting place inside the deck
    private static let playButton = CGPoint(x: 481, y: 300)

    private static let cyan = Color(red: 0.20, green: 0.95, blue: 1.00)
    private static let magenta = Color(red: 1.00, green: 0.18, blue: 0.72)
    private static let yellow = Color(red: 1.00, green: 0.88, blue: 0.30)
    private static let violet = Color(red: 0.56, green: 0.36, blue: 1.00)
    private static let steelHi = Color(red: 0.36, green: 0.40, blue: 0.56)
    private static let steel = Color(red: 0.17, green: 0.19, blue: 0.30)
    private static let steelLo = Color(red: 0.07, green: 0.08, blue: 0.14)

    // MARK: helpers

    private func ease(_ x: Double) -> Double {
        let t = min(max(x, 0), 1)
        return t * t * t * (t * (t * 6 - 15) + 10)
    }
    /// Ease with a small overshoot, for things that settle.
    private func back(_ x: Double) -> Double {
        let t = min(max(x, 0), 1) - 1
        return 1 + t * t * (2.2 * t + 1.2)
    }
    private func phase(_ a: Double, _ b: Double) -> Double { ease((progress - a) / (b - a)) }
    private func mix(_ a: CGPoint, _ b: CGPoint, _ t: Double) -> CGPoint {
        CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)
    }
    private func rnd(_ i: Int) -> Double {
        let x = sin(Double(i) * 12.9898 + 4.1414) * 43758.5453
        return x - floor(x)
    }
    private func pt(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: x, y: y) }

    /// Stroke with a neon halo.
    private func neon(_ ctx: inout GraphicsContext, _ path: Path, _ color: Color, width: Double = 2, glow: Double = 6, alpha: Double = 1) {
        ctx.drawLayer { l in
            l.addFilter(.blur(radius: glow))
            l.stroke(path, with: .color(color.opacity(0.9 * alpha)), style: StrokeStyle(lineWidth: width * 2.4, lineCap: .round, lineJoin: .round))
        }
        ctx.stroke(path, with: .color(color.opacity(alpha)), style: StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round))
        ctx.stroke(path, with: .color(Color.white.opacity(0.55 * alpha)), style: StrokeStyle(lineWidth: width * 0.35, lineCap: .round, lineJoin: .round))
    }

    /// Filled shape with a soft halo.
    private func glowFill(_ ctx: inout GraphicsContext, _ path: Path, _ color: Color, glow: Double = 8, alpha: Double = 1) {
        ctx.drawLayer { l in
            l.addFilter(.blur(radius: glow))
            l.fill(path, with: .color(color.opacity(0.85 * alpha)))
        }
        ctx.fill(path, with: .color(color.opacity(alpha)))
    }

    /// Box with cut corners — the angular hardware look.
    private func chamfer(_ r: CGRect, _ c: Double) -> Path {
        var p = Path()
        p.move(to: pt(r.minX + c, r.minY)); p.addLine(to: pt(r.maxX - c, r.minY))
        p.addLine(to: pt(r.maxX, r.minY + c)); p.addLine(to: pt(r.maxX, r.maxY - c))
        p.addLine(to: pt(r.maxX - c, r.maxY)); p.addLine(to: pt(r.minX + c, r.maxY))
        p.addLine(to: pt(r.minX, r.maxY - c)); p.addLine(to: pt(r.minX, r.minY + c))
        p.closeSubpath()
        return p
    }

    private func label(_ ctx: inout GraphicsContext, _ s: String, at p: CGPoint, size: Double, color: Color, glow: Double = 5, alpha: Double = 1,
                       weight: Font.Weight = .bold, design: Font.Design = .monospaced) {
        let t = Text(s).font(.system(size: size, weight: weight, design: design)).foregroundColor(color.opacity(alpha))
        ctx.drawLayer { l in
            l.addFilter(.blur(radius: glow))
            l.draw(t, at: p)
        }
        ctx.draw(t, at: p)
    }

    var body: some View {
        Canvas { ctx, size in
            let s = min(size.width / 640, size.height / 400)
            ctx.translateBy(x: (size.width - 640 * s) / 2, y: (size.height - 400 * s) / 2)
            ctx.scaleBy(x: s, y: s)
            ctx.clip(to: Path(roundedRect: CGRect(x: 0, y: 0, width: 640, height: 400), cornerRadius: 14))

            let t = progress * Self.duration // seconds, for continuous motion
            let enter = back((progress - 0.0) / 0.18)
            let carry = phase(0.18, 0.52)
            let lidOpen = phase(0.26, 0.44) - phase(0.66, 0.74)
            let drop = phase(0.52, 0.66)
            let reach = phase(0.69, 0.81)
            let press = phase(0.81, Self.pressAt) - phase(Self.pressAt, 0.90)
            let playing = progress >= Self.pressAt
            let since = max(0, progress - Self.pressAt) * Self.duration // seconds since the press
            let after = phase(0.90, 1.0)
            let robotX = -280 * (1 - enter)
            let bob = sin(t * 2.6) * 3.5

            // --- cassette and hand positions ---
            let holdUp = pt(266 + robotX, 190 + bob)
            let overWell = pt(Self.well.x, 118)
            var cassette = mix(holdUp, overWell, carry)
            cassette.y -= sin(carry * .pi) * 30
            cassette = mix(cassette, Self.well, drop)
            let tilt = (1 - carry) * -0.16 + sin(carry * .pi) * 0.05
            var hand: CGPoint
            if progress < 0.52 {
                hand = pt(cassette.x - 8, cassette.y + 34)
            } else if progress < 0.69 {
                hand = mix(pt(overWell.x - 8, overWell.y + 34), pt(Self.well.x - 30, 150), drop)
            } else {
                let above = pt(Self.playButton.x, Self.playButton.y - 26)
                hand = mix(pt(Self.well.x - 30, 150), above, reach)
                hand.y += press * 14
                hand = mix(hand, pt(404, 236), after)
            }

            drawCity(&ctx, t: t, playing: playing, since: since)
            drawFloor(&ctx, t: t, playing: playing)
            drawDeck(&ctx, t: t, press: press, playing: playing, since: since, cassetteIn: drop >= 1, lid: lidOpen)
            if drop < 1 {
                // motion trail
                if carry > 0.02 && carry < 0.98 {
                    for k in 1...3 {
                        let c2 = max(0, carry - Double(k) * 0.035)
                        var g = mix(holdUp, overWell, c2); g.y -= sin(c2 * .pi) * 30
                        drawCassette(&ctx, at: g, tilt: tilt, spin: 0, alpha: 0.10 / Double(k) * 2)
                    }
                }
                drawCassette(&ctx, at: cassette, tilt: tilt, spin: t * 1.5, alpha: 1)
            }
            drawRobot(&ctx, x: robotX, bob: bob, hand: hand, t: t, happy: playing, since: since, gripping: progress < 0.60, bend: 1 - 1.6 * reach)
            drawRain(&ctx, t: t)
            if playing { drawPressBurst(&ctx, since: since) }
            drawScreenFx(&ctx, since: since, playing: playing)
        }
        .aspectRatio(640 / 400, contentMode: .fit)
    }

    // MARK: background

    private func drawCity(_ ctx: inout GraphicsContext, t: Double, playing: Bool, since: Double) {
        ctx.fill(Path(CGRect(x: 0, y: 0, width: 640, height: 400)),
                 with: .linearGradient(Gradient(stops: [
                    .init(color: Color(red: 0.03, green: 0.02, blue: 0.09), location: 0),
                    .init(color: Color(red: 0.10, green: 0.03, blue: 0.20), location: 0.55),
                    .init(color: Color(red: 0.30, green: 0.05, blue: 0.34), location: 0.84),
                    .init(color: Color(red: 0.05, green: 0.02, blue: 0.10), location: 1),
                 ]), startPoint: pt(0, 0), endPoint: pt(0, 400)))
        // haze on the horizon
        ctx.drawLayer { l in
            l.addFilter(.blur(radius: 40))
            l.fill(Path(ellipseIn: CGRect(x: 60, y: 250, width: 520, height: 120)), with: .color(Self.magenta.opacity(0.28)))
            l.fill(Path(ellipseIn: CGRect(x: 330, y: 150, width: 300, height: 220)), with: .color(Self.cyan.opacity(playing ? 0.16 : 0.07)))
        }
        // far skyline
        let far: [(Double, Double, Double)] = [(0, 54, 150), (50, 40, 196), (96, 62, 128), (164, 36, 220), (204, 70, 160), (280, 44, 240),
                                               (330, 58, 176), (392, 38, 208), (436, 76, 140), (516, 42, 232), (562, 80, 168)]
        for (i, b) in far.enumerated() {
            let r = CGRect(x: b.0, y: 338 - b.2, width: b.1, height: b.2)
            ctx.fill(Path(r), with: .color(Color(red: 0.07, green: 0.04, blue: 0.16)))
            // lit windows
            let cols = Int(b.1 / 9), rows = Int(b.2 / 12)
            for cx in 0..<cols {
                for ry in 0..<rows where rnd(i * 97 + cx * 13 + ry * 7) > 0.72 {
                    let flick = rnd(i * 31 + cx * 5 + ry * 3 + Int(t * 3)) > 0.06 ? 1.0 : 0.3
                    let c = rnd(i + cx * 3 + ry) > 0.5 ? Self.cyan : Self.magenta
                    ctx.fill(Path(CGRect(x: b.0 + 4 + Double(cx) * 9, y: 338 - b.2 + 6 + Double(ry) * 12, width: 4, height: 5)),
                             with: .color(c.opacity(0.42 * flick)))
                }
            }
        }
        // neon signs
        let flick = (rnd(Int(t * 9)) > 0.12) ? 1.0 : 0.35
        neon(&ctx, Path(roundedRect: CGRect(x: 46, y: 52, width: 44, height: 92), cornerRadius: 4), Self.magenta, width: 1.6, glow: 7, alpha: 0.85 * flick)
        label(&ctx, "夜", at: pt(68, 78), size: 26, color: Self.magenta, glow: 7, alpha: flick, design: .serif)
        label(&ctx, "曲", at: pt(68, 118), size: 26, color: Self.magenta, glow: 7, alpha: flick, design: .serif)
        let on = playing ? min(1, since * 3) * (rnd(Int(t * 14)) > 0.1 || since > 0.5 ? 1 : 0.3) : 0.16
        neon(&ctx, chamfer(CGRect(x: 508, y: 42, width: 104, height: 34), 7), Self.cyan, width: 1.6, glow: 8, alpha: max(on, 0.22))
        label(&ctx, "▶ PLAY", at: pt(560, 59), size: 15, color: Self.cyan, glow: 8, alpha: max(on, 0.22))
        label(&ctx, "AGB · TAPE", at: pt(250, 96), size: 9, color: Self.violet, glow: 4, alpha: 0.7)
    }

    private func drawFloor(_ ctx: inout GraphicsContext, t: Double, playing: Bool) {
        let horizon = 338.0
        ctx.fill(Path(CGRect(x: 0, y: horizon, width: 640, height: 62)),
                 with: .linearGradient(Gradient(colors: [Color(red: 0.10, green: 0.03, blue: 0.18), Color(red: 0.02, green: 0.01, blue: 0.06)]),
                                       startPoint: pt(0, horizon), endPoint: pt(0, 400)))
        var grid = Path()
        for i in -14...14 {
            grid.move(to: pt(320 + Double(i) * 16, horizon))
            grid.addLine(to: pt(320 + Double(i) * 92, 400))
        }
        // horizontal lines scroll toward the viewer
        for k in 0..<7 {
            let f = (Double(k) + (t * 0.5).truncatingRemainder(dividingBy: 1)) / 7
            let y = horizon + pow(f, 2.2) * 62
            grid.move(to: pt(0, y)); grid.addLine(to: pt(640, y))
        }
        ctx.stroke(grid, with: .color(Self.cyan.opacity(playing ? 0.30 : 0.18)), lineWidth: 0.8)
        var edge = Path(); edge.move(to: pt(0, horizon)); edge.addLine(to: pt(640, horizon))
        neon(&ctx, edge, Self.magenta, width: 1.2, glow: 5, alpha: 0.7)
        // wet-street reflections
        ctx.drawLayer { l in
            l.addFilter(.blur(radius: 14))
            l.fill(Path(ellipseIn: CGRect(x: 380, y: 342, width: 200, height: 34)), with: .color(Self.magenta.opacity(0.35)))
            l.fill(Path(ellipseIn: CGRect(x: 110, y: 344, width: 120, height: 24)), with: .color(Self.cyan.opacity(0.28)))
        }
    }

    private func drawRain(_ ctx: inout GraphicsContext, t: Double) {
        var rain = Path()
        for i in 0..<70 {
            let speed = 260 + rnd(i * 3) * 220
            let x0 = rnd(i * 7 + 1) * 760 - 60
            let y = (rnd(i * 11 + 2) * 440 + t * speed).truncatingRemainder(dividingBy: 440) - 20
            let x = x0 - y * 0.22
            let len = 7 + rnd(i * 5) * 10
            rain.move(to: pt(x, y)); rain.addLine(to: pt(x - len * 0.22, y + len))
        }
        ctx.stroke(rain, with: .color(Color(red: 0.7, green: 0.85, blue: 1).opacity(0.16)), lineWidth: 0.9)
    }

    // MARK: cassette

    private func drawCassette(_ ctx: inout GraphicsContext, at c: CGPoint, tilt: Double, spin: Double, alpha: Double) {
        ctx.drawLayer { g in
            g.opacity = alpha
            g.translateBy(x: c.x, y: c.y)
            g.rotate(by: .radians(tilt))
            let shell = chamfer(CGRect(x: -62, y: -38, width: 124, height: 76), 7)
            g.fill(shell, with: .linearGradient(Gradient(colors: [Color(red: 0.16, green: 0.17, blue: 0.28), Color(red: 0.06, green: 0.06, blue: 0.12)]),
                                                startPoint: pt(0, -38), endPoint: pt(0, 38)))
            neon(&g, shell, Self.cyan, width: 1.3, glow: 5, alpha: 0.95)
            // label
            let lab = CGRect(x: -54, y: -31, width: 108, height: 24)
            g.fill(Path(roundedRect: lab, cornerRadius: 3),
                   with: .linearGradient(Gradient(colors: [Self.magenta, Self.violet, Self.cyan]), startPoint: pt(-54, 0), endPoint: pt(54, 0)))
            g.draw(Text(title).font(.system(size: 10.5, weight: .bold, design: .rounded)).foregroundColor(.white), in: lab.insetBy(dx: 5, dy: 4))
            // tape window
            let win = Path(roundedRect: CGRect(x: -42, y: -2, width: 84, height: 27), cornerRadius: 13.5)
            g.fill(win, with: .color(Color.black.opacity(0.8)))
            g.stroke(win, with: .color(Self.cyan.opacity(0.5)), lineWidth: 0.8)
            for dx in [-25.0, 25.0] {
                let hub = Path(ellipseIn: CGRect(x: dx - 9.5, y: 2, width: 19, height: 19))
                g.stroke(hub, with: .color(Self.cyan), lineWidth: 1.6)
                for k in 0..<6 {
                    let a = spin * (dx < 0 ? 1 : 1.15) + Double(k) * .pi / 3
                    var tooth = Path()
                    tooth.move(to: pt(dx + cos(a) * 3.5, 11.5 + sin(a) * 3.5))
                    tooth.addLine(to: pt(dx + cos(a) * 8, 11.5 + sin(a) * 8))
                    g.stroke(tooth, with: .color(Color.white.opacity(0.85)), lineWidth: 1.4)
                }
            }
            // screws + notch
            for sx in [-55.0, 55.0] { g.fill(Path(ellipseIn: CGRect(x: sx - 2, y: 28, width: 4, height: 4)), with: .color(Self.steelHi)) }
            g.fill(Path(CGRect(x: -26, y: 31, width: 52, height: 4)), with: .color(Color.black.opacity(0.5)))
        }
    }

    // MARK: tape deck

    private func drawDeck(_ ctx: inout GraphicsContext, t: Double, press: Double, playing: Bool, since: Double, cassetteIn: Bool, lid: Double) {
        let beat = playing ? (0.5 + 0.5 * sin(since * 11)) : 0
        // handle
        var handle = Path()
        handle.move(to: pt(398, 150)); handle.addLine(to: pt(416, 122)); handle.addLine(to: pt(544, 122)); handle.addLine(to: pt(562, 150))
        ctx.stroke(handle, with: .color(Self.steel), style: StrokeStyle(lineWidth: 8, lineCap: .round, lineJoin: .round))
        ctx.stroke(handle, with: .color(Self.steelHi.opacity(0.7)), style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
        // body
        let bodyRect = CGRect(x: 352, y: 148, width: 256, height: 190)
        let body = chamfer(bodyRect, 16)
        ctx.fill(body, with: .linearGradient(Gradient(colors: [Color(red: 0.15, green: 0.14, blue: 0.24), Color(red: 0.05, green: 0.05, blue: 0.10)]),
                                             startPoint: pt(0, 148), endPoint: pt(0, 338)))
        neon(&ctx, body, Self.magenta, width: 1.6, glow: 9, alpha: 0.75 + 0.25 * beat)
        // panel seams
        var seam = Path()
        seam.move(to: pt(352, 268)); seam.addLine(to: pt(608, 268))
        ctx.stroke(seam, with: .color(Color.white.opacity(0.08)), lineWidth: 1)

        // cassette well
        let well = CGRect(x: 406, y: 166, width: 148, height: 96)
        ctx.fill(Path(roundedRect: well, cornerRadius: 6), with: .color(Color.black.opacity(0.85)))
        ctx.drawLayer { l in
            l.clip(to: Path(roundedRect: well, cornerRadius: 6))
            l.addFilter(.blur(radius: 10))
            l.stroke(Path(roundedRect: well, cornerRadius: 6), with: .color(Self.cyan.opacity(cassetteIn ? 0.8 : 0.45)), lineWidth: 8)
        }
        if cassetteIn { drawCassette(&ctx, at: Self.well, tilt: 0, spin: playing ? since * 5 : 0, alpha: 1) }
        // lid: a glass door that lifts to let the cassette in
        ctx.drawLayer { l in
            l.clip(to: Path(roundedRect: well.insetBy(dx: -1, dy: -1), cornerRadius: 6))
            let glass = CGRect(x: well.minX, y: well.minY - lid * 90, width: well.width, height: well.height)
            l.fill(Path(glass), with: .linearGradient(Gradient(colors: [Color.white.opacity(0.16), Color.white.opacity(0.03)]),
                                                      startPoint: pt(well.minX, glass.minY), endPoint: pt(well.maxX, glass.maxY)))
            var shine = Path(); shine.move(to: pt(well.minX + 18, glass.maxY)); shine.addLine(to: pt(well.minX + 52, glass.minY))
            l.stroke(shine, with: .color(Color.white.opacity(0.10)), lineWidth: 10)
        }
        neon(&ctx, Path(roundedRect: well, cornerRadius: 6), Self.cyan, width: 1.1, glow: 4, alpha: 0.8)

        // speakers
        for cx in [379.0, 581.0] {
            let pulse = 1 + beat * 0.10
            for (k, r) in [17.0, 11.5, 5.5].enumerated() {
                let ring = Path(ellipseIn: CGRect(x: cx - r * pulse, y: 196 - r * pulse, width: r * 2 * pulse, height: r * 2 * pulse))
                if k == 2 { ctx.fill(ring, with: .color(playing ? Self.cyan.opacity(0.9) : Self.steelHi)) }
                else { ctx.stroke(ring, with: .color((playing ? Self.cyan : Self.steelHi).opacity(k == 0 ? 0.9 : 0.5)), lineWidth: 1.4) }
            }
            // level bars under each speaker
            for i in 0..<5 {
                let h = playing ? 4 + abs(sin(since * (7 + Double(i) * 1.7) + Double(i) + cx)) * 22 : 3
                let bar = CGRect(x: cx - 15 + Double(i) * 6.5, y: 256 - h, width: 4, height: h)
                ctx.fill(Path(bar), with: .linearGradient(Gradient(colors: [Self.magenta, Self.cyan]), startPoint: pt(0, 256), endPoint: pt(0, 226)))
            }
        }

        // display
        let disp = CGRect(x: 406, y: 271, width: 148, height: 15)
        ctx.fill(Path(roundedRect: disp, cornerRadius: 3), with: .color(Color.black.opacity(0.8)))
        let blink = Int(t * 4) % 2 == 0
        let msg = playing ? "▶ PLAY" : cassetteIn ? "TAPE OK" : (blink ? "INSERT TAPE" : "INSERT TAPE_")
        label(&ctx, msg, at: pt(disp.midX, disp.midY), size: 8.5, color: playing ? Self.yellow : Self.cyan, glow: 3)

        // transport keys — play is the middle one
        for i in 0..<5 {
            let isPlay = i == 2
            let x = 392.0 + Double(i) * 36.5
            let down = isPlay ? press * 5 + (playing ? 2 : 0) : 0
            let r = CGRect(x: x, y: 291 + down, width: 30, height: 20)
            let key = chamfer(r, 4)
            ctx.fill(key, with: .linearGradient(Gradient(colors: [Self.steelHi.opacity(isPlay ? 0.9 : 0.55), Self.steelLo]),
                                                startPoint: pt(0, r.minY), endPoint: pt(0, r.maxY)))
            if isPlay {
                let c = playing ? Self.yellow : Self.cyan
                if playing { glowFill(&ctx, key, Self.yellow, glow: 10, alpha: 0.9) }
                neon(&ctx, key, c, width: 1.2, glow: playing ? 9 : 4)
                var tri = Path()
                tri.move(to: pt(r.midX - 4, r.midY - 5)); tri.addLine(to: pt(r.midX + 6, r.midY)); tri.addLine(to: pt(r.midX - 4, r.midY + 5))
                tri.closeSubpath()
                ctx.fill(tri, with: .color(playing ? Self.steelLo : Self.cyan))
            } else {
                ctx.stroke(key, with: .color(Color.white.opacity(0.14)), lineWidth: 0.8)
            }
        }
        // feet
        for x in [376.0, 564.0] {
            ctx.fill(chamfer(CGRect(x: x, y: 336, width: 22, height: 7), 2), with: .color(Self.steel))
        }
    }

    // MARK: android

    private func capsule(_ ctx: inout GraphicsContext, _ a: CGPoint, _ b: CGPoint, width: Double, trim: Color) {
        var p = Path(); p.move(to: a); p.addLine(to: b)
        ctx.stroke(p, with: .color(Self.steelLo), style: StrokeStyle(lineWidth: width + 2.5, lineCap: .round))
        ctx.stroke(p, with: .linearGradient(Gradient(colors: [Self.steelHi, Self.steel]), startPoint: a, endPoint: b),
                   style: StrokeStyle(lineWidth: width, lineCap: .round))
        // trim line along the limb
        let dx = b.x - a.x, dy = b.y - a.y, len = max(1, (dx * dx + dy * dy).squareRoot())
        let nx = -dy / len * width * 0.22, ny = dx / len * width * 0.22
        var tline = Path()
        tline.move(to: pt(a.x + dx * 0.18 + nx, a.y + dy * 0.18 + ny)); tline.addLine(to: pt(a.x + dx * 0.82 + nx, a.y + dy * 0.82 + ny))
        ctx.stroke(tline, with: .color(trim.opacity(0.85)), style: StrokeStyle(lineWidth: 1.2, lineCap: .round))
    }

    private func joint(_ ctx: inout GraphicsContext, _ c: CGPoint, r: Double, color: Color) {
        ctx.fill(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2)),
                 with: .radialGradient(Gradient(colors: [Self.steelHi, Self.steelLo]), center: pt(c.x - r * 0.3, c.y - r * 0.3), startRadius: 0, endRadius: r * 1.3))
        neon(&ctx, Path(ellipseIn: CGRect(x: c.x - r * 0.45, y: c.y - r * 0.45, width: r * 0.9, height: r * 0.9)), color, width: 1, glow: 3)
    }

    private func drawRobot(_ ctx: inout GraphicsContext, x: Double, bob: Double, hand: CGPoint, t: Double, happy: Bool, since: Double, gripping: Bool, bend: Double) {
        // light pool and shadow on the street
        ctx.drawLayer { l in
            l.addFilter(.blur(radius: 9))
            l.fill(Path(ellipseIn: CGRect(x: 170 + x - 46, y: 342 - bob * 0.3, width: 92, height: 16)), with: .color(Self.cyan.opacity(0.5)))
        }
        var g = ctx
        g.translateBy(x: x, y: bob)
        let h = pt(hand.x - x, hand.y - bob) // working hand in robot space
        let lean = min(max((h.x - 260) / 900, -0.02), 0.10) // lean toward the work

        // ---- thruster ----
        let flick = 0.75 + 0.25 * sin(t * 31) + 0.1 * sin(t * 53)
        g.drawLayer { l in
            l.addFilter(.blur(radius: 5))
            var flame = Path()
            flame.move(to: pt(160, 310)); flame.addQuadCurve(to: pt(170, 318 + 22 * flick), control: pt(161, 326))
            flame.addQuadCurve(to: pt(180, 310), control: pt(179, 326))
            l.fill(flame, with: .linearGradient(Gradient(colors: [.white, Self.cyan, Self.cyan.opacity(0)]), startPoint: pt(170, 308), endPoint: pt(170, 342)))
        }

        g.drawLayer { b in
            // whole upper body leans a little
            b.translateBy(x: 170, y: 300); b.rotate(by: .radians(lean)); b.translateBy(x: -170, y: -300)

            // ---- far arm (behind the torso) ----
            let sway = sin(t * 2.6 + 1) * 3
            let ls = pt(138, 228), le = pt(122 - sway, 262), lh = pt(130 + sway, 292)
            capsule(&b, ls, le, width: 9, trim: Self.magenta)
            capsule(&b, le, lh, width: 7.5, trim: Self.magenta)
            joint(&b, le, r: 5.5, color: Self.magenta)
            joint(&b, lh, r: 5, color: Self.cyan)

            // ---- hips / thruster housing ----
            var hips = Path()
            hips.move(to: pt(151, 290)); hips.addLine(to: pt(189, 290)); hips.addLine(to: pt(181, 312)); hips.addLine(to: pt(159, 312)); hips.closeSubpath()
            b.fill(hips, with: .linearGradient(Gradient(colors: [Self.steel, Self.steelLo]), startPoint: pt(0, 290), endPoint: pt(0, 312)))
            neon(&b, Path(CGRect(x: 160, y: 310, width: 20, height: 1.5)), Self.cyan, width: 1.2, glow: 4)

            // ---- torso ----
            var torso = Path()
            torso.move(to: pt(148, 210)); torso.addLine(to: pt(192, 210))
            torso.addQuadCurve(to: pt(207, 224), control: pt(206, 210))
            torso.addLine(to: pt(196, 282))
            torso.addQuadCurve(to: pt(186, 293), control: pt(194, 293))
            torso.addLine(to: pt(154, 293))
            torso.addQuadCurve(to: pt(144, 282), control: pt(146, 293))
            torso.addLine(to: pt(133, 224))
            torso.addQuadCurve(to: pt(148, 210), control: pt(134, 210))
            b.fill(torso, with: .linearGradient(Gradient(colors: [Self.steelHi, Self.steel, Self.steelLo]), startPoint: pt(140, 210), endPoint: pt(200, 293)))
            b.stroke(torso, with: .color(Color.black.opacity(0.5)), lineWidth: 1.2)
            // chest plate lines
            var plate = Path()
            plate.move(to: pt(144, 232)); plate.addLine(to: pt(158, 240)); plate.addLine(to: pt(182, 240)); plate.addLine(to: pt(196, 232))
            plate.move(to: pt(152, 270)); plate.addLine(to: pt(188, 270))
            b.stroke(plate, with: .color(Color.black.opacity(0.45)), lineWidth: 1.2)
            // side trim
            var trim = Path(); trim.move(to: pt(139, 230)); trim.addLine(to: pt(147, 280))
            neon(&b, trim, Self.magenta, width: 1.2, glow: 4, alpha: 0.9)
            // core
            let pulse = happy ? 0.6 + 0.4 * sin(since * 11) : 0.55 + 0.2 * sin(t * 3)
            let core = Path(ellipseIn: CGRect(x: 162, y: 247, width: 16, height: 16))
            glowFill(&b, core, happy ? Self.yellow : Self.cyan, glow: 9, alpha: pulse)
            b.stroke(Path(ellipseIn: CGRect(x: 159, y: 244, width: 22, height: 22)), with: .color(Color.black.opacity(0.6)), lineWidth: 2)

            // ---- neck ----
            for k in 0..<3 {
                b.fill(Path(roundedRect: CGRect(x: 162, y: 197 + Double(k) * 4.5, width: 16, height: 3.5), cornerRadius: 1.5), with: .color(Self.steel))
            }

            // ---- head (turns toward the hand) ----
            b.drawLayer { hd in
                let look = min(max((h.x - 200) / 300, 0), 1)
                let nod = min(max((h.y - 190) / 500, -0.05), 0.10) + (happy ? sin(since * 11) * 0.035 : 0)
                hd.translateBy(x: 170, y: 198); hd.rotate(by: .radians(nod)); hd.translateBy(x: -170, y: -198)

                // antenna fin
                var fin = Path()
                fin.move(to: pt(205, 150)); fin.addLine(to: pt(222, 122)); fin.addLine(to: pt(226, 124)); fin.addLine(to: pt(213, 154)); fin.closeSubpath()
                hd.fill(fin, with: .color(Self.steel))
                let tipOn = Int(t * 3) % 2 == 0 || happy
                glowFill(&hd, Path(ellipseIn: CGRect(x: 220, y: 118, width: 7, height: 7)), Self.magenta, glow: 6, alpha: tipOn ? 1 : 0.25)

                // shell
                let shell = Path(roundedRect: CGRect(x: 124, y: 136, width: 92, height: 62), cornerRadius: 24)
                hd.fill(shell, with: .linearGradient(Gradient(colors: [Self.steelHi, Self.steel, Self.steelLo]), startPoint: pt(130, 136), endPoint: pt(205, 198)))
                hd.stroke(shell, with: .color(Color.black.opacity(0.5)), lineWidth: 1.2)
                // highlight
                var hi = Path(); hi.move(to: pt(140, 144)); hi.addQuadCurve(to: pt(196, 142), control: pt(168, 137))
                hd.stroke(hi, with: .color(Color.white.opacity(0.28)), style: StrokeStyle(lineWidth: 2, lineCap: .round))
                // ear
                joint(&hd, pt(126, 170), r: 7, color: Self.magenta)

                // visor
                let visor = Path(roundedRect: CGRect(x: 134, y: 153, width: 76, height: 30), cornerRadius: 13)
                hd.fill(visor, with: .linearGradient(Gradient(colors: [Color(red: 0.02, green: 0.03, blue: 0.08), Color(red: 0.05, green: 0.10, blue: 0.20)]),
                                                     startPoint: pt(0, 153), endPoint: pt(0, 183)))
                neon(&hd, visor, Self.cyan, width: 1.2, glow: 5, alpha: 0.9)
                // eyes
                let ex = 6 * look
                let blinkT = t.truncatingRemainder(dividingBy: 2.3)
                let open = (blinkT > 2.16 && !happy) ? 0.15 : 1.0
                let eyeColor = happy ? Self.yellow : Self.cyan
                for cx in [154.0 + ex, 184.0 + ex] {
                    if happy {
                        var arc = Path()
                        arc.addArc(center: pt(cx, 172), radius: 7.5, startAngle: .degrees(205), endAngle: .degrees(335), clockwise: false)
                        neon(&hd, arc, eyeColor, width: 2.6, glow: 6)
                    } else {
                        let eh = 11 * open
                        glowFill(&hd, Path(roundedRect: CGRect(x: cx - 6, y: 168 - eh / 2, width: 12, height: eh), cornerRadius: min(4, eh / 2)), eyeColor, glow: 6)
                    }
                }
                // scan line across the visor
                let scan = 155 + (t * 22).truncatingRemainder(dividingBy: 26)
                hd.drawLayer { l in
                    l.clip(to: visor)
                    l.fill(Path(CGRect(x: 134, y: scan, width: 76, height: 1.5)), with: .color(Self.cyan.opacity(0.35)))
                }
            }

            // ---- working arm: two bones, telescopic, solved from the hand position ----
            // Undo the body lean for the target so the hand lands exactly where the scene needs it.
            let c = cos(-lean), s = sin(-lean)
            let rel = pt(h.x - 170, h.y - 300)
            let target = pt(170 + rel.x * c - rel.y * s, 300 + rel.x * s + rel.y * c)
            let shoulder = pt(204, 226)
            let dx = target.x - shoulder.x, dy = target.y - shoulder.y
            let d = max(8, (dx * dx + dy * dy).squareRoot())
            let seg = max(46, d * 0.58) // each bone; grows with the reach
            let base = atan2(dy, dx)
            // `bend` +1: elbow above the line (carrying); −1: below it, so the arm reaches the keys
            // from underneath instead of crossing the cassette window.
            let alpha = acos(min(1, d / (2 * seg))) * bend
            let elbow = pt(shoulder.x + cos(base - alpha) * seg, shoulder.y + sin(base - alpha) * seg)
            capsule(&b, shoulder, elbow, width: 11, trim: Self.cyan)
            // forearm: sleeve + thinner piston
            let mid = pt(elbow.x + (target.x - elbow.x) * 0.5, elbow.y + (target.y - elbow.y) * 0.5)
            capsule(&b, elbow, mid, width: 9.5, trim: Self.cyan)
            capsule(&b, mid, target, width: 6, trim: Self.yellow)
            joint(&b, shoulder, r: 9, color: Self.cyan)
            joint(&b, elbow, r: 7, color: Self.cyan)
            // gripper
            let open = gripping ? 0.0 : 1.0
            for side in [-1.0, 1.0] {
                var finger = Path()
                finger.move(to: pt(target.x + side * 3, target.y - 2))
                finger.addLine(to: pt(target.x + side * (7 + open * 4), target.y - 10))
                finger.addLine(to: pt(target.x + side * (5 + open * 5), target.y - 17))
                b.stroke(finger, with: .color(Self.steelHi), style: StrokeStyle(lineWidth: 3.2, lineCap: .round, lineJoin: .round))
            }
            joint(&b, target, r: 6, color: happy ? Self.yellow : Self.cyan)
        }
    }

    // MARK: effects

    private func drawPressBurst(_ ctx: inout GraphicsContext, since: Double) {
        let c = Self.playButton
        for k in 0..<2 {
            let f = since * 1.6 - Double(k) * 0.18
            guard f > 0, f < 1 else { continue }
            let r = 10 + f * 150
            neon(&ctx, Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r * 0.5, width: r * 2, height: r)), k == 0 ? Self.cyan : Self.magenta,
                 width: 2.2 * (1 - f) + 0.4, glow: 8, alpha: (1 - f) * 0.9)
        }
        // notes rising from the speakers
        for i in 0..<8 {
            let born = Double(i) * 0.09
            let life = since - born
            guard life > 0, life < 1.1 else { continue }
            let side = i % 2 == 0 ? 379.0 : 581.0
            let x = side + sin(life * 5 + Double(i)) * 14 + (rnd(i) - 0.5) * 30
            let y = 176 - life * 86
            label(&ctx, i % 3 == 0 ? "♪" : "♫", at: pt(x, y), size: 13 + rnd(i * 3) * 7, color: i % 2 == 0 ? Self.cyan : Self.magenta, glow: 5,
                  alpha: min(1, (1.1 - life) * 2), weight: .regular, design: .default)
        }
    }

    private func drawScreenFx(_ ctx: inout GraphicsContext, since: Double, playing: Bool) {
        // flash and a short glitch right at the press
        if playing, since < 0.22 {
            let a = (1 - since / 0.22)
            ctx.fill(Path(CGRect(x: 0, y: 0, width: 640, height: 400)), with: .color(Self.cyan.opacity(0.20 * a)))
            for k in 0..<4 {
                let y = rnd(k + Int(since * 60) * 7) * 380
                ctx.fill(Path(CGRect(x: 0, y: y, width: 640, height: 3 + rnd(k * 5) * 9)), with: .color((k % 2 == 0 ? Self.magenta : Self.cyan).opacity(0.22 * a)))
            }
        }
        // scanlines
        var lines = Path()
        var y = 0.0
        while y < 400 { lines.addRect(CGRect(x: 0, y: y, width: 640, height: 1)); y += 3 }
        ctx.fill(lines, with: .color(Color.black.opacity(0.16)))
        // vignette
        ctx.fill(Path(CGRect(x: 0, y: 0, width: 640, height: 400)),
                 with: .radialGradient(Gradient(colors: [.clear, Color.black.opacity(0.55)]), center: pt(320, 210), startRadius: 210, endRadius: 430))
    }
}

/// Full-window overlay shown while a song file is being loaded.
struct TapeLoadingOverlay: View {
    let title: String
    let started: Date

    var body: some View {
        TimelineView(.animation) { tl in
            let p = min(max(tl.date.timeIntervalSince(started) / TapeLoadingScene.duration, 0), 1)
            let playing = p >= TapeLoadingScene.pressAt
            ZStack {
                Color(red: 0.02, green: 0.01, blue: 0.06).ignoresSafeArea()
                VStack(spacing: 20) {
                    TapeLoadingScene(progress: p, title: title)
                        .frame(maxWidth: 880, maxHeight: 550)
                        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Color(red: 0.2, green: 0.95, blue: 1).opacity(0.25)))
                    Text(playing ? "▶  PLAY" : "LOADING  《\(title)》")
                        .font(.system(size: 15, weight: .semibold, design: .monospaced)).tracking(4)
                        .foregroundStyle(playing ? Color(red: 1, green: 0.88, blue: 0.3) : Color(red: 0.2, green: 0.95, blue: 1))
                        .shadow(color: (playing ? Color(red: 1, green: 0.88, blue: 0.3) : Color(red: 0.2, green: 0.95, blue: 1)).opacity(0.8), radius: 8)
                }
                .padding(36)
            }
        }
    }
}
