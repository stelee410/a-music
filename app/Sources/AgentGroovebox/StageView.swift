import SwiftUI

/// The heart of the instrument made visible: every layer is a ring of
/// `lengthSteps` beads. Rings of different lengths drift against each other
/// and realign at their least common multiple — polymeter you can see.
struct OrbitView: View {
    @ObservedObject var session: Session
    /// Idle mode draws ghost rings (7 / 5 / 8) turning slowly.
    var ambient = false

    var body: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 60)) { tl in
            Canvas { ctx, size in
                let rings = ambient ? ambientRings : session.tracks.enumerated().flatMap { (i, t) in
                    t.layers.map { (Theme.color(i), $0) }
                }
                let tick: Double = ambient
                    ? tl.date.timeIntervalSinceReferenceDate * 520
                    : (session.playheadTick ?? 0)
                draw(ctx: &ctx, size: size, rings: rings, tick: tick)
            }
        }
    }

    private var ambientRings: [(Color, LayerInfo)] {
        [(Theme.color(0), LayerInfo(track: "a", layer: "7", length: 7, offset: 0, mute: false, noteSteps: [0, 3, 5])),
         (Theme.color(1), LayerInfo(track: "b", layer: "5", length: 5, offset: 0, mute: false, noteSteps: [0, 2])),
         (Theme.color(2), LayerInfo(track: "c", layer: "8", length: 8, offset: 0, mute: false, noteSteps: [0, 4, 6]))]
    }

    private func draw(ctx: inout GraphicsContext, size: CGSize, rings: [(Color, LayerInfo)], tick: Double) {
        let c = CGPoint(x: size.width / 2, y: size.height / 2)
        let outer = min(size.width, size.height) / 2 - 18
        let inner = outer * 0.36
        guard !rings.isEmpty else { return }
        let gap = rings.count > 1 ? (outer - inner) / CGFloat(rings.count - 1) : 0
        let tps = session.ticksPerStep
        let stepFrac = (tick / tps).truncatingRemainder(dividingBy: 1)

        for (k, (col, layer)) in rings.enumerated() {
            let r = rings.count == 1 ? (outer + inner) / 2 : outer - CGFloat(k) * gap
            let n = max(layer.length, 1)
            let alpha: Double = layer.mute ? 0.12 : 1
            ctx.stroke(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r)),
                       with: .color(col.opacity(0.10 * alpha)), lineWidth: 1)
            let cur = layer.step(atTick: tick, ticksPerStep: tps)
            let angle = { (i: Double) -> Double in -Double.pi / 2 + i / Double(n) * 2 * .pi }
            // comet trail behind the current bead
            if !layer.mute {
                let a1 = angle(Double(cur) + stepFrac)
                let a0 = a1 - 2 * .pi * min(0.35, 3.0 / Double(n))
                var trail = Path()
                trail.addArc(center: c, radius: r, startAngle: .radians(a0), endAngle: .radians(a1), clockwise: false)
                ctx.stroke(trail, with: .linearGradient(
                    Gradient(colors: [col.opacity(0), col.opacity(0.55)]),
                    startPoint: CGPoint(x: c.x + cos(a0) * r, y: c.y + sin(a0) * r),
                    endPoint: CGPoint(x: c.x + cos(a1) * r, y: c.y + sin(a1) * r)),
                    style: StrokeStyle(lineWidth: 3, lineCap: .round))
            }
            let bead = max(2.5, min(7, r * 2 * .pi / CGFloat(n) * 0.16))
            for i in 0..<n {
                let a = angle(Double(i))
                let p = CGPoint(x: c.x + cos(a) * r, y: c.y + sin(a) * r)
                let hasNote = layer.noteSteps.contains(i)
                let isCur = i == cur && !layer.mute
                var d = hasNote ? bead * 1.25 : bead * 0.55
                var o = (hasNote ? 0.75 : 0.22) * alpha
                if isCur {
                    let pulse = hasNote ? (1 - stepFrac) : 0.3 * (1 - stepFrac)
                    d *= 1 + 0.9 * pulse
                    o = 1
                    if hasNote {
                        ctx.drawLayer { l in
                            l.addFilter(.blur(radius: 10))
                            l.fill(Path(ellipseIn: CGRect(x: p.x - d * 1.6, y: p.y - d * 1.6, width: d * 3.2, height: d * 3.2)),
                                   with: .color(col.opacity(0.7 * pulse + 0.2)))
                        }
                    }
                }
                ctx.fill(Path(ellipseIn: CGRect(x: p.x - d / 2, y: p.y - d / 2, width: d, height: d)),
                         with: .color((isCur ? Color.white : col).opacity(o)))
                if i == 0 && !ambient {
                    ctx.stroke(Path { $0.move(to: CGPoint(x: c.x + cos(a) * (r - 6), y: c.y + sin(a) * (r - 6)))
                                      $0.addLine(to: CGPoint(x: c.x + cos(a) * (r + 6), y: c.y + sin(a) * (r + 6))) },
                               with: .color(col.opacity(0.35 * alpha)), lineWidth: 1)
                }
            }
        }
    }
}

/// Ring legend, outermost first.
struct OrbitLegend: View {
    @ObservedObject var session: Session
    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            if session.tracks.contains(where: { !$0.layers.isEmpty }) {
                Eyebrow("外圈 → 内圈", color: Theme.faint)
            }
            ForEach(Array(session.tracks.enumerated()), id: \.element.id) { i, t in
                ForEach(t.layers) { l in
                    HStack(spacing: 7) {
                        Circle().fill(Theme.color(i)).frame(width: 6, height: 6)
                        Text(t.id).foregroundStyle(Theme.color(i).opacity(l.mute ? 0.35 : 0.9))
                        Text(l.layer).foregroundStyle(Theme.dim.opacity(l.mute ? 0.4 : 1))
                        Text("\(l.length) 步").foregroundStyle(Theme.faint)
                        if l.mute { Text("静音").foregroundStyle(Theme.faint) }
                    }
                    .font(.system(size: 10, weight: .medium, design: .rounded))
                }
            }
        }
    }
}

/// A slow river of notes flowing past the playhead.
struct ScoreRiver: View {
    @ObservedObject var session: Session
    private let past = 3.0
    private let future = 7.0

    var body: some View {
        HStack(spacing: 10) {
            VStack(alignment: .trailing, spacing: 0) {
                ForEach(Array(session.tracks.enumerated()), id: \.element.id) { i, tr in
                    Text(tr.id.uppercased()).font(Theme.label(9)).tracking(1.5)
                        .foregroundStyle(tr.mute ? Theme.faint : Theme.color(i).opacity(0.85))
                        .lineLimit(1)
                        .frame(maxHeight: .infinity)
                }
            }
            .frame(width: 70, alignment: .trailing)
            river
        }
    }

    private var river: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 60)) { _ in
            Canvas { ctx, size in
                let tracks = session.tracks
                guard !tracks.isEmpty else { return }
                let tps = session.tempo * session.ppqn / 60
                let now = session.playheadTick ?? 0
                let t0 = now - past * tps, t1 = now + future * tps
                let labelW: CGFloat = 0
                let w = size.width - labelW
                let px = { (t: Double) -> CGFloat in labelW + CGFloat((t - t0) / (t1 - t0)) * w }
                let rowH = size.height / CGFloat(tracks.count)

                let bt = Double(session.barTicks)
                var b = (t0 / bt).rounded(.down) * bt
                while b <= t1 {
                    let x = px(b)
                    if x > labelW {
                        ctx.fill(Path(CGRect(x: x, y: 0, width: 0.5, height: size.height)), with: .color(Theme.hairline))
                        ctx.draw(Text("\(Int(b / bt) + 1)").font(.system(size: 9, design: .monospaced)).foregroundStyle(Theme.faint),
                                 at: CGPoint(x: x + 4, y: 2), anchor: .topLeading)
                    }
                    b += bt
                }
                for ph in session.phrases where Double(ph.tick) > t0 && Double(ph.tick) < t1 {
                    let x = px(Double(ph.tick))
                    ctx.fill(Path(CGRect(x: x - 0.5, y: 0, width: 1, height: size.height)), with: .color(Theme.accent.opacity(0.5)))
                    ctx.draw(Text(ph.title).font(Theme.voice(11)).foregroundStyle(Theme.accent),
                             at: CGPoint(x: x + 5, y: size.height - 3), anchor: .bottomLeading)
                }
                for (i, tr) in tracks.enumerated() {
                    let col = Theme.color(i)
                    let y0 = CGFloat(i) * rowH
                    let mine = session.events.filter { $0.track == tr.id }
                    let lo = mine.map(\.pitch).min() ?? 36, hi = max(mine.map(\.pitch).max() ?? 84, lo + 1)
                    for e in mine where Double(e.tick + e.dur) >= t0 && Double(e.tick) <= t1 {
                        let x0 = max(px(Double(e.tick)), labelW), x1 = px(Double(e.tick + e.dur))
                        guard x1 > labelW else { continue }
                        let h: CGFloat = 3
                        let y = y0 + 4 + (1 - CGFloat(e.pitch - lo) / CGFloat(hi - lo)) * (rowH - 8 - h)
                        let rect = CGRect(x: x0, y: y, width: max(2, x1 - x0 - 1), height: h)
                        let sounding = Double(e.tick) <= now && now < Double(e.tick + e.dur)
                        let isPast = Double(e.tick + e.dur) < now
                        if sounding {
                            ctx.drawLayer { l in
                                l.addFilter(.blur(radius: 5))
                                l.fill(Path(roundedRect: rect.insetBy(dx: -2, dy: -3), cornerRadius: 3), with: .color(col.opacity(0.9)))
                            }
                        }
                        let o = sounding ? 1.0 : (isPast ? 0.18 : 0.30 + 0.45 * e.velocity)
                        ctx.fill(Path(roundedRect: rect, cornerRadius: 1.5), with: .color((sounding ? Color.white : col).opacity(o)))
                    }
                }
                let x = px(now)
                ctx.fill(Path(CGRect(x: x - 0.5, y: 0, width: 1, height: size.height)),
                         with: .linearGradient(Gradient(colors: [.white.opacity(0), .white.opacity(0.9), .white.opacity(0)]),
                                               startPoint: CGPoint(x: x, y: 0), endPoint: CGPoint(x: x, y: size.height)))
            }
        }
        .mask(LinearGradient(stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.06),
                                     .init(color: .black, location: 0.9), .init(color: .clear, location: 1)],
                             startPoint: .leading, endPoint: .trailing))
    }
}
