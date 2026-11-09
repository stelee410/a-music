import SwiftUI
import UniformTypeIdentifiers

// MARK: - Primitive controls

/// Rotary knob: drag up/down (Shift = fine), double-click to reset.
struct Knob: View {
    let label: String
    @Binding var value: Double // 0…1
    var defaultValue: Double = 0.5
    var color: Color = Theme.accent
    var size: CGFloat = 42
    var display: (Double) -> String = { "\(Int($0 * 100))" }
    @State private var start: Double?

    var body: some View {
        VStack(spacing: 5) {
            ZStack {
                Circle().trim(from: 0, to: 0.75)
                    .stroke(Color.white.opacity(0.08), style: StrokeStyle(lineWidth: 3, lineCap: .round))
                    .rotationEffect(.degrees(135))
                Circle().trim(from: 0, to: 0.75 * value)
                    .stroke(color, style: StrokeStyle(lineWidth: 3, lineCap: .round))
                    .rotationEffect(.degrees(135))
                    .shadow(color: color.opacity(0.6), radius: 4)
                Circle().fill(Color.white.opacity(0.05)).padding(7)
                Rectangle().fill(Theme.text).frame(width: 2, height: size * 0.22)
                    .offset(y: -size * 0.22)
                    .rotationEffect(.degrees(-135 + 270 * value))
            }
            .frame(width: size, height: size)
            .contentShape(Circle())
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { g in
                    if start == nil { start = value }
                    let fine = NSEvent.modifierFlags.contains(.shift) ? 0.2 : 1
                    value = min(1, max(0, start! - Double(g.translation.height) / 160 * fine))
                }
                .onEnded { _ in start = nil })
            .onTapGesture(count: 2) { value = defaultValue }
            Text(display(value)).font(.system(size: 10, weight: .medium, design: .monospaced)).foregroundStyle(Theme.text.opacity(0.85))
            Text(label).font(Theme.label(9)).tracking(1).foregroundStyle(Theme.faint)
        }
    }
}

/// Vertical fader.
struct Fader: View {
    @Binding var value: Double
    var color: Color
    var height: CGFloat = 92

    var body: some View {
        GeometryReader { g in
            let h = g.size.height
            ZStack(alignment: .bottom) {
                Capsule().fill(Color.white.opacity(0.07)).frame(width: 4)
                Capsule().fill(color.opacity(0.85)).frame(width: 4, height: max(0, h * value))
                    .shadow(color: color.opacity(0.7), radius: 4)
                RoundedRectangle(cornerRadius: 3).fill(Theme.text)
                    .frame(width: 18, height: 7)
                    .offset(y: -(h - 7) * value)
            }
            .frame(maxWidth: .infinity)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { v in
                value = min(1, max(0, 1 - Double(v.location.y / h)))
            })
            .onTapGesture(count: 2) { value = 0.85 }
        }
        .frame(width: 26, height: height)
    }
}

/// Small round on/off switch.
struct PowerToggle: View {
    @Binding var isOn: Bool
    var color: Color = Theme.accent
    var body: some View {
        Button { isOn.toggle() } label: {
            Image(systemName: "power")
                .font(.system(size: 10, weight: .bold))
                .foregroundStyle(isOn ? Theme.ink : Theme.dim)
                .frame(width: 22, height: 22)
                .background(isOn ? color : Color.white.opacity(0.07), in: Circle())
                .shadow(color: isOn ? color.opacity(0.8) : .clear, radius: 6)
        }
        .buttonStyle(.plain)
    }
}

/// Card used for each console section.
struct ConsoleCard<Content: View>: View {
    let title: String
    var accessory: AnyView? = nil
    let content: Content
    init(_ title: String, accessory: AnyView? = nil, @ViewBuilder content: () -> Content) {
        self.title = title
        self.accessory = accessory
        self.content = content()
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Eyebrow(title, color: Theme.dim)
                Spacer()
                accessory
            }
            content
        }
        .padding(14)
        .frame(maxHeight: .infinity, alignment: .top)
        .glass(16)
    }
}

// MARK: - Console

struct ConsoleView: View {
    @ObservedObject var console: Console
    @ObservedObject var session: Session

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 12) {
                mixer
                masterFX
                pads
                healing
            }
            .padding(.horizontal, 2)
        }
        .frame(height: 228)
    }

    // AI band mixer
    private var mixer: some View {
        ConsoleCard("乐队调音台") {
            if session.tracks.isEmpty {
                Text("AI 的乐器入场后\n会出现在这里").font(Theme.voice(12)).foregroundStyle(Theme.faint)
                    .frame(width: 140, height: 130)
            } else {
                HStack(alignment: .bottom, spacing: 8) {
                    ForEach(Array(session.tracks.enumerated()), id: \.element.id) { i, t in
                        let col = Theme.color(i)
                        let muted = console.mutes.contains(t.id)
                        VStack(spacing: 6) {
                            Fader(value: Binding(get: { console.volume(t.id) }, set: { console.setVolume(t.id, $0) }),
                                  color: muted ? Theme.faint : col)
                            Button { console.toggleMute(t.id) } label: {
                                Text("M").font(Theme.label(9))
                                    .foregroundStyle(muted ? Theme.ink : Theme.dim)
                                    .frame(width: 20, height: 16)
                                    .background(muted ? Theme.color(1) : Color.white.opacity(0.07), in: RoundedRectangle(cornerRadius: 4))
                            }
                            .buttonStyle(.plain)
                            .help(muted ? "取消静音" : "静音这条 AI 轨道")
                            Text(t.id).font(Theme.label(9)).foregroundStyle(col.opacity(muted ? 0.4 : 0.9)).lineLimit(1)
                                .frame(width: 44)
                        }
                    }
                }
            }
        }
    }

    // master FX + tempo
    private var masterFX: some View {
        ConsoleCard("主效果", accessory: AnyView(recordButton)) {
            HStack(alignment: .top, spacing: 14) {
                Knob(label: "滤波", value: $console.filter, defaultValue: 1, color: Theme.color(4)) { v in
                    v >= 0.995 ? "全开" : "\(Int(200 * pow(100, v))) Hz"
                }
                Knob(label: "混响", value: $console.reverb, defaultValue: 0.12, color: Theme.color(3))
                Knob(label: "延迟", value: $console.delay, defaultValue: 0, color: Theme.color(2))
                Knob(label: "调音 A4", value: Binding(get: { (console.tuningA4 - 426) / 28 },
                                                     set: { console.tuningA4 = (426 + $0 * 28).rounded() }),
                     defaultValue: 0.5, color: Theme.accent) { _ in "\(Int(console.tuningA4)) Hz" }
                tempoControl
            }
            HStack(spacing: 8) {
                Button("A = 432") { console.tuningA4 = 432 }.buttonStyle(.plain)
                Button("A = 440") { console.tuningA4 = 440 }.buttonStyle(.plain)
                Spacer()
                Image(systemName: "speaker.wave.2").foregroundStyle(Theme.faint)
                Slider(value: $console.master, in: 0...1).frame(width: 80).controlSize(.mini)
            }
            .font(.system(size: 10, design: .rounded)).foregroundStyle(Theme.dim)
        }
    }

    private var tempoControl: some View {
        let bpm = console.bpmTarget > 0 ? console.bpmTarget : session.currentBpm.rounded()
        return Knob(label: "BPM", value: Binding(get: { (bpm - 40) / 160 },
                                                 set: { console.requestTempo((40 + $0 * 160).rounded()) }),
                    defaultValue: (session.tempo - 40) / 160, color: Theme.color(0)) { _ in "\(Int(bpm))" }
            .opacity(session.isRunning ? 1 : 0.35)
            .disabled(!session.isRunning)
            .help("拖动改变速度：乐队会在下一小节跟上")
    }

    private var recordButton: some View {
        Button { console.toggleRecording() } label: {
            HStack(spacing: 4) {
                Circle().fill(console.recording ? Theme.color(0) : Theme.dim).frame(width: 7, height: 7)
                    .shadow(color: console.recording ? Theme.color(0) : .clear, radius: 4)
                Text(console.recording ? "录音中" : "录音").font(Theme.label(9))
            }
            .foregroundStyle(console.recording ? Theme.color(0) : Theme.dim)
        }
        .buttonStyle(.plain)
        .help("录下你听到的全部声音（乐队 + 打击垫 + 疗愈声）")
    }

    // pads
    private var pads: some View {
        ConsoleCard("打击垫", accessory: AnyView(
            Toggle("对齐节拍", isOn: $console.quantize).toggleStyle(.switch).controlSize(.mini)
                .font(Theme.label(9)).foregroundStyle(Theme.dim)
                .help("开启后，敲击会推迟到下一个 16 分音符，与乐队严丝合缝")
        )) {
            HStack(spacing: 4) {
                ForEach(Array(PadBank.banks.enumerated()), id: \.offset) { i, b in
                    Button { console.bank = i } label: {
                        HStack(spacing: 3) {
                            Text("\(i + 1)").font(.system(size: 8, design: .monospaced)).foregroundStyle(Theme.faint)
                            Text(b.title).font(.system(size: 10, weight: .medium, design: .rounded))
                        }
                        .foregroundStyle(console.bank == i ? Theme.ink : Theme.dim)
                        .padding(.horizontal, 7).padding(.vertical, 3)
                        .background(console.bank == i ? Theme.color(i + 2) : Color.white.opacity(0.06), in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
            }
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(56), spacing: 6), count: 4), spacing: 6) {
                ForEach(console.pads) { pad in
                    PadView(pad: pad, color: Theme.color(console.bank + 2), selected: pad.id == console.selected) { console.hit(pad.id) }
                        .onDrop(of: [.fileURL], isTargeted: nil) { items in
                            guard let item = items.first else { return false }
                            _ = item.loadObject(ofClass: URL.self) { url, _ in
                                if let url { Task { @MainActor in console.assign(pad.id, file: url) } }
                            }
                            return true
                        }
                }
            }
            padEditor
        }
    }

    private var padEditor: some View {
        let s = console.selectedPad?.settings ?? PadSettings()
        let col = Theme.color(console.bank + 2)
        return HStack(alignment: .top, spacing: 9) {
            VStack(alignment: .leading, spacing: 2) {
                Text(console.selectedPad?.name ?? "—").font(.system(size: 11, weight: .medium, design: .serif))
                    .foregroundStyle(Theme.text).lineLimit(1)
                Button("复原") { console.resetSelected() }.buttonStyle(.plain)
                    .font(.system(size: 9, design: .rounded)).foregroundStyle(Theme.faint)
            }
            .frame(width: 44, alignment: .leading)
            Knob(label: "音高", value: Binding(get: { (s.pitch + 12) / 24 }, set: { v in console.updateSelected { $0.pitch = (v * 24 - 12).rounded() } }),
                 defaultValue: 0.5, color: col, size: 26) { _ in s.pitch == 0 ? "0" : String(format: "%+.0f", s.pitch) }
            Knob(label: "衰减", value: Binding(get: { s.decay }, set: { v in console.updateSelected { $0.decay = v } }),
                 defaultValue: 1, color: col, size: 26) { v in v >= 0.999 ? "自然" : "\(Int(v * 100))%" }
            Knob(label: "音色", value: Binding(get: { s.tone }, set: { v in console.updateSelected { $0.tone = v } }),
                 defaultValue: 1, color: col, size: 26) { v in v >= 0.999 ? "明亮" : "\(Int(v * 100))" }
            Knob(label: "音量", value: Binding(get: { s.level }, set: { v in console.updateSelected { $0.level = v } }),
                 defaultValue: 0.8, color: col, size: 26)
            Knob(label: "空间", value: Binding(get: { console.space[console.bank] }, set: { console.space[console.bank] = $0 }),
                 defaultValue: PadBank.banks[console.bank].space, color: Theme.color(3), size: 26)
        }
    }

    // healing
    private var healing: some View {
        ConsoleCard("疗愈声场", accessory: AnyView(
            Text("放松辅助 · 非医疗用途").font(.system(size: 9, design: .rounded)).foregroundStyle(Theme.faint)
        )) {
            HStack(alignment: .top, spacing: 16) {
                // tone generator
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 6) {
                        PowerToggle(isOn: $console.toneOn, color: Theme.color(1))
                        Text("频率发生器").font(.system(size: 11, weight: .medium, design: .rounded)).foregroundStyle(Theme.text)
                    }
                    HStack(spacing: 10) {
                        Knob(label: "频率", value: Binding(get: { log(console.toneHz / 40) / log(25) },
                                                           set: { console.toneHz = (40 * pow(25, $0)).rounded() }),
                             defaultValue: log(432.0 / 40) / log(25), color: Theme.color(1), size: 38) { _ in "\(Int(console.toneHz))" }
                        Knob(label: "音量", value: $console.toneLevel, defaultValue: 0.35, color: Theme.color(1), size: 38)
                    }
                    Menu("Solfeggio") {
                        ForEach(Healing.solfeggio, id: \.0) { hz, name in
                            Button(name) { console.toneHz = hz; console.toneOn = true }
                        }
                    }
                    .menuStyle(.borderlessButton).fixedSize().font(.system(size: 10))
                }
                // binaural
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 6) {
                        PowerToggle(isOn: $console.binOn, color: Theme.color(3))
                        Text("双耳节拍").font(.system(size: 11, weight: .medium, design: .rounded)).foregroundStyle(Theme.text)
                        Image(systemName: "headphones").font(.system(size: 10)).foregroundStyle(Theme.faint).help("需要佩戴耳机")
                    }
                    HStack(spacing: 10) {
                        Knob(label: "载波", value: Binding(get: { (console.carrierHz - 100) / 400 },
                                                           set: { console.carrierHz = (100 + $0 * 400).rounded() }),
                             defaultValue: 0.25, color: Theme.color(3), size: 38) { _ in "\(Int(console.carrierHz))" }
                        Knob(label: Healing.bandName(console.beatHz), value: Binding(get: { log(console.beatHz / 0.5) / log(80) },
                                                                                     set: { console.beatHz = (0.5 * pow(80, $0) * 10).rounded() / 10 }),
                             defaultValue: log(12) / log(80), color: Theme.color(3), size: 38) { _ in String(format: "%.1f Hz", console.beatHz) }
                        Knob(label: "音量", value: $console.binLevel, defaultValue: 0.35, color: Theme.color(3), size: 38)
                    }
                    HStack(spacing: 4) {
                        ForEach(Healing.bands, id: \.0) { sym, name, hz in
                            Button(sym) { console.beatHz = hz; console.binOn = true }
                                .buttonStyle(.plain).help(name)
                                .font(.system(size: 11, design: .serif))
                                .foregroundStyle(abs(console.beatHz - hz) < 0.01 ? Theme.color(3) : Theme.dim)
                                .frame(width: 20, height: 18)
                                .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 4))
                        }
                    }
                }
                // noise
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 6) {
                        PowerToggle(isOn: $console.noiseOn, color: Theme.color(2))
                        Text("噪声").font(.system(size: 11, weight: .medium, design: .rounded)).foregroundStyle(Theme.text)
                    }
                    Knob(label: "音量", value: $console.noiseLevel, defaultValue: 0.25, color: Theme.color(2), size: 38)
                    Picker("", selection: $console.noiseColor) {
                        Text("白").tag(0); Text("粉").tag(1); Text("褐").tag(2)
                    }
                    .pickerStyle(.segmented).labelsHidden().frame(width: 96).controlSize(.mini)
                }
            }
        }
    }
}

struct PadView: View {
    let pad: Pad
    let color: Color
    var selected = false
    let action: () -> Void
    @State private var down = false

    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 60)) { tl in
            let age = tl.date.timeIntervalSince(pad.flash)
            let glow = max(0, 1 - age / 0.3)
            VStack(spacing: 1) {
                Text(pad.name).font(.system(size: 10.5, weight: .medium, design: .rounded)).lineLimit(1).minimumScaleFactor(0.7)
                    .foregroundStyle(glow > 0.15 ? Theme.ink : Theme.text.opacity(pad.url == nil ? 0.3 : 0.9))
                Text(pad.key.uppercased()).font(.system(size: 8, design: .monospaced))
                    .foregroundStyle(glow > 0.15 ? Theme.ink.opacity(0.7) : Theme.faint)
            }
            .frame(width: 56, height: 38)
            .background(RoundedRectangle(cornerRadius: 9, style: .continuous).fill(color.opacity(0.10 + 0.85 * glow)))
            .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous)
                .strokeBorder(selected ? color : color.opacity(0.3), lineWidth: selected ? 1.5 : 1))
            .shadow(color: color.opacity(0.8 * glow), radius: 10)
            .scaleEffect(down ? 0.94 : 1)
        }
        .contentShape(Rectangle())
        // Fire on mouse-DOWN (a Button would wait for mouse-up).
        .gesture(DragGesture(minimumDistance: 0)
            .onChanged { _ in
                if !down { down = true; if pad.url != nil { action() } }
            }
            .onEnded { _ in down = false })
        .help(pad.url == nil ? "正在生成音色…" : "点击或按 \(pad.key.uppercased())；拖入音频文件替换")
    }
}

// MARK: - Chat

/// Conversation with the band: listener messages, AI replies and phrase changes.
struct ChatPanel: View {
    @ObservedObject var session: Session
    @State private var text = ""
    @FocusState private var focused: Bool
    private let quick = ["再安静一点", "更有能量", "来段旋律独奏", "加点雨声的感觉", "慢下来，我想冥想"]

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ScrollViewReader { proxy in
                ScrollView(showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 10) {
                        if session.feed.isEmpty {
                            Text("演奏开始后，你可以随时对乐手说话。\n它会回答你，并在下一小节用音乐回应。")
                                .font(Theme.voice(13)).foregroundStyle(Theme.faint).lineSpacing(4)
                        }
                        ForEach(session.feed) { item in row(item).id(item.id) }
                        if session.aiThinkingAboutListener {
                            HStack(spacing: 8) { ThinkingDots(); Text("乐手在想…").font(Theme.voice(12)).foregroundStyle(Theme.dim) }
                                .id("thinking")
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .onChange(of: session.feed.count) { _, _ in
                    withAnimation { proxy.scrollTo(session.aiThinkingAboutListener ? AnyHashable("thinking") : AnyHashable(session.feed.last?.id), anchor: .bottom) }
                }
            }
            if session.isRunning {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        ForEach(quick, id: \.self) { q in
                            Button { session.say(q) } label: {
                                Text(q).font(.system(size: 10, design: .serif).italic()).foregroundStyle(Theme.dim)
                                    .padding(.horizontal, 8).padding(.vertical, 4)
                                    .overlay(Capsule().strokeBorder(Theme.hairline))
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
            HStack(spacing: 8) {
                TextField("对乐手说点什么…", text: $text)
                    .textFieldStyle(.plain)
                    .font(Theme.display(14))
                    .focused($focused)
                    .onSubmit(send)
                Button(action: send) {
                    Image(systemName: "arrow.up").font(.system(size: 11, weight: .bold)).foregroundStyle(Theme.ink)
                        .frame(width: 24, height: 24)
                        .background(text.isEmpty ? Theme.faint : Theme.accent, in: Circle())
                }
                .buttonStyle(.plain)
                .disabled(text.isEmpty || !session.isRunning)
            }
            .padding(.horizontal, 12).padding(.vertical, 9)
            .glass(14)
            .disabled(!session.isRunning)
        }
    }

    private func send() {
        session.say(text)
        text = ""
    }

    @ViewBuilder private func row(_ item: FeedItem) -> some View {
        switch item.kind {
        case .listener:
            HStack {
                Spacer(minLength: 40)
                Text(item.text).font(.system(size: 13, design: .rounded)).foregroundStyle(Theme.ink)
                    .padding(.horizontal, 12).padding(.vertical, 8)
                    .background(Theme.accent, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
        case .ai:
            HStack(alignment: .top, spacing: 8) {
                Circle().fill(Theme.color(3)).frame(width: 6, height: 6).padding(.top, 7)
                Text(item.text).font(Theme.voice(14)).foregroundStyle(Theme.text).lineSpacing(3)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 30)
            }
        case .phrase:
            VStack(alignment: .leading, spacing: 2) {
                Text("♪ \(item.text)").font(.system(size: 12, weight: .medium, design: .serif)).foregroundStyle(Theme.accent)
                Text(item.detail).font(.system(size: 11, design: .serif)).foregroundStyle(Theme.dim).lineLimit(2)
            }
        case .event:
            HStack(spacing: 6) {
                Rectangle().fill(Theme.hairline).frame(width: 14, height: 1)
                Text(item.text).font(Theme.label(10)).foregroundStyle(Theme.dim)
                if !item.detail.isEmpty { Text(item.detail).font(Theme.label(10)).foregroundStyle(Theme.faint) }
            }
        }
    }
}
