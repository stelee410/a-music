import SwiftUI

/// Visual language: a night-time instrument. Deep ink background, one neon
/// hue per track, serif type for the AI's voice, thin mono numerals for time.
enum Theme {
    static let ink = Color(red: 0.035, green: 0.031, blue: 0.067)
    static let inkHigh = Color(red: 0.094, green: 0.071, blue: 0.180)
    static let text = Color(white: 0.94)
    static let dim = Color(white: 0.62)
    static let faint = Color(white: 0.40)
    static let hairline = Color.white.opacity(0.08)
    static let accent = Color(red: 1.00, green: 0.71, blue: 0.40) // warm amber

    static let palette: [Color] = [
        Color(red: 1.00, green: 0.42, blue: 0.45), // coral
        Color(red: 1.00, green: 0.72, blue: 0.30), // amber
        Color(red: 0.31, green: 0.86, blue: 0.78), // mint
        Color(red: 0.70, green: 0.56, blue: 1.00), // lilac
        Color(red: 0.38, green: 0.70, blue: 1.00), // sky
        Color(red: 1.00, green: 0.48, blue: 0.78), // rose
        Color(red: 0.68, green: 0.92, blue: 0.40), // lime
        Color(red: 1.00, green: 0.60, blue: 0.48), // peach
    ]

    static func color(_ i: Int) -> Color { palette[((i % palette.count) + palette.count) % palette.count] }

    // Typography
    static func display(_ size: CGFloat) -> Font { .system(size: size, weight: .regular, design: .serif) }
    static func voice(_ size: CGFloat) -> Font { .system(size: size, weight: .light, design: .serif).italic() }
    static func numerals(_ size: CGFloat) -> Font { .system(size: size, weight: .ultraLight, design: .monospaced) }
    static func label(_ size: CGFloat = 10) -> Font { .system(size: size, weight: .semibold, design: .rounded) }
}

/// Small-caps style eyebrow label.
struct Eyebrow: View {
    let text: String
    var color: Color = Theme.dim
    init(_ text: String, color: Color = Theme.dim) { self.text = text; self.color = color }
    var body: some View {
        Text(text.uppercased()).font(Theme.label(10)).tracking(2.2).foregroundStyle(color)
    }
}

/// Ink background with a soft aurora and film grain (static, drawn once).
struct InkBackground: View {
    var body: some View {
        ZStack {
            Theme.ink
            RadialGradient(colors: [Theme.inkHigh.opacity(0.95), .clear], center: .init(x: 0.28, y: 0.30),
                           startRadius: 0, endRadius: 720)
            RadialGradient(colors: [Color(red: 0.30, green: 0.08, blue: 0.22).opacity(0.45), .clear],
                           center: .init(x: 0.85, y: 0.95), startRadius: 0, endRadius: 620)
            Grain().opacity(0.5).allowsHitTesting(false)
        }
        .ignoresSafeArea()
    }
}

private struct Grain: View {
    var body: some View {
        Canvas(rendersAsynchronously: true) { ctx, size in
            var s: UInt64 = 0x9E3779B97F4A7C15
            func rnd() -> CGFloat {
                s ^= s << 13; s ^= s >> 7; s ^= s << 17
                return CGFloat(s % 10_000) / 10_000
            }
            let n = Int(size.width * size.height / 900)
            for _ in 0..<n {
                let r = CGRect(x: rnd() * size.width, y: rnd() * size.height, width: 1, height: 1)
                ctx.fill(Path(r), with: .color(.white.opacity(0.025 + 0.05 * rnd())))
            }
        }
    }
}

/// Dark glass panel.
struct Glass: ViewModifier {
    var radius: CGFloat = 18
    func body(content: Content) -> some View {
        content
            .background(.ultraThinMaterial.opacity(0.55), in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .background(Color.black.opacity(0.25), in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(Theme.hairline))
    }
}

extension View {
    func glass(_ radius: CGFloat = 18) -> some View { modifier(Glass(radius: radius)) }
}

/// Pill-shaped option chip.
struct Chip<Content: View>: View {
    let content: Content
    init(@ViewBuilder _ c: () -> Content) { content = c() }
    var body: some View {
        content
            .font(.system(size: 12, weight: .medium, design: .rounded))
            .foregroundStyle(Theme.text.opacity(0.85))
            .padding(.horizontal, 12).padding(.vertical, 7)
            .background(Color.white.opacity(0.06), in: Capsule())
            .overlay(Capsule().strokeBorder(Theme.hairline))
    }
}

/// Animated "thinking" dots.
struct ThinkingDots: View {
    var color: Color = Theme.accent
    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 30)) { tl in
            let t = tl.date.timeIntervalSinceReferenceDate
            HStack(spacing: 4) {
                ForEach(0..<3) { i in
                    Circle().fill(color)
                        .frame(width: 5, height: 5)
                        .opacity(0.25 + 0.75 * max(0, sin(t * 3.2 - Double(i) * 0.7)))
                }
            }
        }
    }
}

/// Round play / stop button with a glow.
struct GlowButton: View {
    let systemImage: String
    var size: CGFloat = 72
    var color: Color = Theme.accent
    let action: () -> Void
    @State private var hover = false
    var body: some View {
        Button(action: action) {
            ZStack {
                Circle().fill(color.opacity(hover ? 0.45 : 0.30)).blur(radius: 18)
                Circle().fill(LinearGradient(colors: [color, color.opacity(0.75)], startPoint: .top, endPoint: .bottom))
                Image(systemName: systemImage)
                    .font(.system(size: size * 0.34, weight: .bold))
                    .foregroundStyle(Theme.ink)
                    .offset(x: systemImage.hasPrefix("play") ? size * 0.03 : 0)
            }
            .frame(width: size, height: size)
            .scaleEffect(hover ? 1.04 : 1)
        }
        .buttonStyle(.plain)
        .onHover { h in withAnimation(.easeOut(duration: 0.18)) { hover = h } }
    }
}

/// Quiet text button.
struct GhostButton: View {
    let title: String
    let systemImage: String
    let action: () -> Void
    @State private var hover = false
    var body: some View {
        Button(action: action) {
            Label(title, systemImage: systemImage)
                .font(.system(size: 12, weight: .medium, design: .rounded))
                .lineLimit(1).fixedSize()
                .foregroundStyle(hover ? Theme.text : Theme.dim)
                .padding(.horizontal, 12).padding(.vertical, 7)
                .background(Color.white.opacity(hover ? 0.10 : 0.05), in: Capsule())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
    }
}
