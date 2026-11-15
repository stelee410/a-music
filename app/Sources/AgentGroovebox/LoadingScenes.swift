import SwiftUI

/// What is shown while a song file is being loaded. There are several scenes, none of them carries
/// any text: the picture is the whole message. Each is a pure function of `progress` (0…1), so it can
/// be rendered frame by frame, and each has one moment — `LoadingScene.pressAt` — at which the music
/// starts: the star ignites, the ship arrives, the flower opens, the gate lets the light through.
enum LoadingStyle: String, CaseIterable, Identifiable {
    /// A nebula falls inward and a star ignites.
    case stellar
    /// Stars stretch into a jump; the ship arrives over a world.
    case warp
    /// Seeds appear on a golden-angle spiral and the flower opens.
    case bloom
    /// A flight down a tunnel of light frames towards an iris that opens.
    case gate

    var id: String { rawValue }
    /// Name in the settings (the scene itself shows no text).
    var name: String {
        switch self {
        case .stellar: return "恒星"
        case .warp: return "跃迁"
        case .bloom: return "生长"
        case .gate: return "星门"
        }
    }

    /// UserDefaults key: a style's raw value, or "random".
    static let storageKey = "loadingStyle"

    /// The style for a load that began at `started`: the stored one, or a different one each time.
    static func resolve(_ stored: String, started: Date) -> LoadingStyle {
        if let s = LoadingStyle(rawValue: stored) { return s }
        let n = Int((started.timeIntervalSinceReferenceDate * 7).rounded())
        return allCases[abs(n) % allCases.count]
    }
}

struct LoadingScene: View {
    let progress: Double
    let style: LoadingStyle
    /// From the song's title: every song gets its own colours and its own arrangement of particles.
    let seed: UInt64

    static let duration: Double = 4.0
    /// The moment the music starts.
    static let pressAt: Double = 0.84

    static func seed(_ title: String) -> UInt64 {
        var h: UInt64 = 5381
        for u in title.unicodeScalars { h = (h &* 33) ^ UInt64(u.value) }
        return h
    }

    var body: some View {
        Canvas(rendersAsynchronously: false) { ctx, size in
            switch style {
            case .stellar: stellar(&ctx, size)
            case .warp: warp(&ctx, size)
            case .bloom: bloom(&ctx, size)
            case .gate: gate(&ctx, size)
            }
        }
        .background(Color.black)
    }

    // MARK: time

    /// 0…1 up to the moment.
    private var q: Double { min(1, max(0, progress / Self.pressAt)) }
    /// 0…1 after the moment (0 before it).
    private var after: Double { min(1, max(0, (progress - Self.pressAt) / (1 - Self.pressAt))) }
    private var seconds: Double { progress * Self.duration }

    // MARK: helpers

    private func ease(_ x: Double) -> Double {
        let t = min(max(x, 0), 1)
        return t * t * t * (t * (t * 6 - 15) + 10)
    }
    /// Ease with a small overshoot, for things that open.
    private func back(_ x: Double) -> Double {
        let t = min(max(x, 0), 1) - 1
        return 1 + t * t * (2.2 * t + 1.2)
    }
    private func rnd(_ i: Int) -> Double {
        let x = sin(Double(i) * 12.9898 + Double(seed % 10007) * 0.731 + 4.1414) * 43758.5453
        return x - floor(x)
    }
    private var hue: Double { Double(seed % 360) / 360 }
    private func hsb(_ h: Double, _ s: Double, _ b: Double, _ a: Double = 1) -> Color {
        Color(hue: h - floor(h), saturation: s, brightness: b, opacity: a)
    }
    private func dot(_ p: inout Path, _ x: Double, _ y: Double, _ r: Double) {
        p.addEllipse(in: CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2))
    }
    /// Soft light: a disc that fades to nothing at `radius`.
    private func glow(_ ctx: inout GraphicsContext, _ c: CGPoint, _ radius: Double, _ color: Color, _ alpha: Double) {
        guard radius > 0.5, alpha > 0.003 else { return }
        ctx.fill(Path(ellipseIn: CGRect(x: c.x - radius, y: c.y - radius, width: radius * 2, height: radius * 2)),
                 with: .radialGradient(Gradient(colors: [color.opacity(alpha), color.opacity(alpha * 0.32), color.opacity(0)]),
                                       center: c, startRadius: 0, endRadius: radius))
    }
    private func backdrop(_ ctx: inout GraphicsContext, _ s: CGSize, _ inner: Color, centre: CGPoint? = nil) {
        let c = centre ?? CGPoint(x: s.width / 2, y: s.height / 2)
        ctx.fill(Path(CGRect(origin: .zero, size: s)),
                 with: .radialGradient(Gradient(colors: [inner, .black]), center: c, startRadius: 0, endRadius: max(s.width, s.height) * 0.72))
    }
    /// Distant stars that do not move.
    private func farStars(_ ctx: inout GraphicsContext, _ s: CGSize, count: Int, alpha: Double) {
        var dim = Path(), bright = Path()
        for i in 0..<count {
            let x = rnd(i + 11000) * s.width, y = rnd(i + 12000) * s.height
            let r = 0.4 + rnd(i + 13000) * 0.9
            if i % 5 == 0 { dot(&bright, x, y, r * 1.2) } else { dot(&dim, x, y, r) }
        }
        ctx.fill(dim, with: .color(Color.white.opacity(0.35 * alpha)))
        ctx.fill(bright, with: .color(Color.white.opacity(0.8 * alpha)))
    }
    private func flash(_ ctx: inout GraphicsContext, _ s: CGSize, _ color: Color, strength: Double, length: Double = 0.3) {
        guard progress >= Self.pressAt, after < length else { return }
        ctx.fill(Path(CGRect(origin: .zero, size: s)), with: .color(color.opacity(strength * (1 - after / length))))
    }

    // MARK: stellar — a nebula falls inward and a star ignites

    private func stellar(_ ctx: inout GraphicsContext, _ s: CGSize) {
        let c = CGPoint(x: s.width / 2, y: s.height / 2), u = min(s.width, s.height)
        backdrop(&ctx, s, hsb(hue + 0.62, 0.75, 0.11))
        farStars(&ctx, s, count: 240, alpha: 0.8)
        ctx.blendMode = .plusLighter
        // the star's colour is its class: blue-white, yellow, orange, red
        let starHue = [0.60, 0.14, 0.08, 0.03][Int(rnd(900) * 4) % 4]
        let lit = progress >= Self.pressAt
        let k = ease(q)

        // the clouds of the nebula, drawn in as the gas falls
        if !lit {
            for j in 0..<7 {
                let r = (0.16 + 0.34 * rnd(j + 300)) * (1 - 0.85 * k)
                let ang = rnd(j + 310) * 2 * .pi + seconds * 0.10 + k * 1.6
                let at = CGPoint(x: c.x + cos(ang) * r * u, y: c.y + sin(ang) * r * u * 0.8)
                glow(&ctx, at, u * (0.30 + 0.22 * rnd(j + 320)) * (1 - 0.6 * k),
                     hsb(j % 2 == 0 ? hue : hue + 0.5, 0.75, 1), 0.20 * (1 - 0.5 * k))
            }
        }
        var dust = [Path(), Path(), Path()]
        for i in 0..<960 {
            let r0 = 0.10 + 0.74 * sqrt(rnd(i))
            let a0 = rnd(i + 2000) * 2 * .pi
            var r: Double, a: Double
            if !lit {
                let fall = pow(k, 1.0 + 1.7 * rnd(i + 4000))            // each grain falls in its own time
                r = r0 * (1 - fall) + 0.010 * rnd(i + 5)
                a = a0 + 0.5 + 2.8 * fall / (r0 * 2.0 + 0.25) + seconds * 0.12   // faster as it nears the centre
            } else {
                r = (0.04 + 1.15 * rnd(i + 7000)) * (1 - pow(1 - after, 3))      // blown out by the ignition
                a = a0 + 3.3
            }
            let rad = (0.7 + 2.0 * rnd(i + 3000)) * u / 560 * (lit ? 1 - after * 0.55 : 1)
            dot(&dust[i % 3], c.x + cos(a) * r * u, c.y + sin(a) * r * u * 0.80, rad)
        }
        let fade = lit ? 1 - after : 1
        ctx.fill(dust[0], with: .color(hsb(hue, 0.55, 1, 0.85 * fade)))
        ctx.fill(dust[1], with: .color(hsb(hue + 0.5, 0.5, 1, 0.8 * fade)))
        ctx.fill(dust[2], with: .color(hsb(starHue, 0.3, 1, 0.9 * fade)))

        if !lit {
            // the core heats up as the gas gathers
            let heat = pow(k, 3)
            glow(&ctx, c, u * (0.05 + 0.22 * heat), hsb(starHue, 0.55, 1), 0.15 + 0.75 * heat)
            glow(&ctx, c, u * 0.022 * (0.3 + heat), .white, heat)
        } else {
            let open = back(min(1, after * 2.2))
            let breathe = 1 + 0.06 * sin(seconds * 9)
            glow(&ctx, c, u * 0.62 * open, hsb(starHue, 0.6, 1), 0.55)
            glow(&ctx, c, u * 0.20 * open * breathe, hsb(starHue, 0.25, 1), 0.95)
            // rays
            for j in 0..<8 {
                let ang = Double(j) * .pi / 4 + 0.2
                let len = u * (j % 2 == 0 ? 0.62 : 0.30) * open
                let w = u * (j % 2 == 0 ? 0.010 : 0.006)
                var ray = Path()
                ray.move(to: CGPoint(x: c.x + cos(ang + .pi / 2) * w, y: c.y + sin(ang + .pi / 2) * w))
                ray.addLine(to: CGPoint(x: c.x + cos(ang) * len, y: c.y + sin(ang) * len))
                ray.addLine(to: CGPoint(x: c.x - cos(ang + .pi / 2) * w, y: c.y - sin(ang + .pi / 2) * w))
                ray.closeSubpath()
                ctx.fill(ray, with: .linearGradient(Gradient(colors: [Color.white.opacity(0.85), hsb(starHue, 0.5, 1, 0)]),
                                                    startPoint: c, endPoint: CGPoint(x: c.x + cos(ang) * len, y: c.y + sin(ang) * len)))
            }
            // shock fronts
            for j in 0..<2 {
                let e = max(0, after - Double(j) * 0.16)
                guard e > 0 else { continue }
                let rr = u * (0.06 + 0.95 * (1 - pow(1 - e, 2.5)))
                ctx.stroke(Path(ellipseIn: CGRect(x: c.x - rr, y: c.y - rr * 0.80, width: rr * 2, height: rr * 1.60)),
                           with: .color(hsb(starHue, 0.35, 1, 0.7 * (1 - e))), lineWidth: u / 300)
            }
            var disc = Path(); dot(&disc, c.x, c.y, u * 0.034 * open * breathe)
            ctx.fill(disc, with: .color(.white))
            flash(&ctx, s, hsb(starHue, 0.2, 1), strength: 0.6)
        }
    }

    // MARK: warp — the jump, and the arrival over a world

    private func warp(_ ctx: inout GraphicsContext, _ s: CGSize) {
        let c = CGPoint(x: s.width / 2, y: s.height * 0.47), u = min(s.width, s.height)
        backdrop(&ctx, s, hsb(hue + 0.6, 0.8, 0.09), centre: c)
        ctx.blendMode = .plusLighter
        let arrived = progress >= Self.pressAt
        // distance flown and speed (in depths of the field)
        let travel = arrived ? 2.75 + 1.0 * (1 - exp(-after * 8)) : 0.15 * q + 2.6 * q * q * q
        let speed = arrived ? 7.95 * exp(-after * 8) : 0.15 + 7.8 * q * q

        var near = [Path(), Path(), Path()], tint = Path()
        for i in 0..<540 {
            let x = (rnd(i) * 2 - 1) * 1.25, y = (rnd(i + 1000) * 2 - 1) * 1.25
            let f = rnd(i + 2000) - travel
            let z = 0.035 + 0.965 * (f - floor(f))
            let z2 = min(1.0, z + speed * 0.030)                              // the tail is where the star was
            let hx = c.x + x / z * u * 0.20, hy = c.y + y / z * u * 0.20
            guard hx > -60, hx < s.width + 60, hy > -60, hy < s.height + 60 else { continue }
            let tx = c.x + x / z2 * u * 0.20, ty = c.y + y / z2 * u * 0.20
            if i % 4 == 0 {
                tint.move(to: CGPoint(x: tx, y: ty)); tint.addLine(to: CGPoint(x: hx, y: hy))
            } else {
                let b = z < 0.25 ? 2 : z < 0.55 ? 1 : 0
                near[b].move(to: CGPoint(x: tx, y: ty)); near[b].addLine(to: CGPoint(x: hx, y: hy))
            }
        }
        let w = u / 760
        ctx.stroke(near[0], with: .color(Color.white.opacity(0.35)), style: StrokeStyle(lineWidth: w * 1.0, lineCap: .round))
        ctx.stroke(near[1], with: .color(Color.white.opacity(0.65)), style: StrokeStyle(lineWidth: w * 1.6, lineCap: .round))
        ctx.stroke(near[2], with: .color(Color.white.opacity(0.95)), style: StrokeStyle(lineWidth: w * 2.4, lineCap: .round))
        ctx.stroke(tint, with: .color(hsb(hue, 0.6, 1, 0.9)), style: StrokeStyle(lineWidth: w * 1.8, lineCap: .round))

        if !arrived {
            let k = ease(q)
            glow(&ctx, c, u * (0.06 + 0.42 * k * k), hsb(hue, 0.5, 1), 0.10 + 0.6 * k * k)
            glow(&ctx, c, u * 0.05 * k, .white, k)
        } else {
            // a world rises under the ship, its air lit from behind
            let rise = ease(after * 1.25)
            let R = u * 1.7
            let pc = CGPoint(x: c.x - u * 0.12, y: s.height + R - u * 0.30 * rise)
            let air = R + u * 0.075
            ctx.fill(Path(ellipseIn: CGRect(x: pc.x - air, y: pc.y - air, width: air * 2, height: air * 2)),
                     with: .radialGradient(Gradient(stops: [.init(color: hsb(hue, 0.55, 1, 0), location: 0),
                                                            .init(color: hsb(hue, 0.55, 1, 0.9 * rise), location: R / air),
                                                            .init(color: hsb(hue + 0.08, 0.6, 1, 0), location: 1)]),
                                           center: pc, startRadius: 0, endRadius: air))
            ctx.blendMode = .normal
            ctx.fill(Path(ellipseIn: CGRect(x: pc.x - R, y: pc.y - R, width: R * 2, height: R * 2)),
                     with: .radialGradient(Gradient(colors: [.black, hsb(hue + 0.6, 0.7, 0.05)]), center: pc, startRadius: R * 0.9, endRadius: R))
            ctx.blendMode = .plusLighter
            ctx.stroke(Path(ellipseIn: CGRect(x: pc.x - R, y: pc.y - R, width: R * 2, height: R * 2)),
                       with: .color(Color.white.opacity(0.85 * rise)), lineWidth: u / 420)
            // its sun, just clearing the limb
            let sun = CGPoint(x: pc.x + R * sin(0.16), y: pc.y - R * cos(0.16) - u * 0.004)
            glow(&ctx, sun, u * 0.30 * rise, hsb(hue + 0.08, 0.35, 1), 0.8)
            glow(&ctx, sun, u * 0.05 * rise, .white, 1)
            flash(&ctx, s, .white, strength: 0.75, length: 0.35)
        }
    }

    // MARK: bloom — seeds on the golden angle, then the flower opens

    private func bloom(_ ctx: inout GraphicsContext, _ s: CGSize) {
        let c = CGPoint(x: s.width / 2, y: s.height / 2), u = min(s.width, s.height)
        let shift = (hue - 0.5) * 0.14                                         // every song a slightly different plant
        backdrop(&ctx, s, hsb(0.46 + shift, 0.85, 0.12))
        ctx.blendMode = .plusLighter
        let open = progress >= Self.pressAt
        let n = 377, R = u * 0.34
        let shown = open ? Double(n) + 20 : Double(n) * pow(ease(q), 0.85)
        let turn = seconds * 0.10 + rnd(1) * 6

        // spores adrift
        var spores = Path()
        for i in 0..<80 {
            let x = (rnd(i + 500) + 0.03 * sin(seconds * (0.4 + rnd(i)) + Double(i))) * s.width
            let y = (rnd(i + 600) - seconds * 0.012 * (0.5 + rnd(i + 700))) * s.height
            dot(&spores, x, y - floor(y / s.height) * s.height, (0.6 + rnd(i + 800) * 1.4) * u / 700)
        }
        ctx.fill(spores, with: .color(hsb(0.30 + shift, 0.5, 1, 0.45)))

        if open {
            // two rings of petals, 21 and 13, as the seed head would have them
            let unfold = back(min(1, after * 1.5))
            for (count, len, wid, h, a, off) in [(21, 0.24, 0.034, 0.52 + shift, 0.42, 0.0), (13, 0.15, 0.040, 0.13 + shift, 0.5, 0.12)] {
                var petals = Path()
                for j in 0..<count {
                    let ang = Double(j) / Double(count) * 2 * .pi + turn + off
                    let base = R * 0.98, tip = base + u * len * unfold
                    let dx = cos(ang), dy = sin(ang), nx = -dy, ny = dx, w = u * wid * unfold
                    let p0 = CGPoint(x: c.x + dx * base, y: c.y + dy * base)
                    let p1 = CGPoint(x: c.x + dx * tip, y: c.y + dy * tip)
                    let mid = base + (tip - base) * 0.45
                    petals.move(to: p0)
                    petals.addQuadCurve(to: p1, control: CGPoint(x: c.x + dx * mid + nx * w, y: c.y + dy * mid + ny * w))
                    petals.addQuadCurve(to: p0, control: CGPoint(x: c.x + dx * mid - nx * w, y: c.y + dy * mid - ny * w))
                }
                ctx.fill(petals, with: .color(hsb(h, 0.55, 1, a)))
                ctx.stroke(petals, with: .color(hsb(h, 0.3, 1, 0.7)), lineWidth: u / 900)
            }
            let rr = R + u * 0.7 * (1 - pow(1 - after, 2.2))
            ctx.stroke(Path(ellipseIn: CGRect(x: c.x - rr, y: c.y - rr, width: rr * 2, height: rr * 2)),
                       with: .color(hsb(0.14 + shift, 0.4, 1, 0.5 * (1 - after))), lineWidth: u / 320)
        }

        // the seed head
        var rings = [Path(), Path(), Path(), Path(), Path()], fresh = Path()
        for i in 0..<n where Double(i) < shown {
            let f = (Double(i) + 0.5) / Double(n)
            let ang = Double(i) * 2.399963229728653 + turn
            let r = R * sqrt(f)
            let age = shown - Double(i)
            let size = u / 250 * (0.55 + 1.5 * sqrt(f)) * back(min(1, age / 14))
            let x = c.x + cos(ang) * r, y = c.y + sin(ang) * r
            dot(&rings[min(4, Int(f * 5))], x, y, size)
            if age < 14, !open { dot(&fresh, x, y, size * 1.5) }
        }
        for (b, h) in [0.11, 0.17, 0.27, 0.38, 0.48].enumerated() {
            ctx.fill(rings[b], with: .color(hsb(h + shift, 0.65, 1, 0.9)))
        }
        ctx.fill(fresh, with: .color(Color.white.opacity(0.8)))
        glow(&ctx, c, R * (0.5 + 0.9 * ease(q)), hsb(0.13 + shift, 0.7, 1), 0.10 + 0.25 * ease(q))
        if open {
            glow(&ctx, c, R * 2.3 * back(min(1, after * 1.6)), hsb(0.13 + shift, 0.5, 1), 0.35)
            flash(&ctx, s, hsb(0.13 + shift, 0.35, 1), strength: 0.4)
        }
    }

    // MARK: gate — down a tunnel of light frames to an iris that opens

    private func gate(_ ctx: inout GraphicsContext, _ s: CGSize) {
        let c = CGPoint(x: s.width / 2, y: s.height / 2), u = min(s.width, s.height)
        backdrop(&ctx, s, hsb(hue + 0.55, 0.8, 0.07))
        farStars(&ctx, s, count: 160, alpha: 0.6)                              // open space outside the frames
        ctx.blendMode = .plusLighter
        let through = progress >= Self.pressAt
        let travel = through ? 1.9 + 2.2 * after : 0.30 * q + 1.6 * q * q * q
        let a = hsb(hue, 0.8, 1), b = hsb(hue + 0.42, 0.8, 1)
        let far = u * 0.105
        func corner(_ j: Int, _ zz: Double) -> CGPoint {
            let ang = Double(j) * .pi / 3 + zz * 1.1 + 0.4
            return CGPoint(x: c.x + cos(ang) * far / zz, y: c.y + sin(ang) * far / zz)
        }

        // the rails along the tunnel
        var rails = Path()
        for j in 0..<6 {
            rails.move(to: corner(j, 1))
            var zz = 0.94
            while zz > 0.05 { rails.addLine(to: corner(j, zz)); zz -= 0.06 }
        }
        ctx.stroke(rails, with: .color(a.opacity(0.45)), lineWidth: u / 500)

        // the frames, nearest last
        let count = 14
        var order: [(Double, Int)] = []
        for k in 0..<count {
            let f = Double(k) / Double(count) - travel
            order.append((f - floor(f), k))
        }
        for (z, k) in order.sorted(by: { $0.0 > $1.0 }) {
            let zz = 0.055 + 0.945 * z
            var hex = Path()
            hex.move(to: corner(0, zz))
            for j in 1..<6 { hex.addLine(to: corner(j, zz)) }
            hex.closeSubpath()
            let alpha = pow(1 - z, 0.7) * min(1, (zz - 0.055) / 0.05) * (0.35 + 0.65 * min(1, z * 8))
            let col = k % 2 == 0 ? a : b
            let w = min(u / 40, u / 520 / zz)
            ctx.stroke(hex, with: .color(col.opacity(0.22 * alpha)), style: StrokeStyle(lineWidth: w * 4.5, lineJoin: .round))
            ctx.stroke(hex, with: .color(col.opacity(0.95 * alpha)), style: StrokeStyle(lineWidth: w, lineJoin: .round))
            ctx.stroke(hex, with: .color(Color.white.opacity(0.6 * alpha)), style: StrokeStyle(lineWidth: w * 0.35, lineJoin: .round))
        }

        // the iris at the far end
        let aperture = through ? 1.0 : ease((q - 0.25) / 0.75)
        let ir = far * 0.92
        if !through {
            // light spills through as the blades draw back
            for j in 0..<12 {
                let ang = Double(j) * .pi / 6 + seconds * 0.25
                let len = u * 0.75 * aperture
                var beam = Path()
                beam.move(to: c)
                beam.addLine(to: CGPoint(x: c.x + cos(ang - 0.035) * len, y: c.y + sin(ang - 0.035) * len))
                beam.addLine(to: CGPoint(x: c.x + cos(ang + 0.035) * len, y: c.y + sin(ang + 0.035) * len))
                beam.closeSubpath()
                ctx.fill(beam, with: .linearGradient(Gradient(colors: [Color.white.opacity(0.20 * aperture), Color.white.opacity(0)]),
                                                     startPoint: c, endPoint: CGPoint(x: c.x + cos(ang) * len, y: c.y + sin(ang) * len)))
            }
            glow(&ctx, c, ir * (1.2 + 2.6 * aperture), a, 0.25 + 0.5 * aperture)
            var hole = Path()
            let hr = ir * max(0.05, aperture)
            for j in 0..<6 {
                let ang = Double(j) * .pi / 3 + aperture * 1.0
                let p = CGPoint(x: c.x + cos(ang) * hr, y: c.y + sin(ang) * hr)
                if j == 0 { hole.move(to: p) } else { hole.addLine(to: p) }
            }
            hole.closeSubpath()
            ctx.fill(hole, with: .color(Color.white.opacity(0.35 + 0.65 * aperture)))
            // the blades' edges
            var blades = Path()
            for j in 0..<6 {
                let ang = Double(j) * .pi / 3 + aperture * 1.0
                blades.move(to: CGPoint(x: c.x + cos(ang) * hr, y: c.y + sin(ang) * hr))
                blades.addLine(to: CGPoint(x: c.x + cos(ang + 1.25) * ir, y: c.y + sin(ang + 1.25) * ir))
            }
            ctx.stroke(blades, with: .color(b.opacity(0.7)), lineWidth: u / 700)
            ctx.stroke(Path(ellipseIn: CGRect(x: c.x - ir, y: c.y - ir, width: ir * 2, height: ir * 2)), with: .color(b.opacity(0.8)), lineWidth: u / 500)
        } else {
            // through: the light takes the whole view
            let e = ease(after * 1.15)
            glow(&ctx, c, u * (0.55 + 1.4 * e), a, 0.9 * (1 - 0.6 * after))
            glow(&ctx, c, u * (0.30 + 0.8 * e), .white, 1 - 0.75 * after * after)
            flash(&ctx, s, hsb(hue, 0.25, 1), strength: 0.55)
        }
    }
}

/// Full-window overlay shown while a song file is being loaded. No text.
struct LoadingOverlay: View {
    let title: String
    let started: Date
    @AppStorage(LoadingStyle.storageKey) private var stored = "random"

    var body: some View {
        let style = LoadingStyle.resolve(stored, started: started)
        TimelineView(.animation) { tl in
            let p = min(max(tl.date.timeIntervalSince(started) / LoadingScene.duration, 0), 1)
            LoadingScene(progress: p, style: style, seed: LoadingScene.seed(title))
        }
        .ignoresSafeArea()
    }
}
