import AppKit
import AVFoundation
import SwiftUI

// MARK: - Root

/// Owns the session (AI performance) and the console (human controls).
@MainActor
final class Studio: ObservableObject {
    let session = Session()
    lazy var console = Console(session: session)
    lazy var playlist = Playlist(session: session)
}

struct ContentView: View {
    @StateObject private var studio = Studio()
    @StateObject private var library = Library()
    @State private var opts = PerformOptions(brief: "")
    @State private var showShelf = false
    @State private var showSettings = false

    var body: some View {
        Root(session: studio.session, console: studio.console, playlist: studio.playlist, library: library, opts: $opts,
             showShelf: $showShelf, showSettings: $showSettings)
            .onAppear {
                library.refresh(); selfTest(); openFromArguments()
                if let url = SongOpener.takePending() { studio.session.open(song: url) }
            }
            .onReceive(NotificationCenter.default.publisher(for: SongOpener.notification)) { _ in
                if let url = SongOpener.takePending() { studio.session.open(song: url) }
            }
    }

    /// `open -a "Agent Groovebox" --args /path/to/song.agbsong` plays that song.
    private func openFromArguments() {
        guard let path = CommandLine.arguments.dropFirst().first(where: { $0.hasSuffix(".agbsong") }) else { return }
        // AGB_SONG_MOCK: offline performer, so a test run does not ask for the keychain.
        studio.session.open(song: URL(fileURLWithPath: path), mock: ProcessInfo.processInfo.environment["AGB_SONG_MOCK"] != nil)
    }

    /// Hidden self-test: AGB_AUTOSTART=mock|ai|idle starts a short performance;
    /// AGB_SELFTEST_OUT=<file> + AGB_SELFTEST_SECONDS=<n> dumps state as JSON and quits.
    /// AGB_SELFTEST_SAY="<text>@<seconds>" sends a chat message; AGB_SELFTEST_HEAL=1 turns on binaural + tone.
    private func selfTest() {
        let env = ProcessInfo.processInfo.environment
        guard let mode = env["AGB_AUTOSTART"] else { return }
        let session = studio.session, console = studio.console
        var o = opts
        o.mock = mode == "mock"
        o.phrases = Int(env["AGB_AUTOSTART_PHRASES"] ?? "") ?? 3
        o.bars = Int(env["AGB_AUTOSTART_BARS"] ?? "") ?? 4
        o.brief = env["AGB_AUTOSTART_BRIEF"] ?? "深夜城市里的数学摇滚电子乐"
        if mode != "idle" { session.start(o) } else { opts.brief = o.brief }
        if env["AGB_SELFTEST_HEAL"] != nil {
            console.binOn = true; console.beatHz = 6; console.toneOn = true; console.toneHz = 528
        }
        if let say = env["AGB_SELFTEST_SAY"], let at = say.split(separator: "@").last.flatMap({ Double($0) }) {
            let text = String(say.split(separator: "@").first ?? "")
            Task { @MainActor in try? await Task.sleep(for: .seconds(at)); session.say(text) }
        }
        if let pads = env["AGB_SELFTEST_PADS"], let at = Double(pads) {
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(at))
                for i in [0, 4, 1, 5] { console.hit(i); try? await Task.sleep(for: .milliseconds(400)) }
            }
        }
        guard let out = env["AGB_SELFTEST_OUT"] else { return }
        let secs = Double(env["AGB_SELFTEST_SECONDS"] ?? "") ?? 20
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(secs))
            let state: [String: Any] = [
                "status": "\(session.status)",
                "phrases": session.phrases.map { ["title": $0.title, "commentary": $0.commentary, "tick": $0.tick, "lateBars": $0.lateBars, "answer": $0.answer] },
                "feed": session.feed.map { "\($0.kind): \($0.text) \($0.detail)" },
                "tracks": session.tracks.map { ["id": $0.id, "layers": $0.layers.map { "\($0.layer):\($0.length)" }] },
                "events": session.events.count,
                "playheadTick": session.playheadTick ?? -1,
                "pads": console.pads.map { "\($0.name):\($0.url != nil)" },
                "humanState": console.summary(),
                "log": session.log,
            ]
            if let d = try? JSONSerialization.data(withJSONObject: state, options: [.prettyPrinted]) {
                try? d.write(to: URL(fileURLWithPath: out))
            }
            NSApp.terminate(nil)
        }
    }
}

struct Root: View {
    @ObservedObject var session: Session
    @ObservedObject var console: Console
    @ObservedObject var playlist: Playlist
    @ObservedObject var library: Library
    @Binding var opts: PerformOptions
    @Binding var showShelf: Bool
    @Binding var showSettings: Bool

    private var onStage: Bool {
        switch session.status {
        case .idle, .stopped: return false
        case .failed: return !session.tracks.isEmpty
        default: return true
        }
    }

    var body: some View {
        ZStack {
            InkBackground()
            if onStage {
                StageScreen(session: session, console: console, playlist: playlist,
                            onNew: { session.stop(final: false); session.status = .idle },
                            onShelf: { showShelf = true }, onSettings: { showSettings = true },
                            onContinue: { more in session.continueCreating(opts, more: more) })
                    .transition(.opacity)
            } else {
                IdleScreen(session: session, console: console, playlist: playlist, opts: $opts,
                           onShelf: { showShelf = true }, onSettings: { showSettings = true })
                    .transition(.opacity)
            }
        }
        .overlay {
            if let l = session.loadingSong {
                LoadingOverlay(title: l.title, started: l.started).transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.5), value: session.loadingSong?.started)
        .animation(.easeInOut(duration: 0.6), value: onStage)
        .preferredColorScheme(.dark)
        .sheet(isPresented: $showShelf) {
            ShelfView(library: library, onContinue: { item, more in
                showShelf = false
                var o = opts; o.phrases = more; o.brief = item.brief
                session.start(o, continuing: item.url)
            })
            .frame(width: 780, height: 560)
        }
        .sheet(isPresented: $showSettings) { SettingsView().frame(width: 520) }
        .onChange(of: session.status) { _, s in if s == .finished || s == .stopped { library.refresh() } }
    }
}

// MARK: - Idle

struct IdleScreen: View {
    @ObservedObject var session: Session
    @ObservedObject var console: Console
    @ObservedObject var playlist: Playlist
    @Binding var opts: PerformOptions
    let onShelf: () -> Void
    let onSettings: () -> Void
    @FocusState private var focused: Bool
    @State private var showConsole = false

    private let suggestions = [
        "雨夜窗边的 lo-fi 嘻哈，慵懒的鼓和电钢琴",
        "深夜城市的数学摇滚电子乐，7 拍琶音缠绕厚重贝斯",
        "柏林地下室的极简 techno，酸性贝斯慢慢打开",
        "海边日出的冥想音乐，缓慢呼吸的长音",
    ]

    var body: some View {
        ZStack {
            OrbitView(session: session, ambient: true)
                .frame(maxWidth: 780, maxHeight: 780)
                .padding(40)
                .opacity(0.20)
            VStack(spacing: 22) {
                Spacer()
                Eyebrow("Agent Groovebox", color: Theme.accent)
                Text("今晚，想听 AI 演奏什么？")
                    .font(Theme.display(44))
                    .foregroundStyle(Theme.text)
                ZStack(alignment: .topLeading) {
                    if opts.brief.isEmpty {
                        Text("描述一种情绪、一个场景，或一种风格…")
                            .font(Theme.voice(20)).foregroundStyle(Theme.faint)
                            .padding(.horizontal, 5).padding(.vertical, 1)
                            .allowsHitTesting(false)
                    }
                    TextEditor(text: $opts.brief)
                        .font(Theme.display(20))
                        .foregroundStyle(Theme.text)
                        .scrollContentBackground(.hidden)
                        .focused($focused)
                        .frame(height: 60)
                }
                .padding(.horizontal, 18).padding(.vertical, 14)
                .frame(width: 640)
                .glass(20)
                HStack(spacing: 8) {
                    ForEach(suggestions, id: \.self) { s in
                        Button { opts.brief = s } label: {
                            Text(s).lineLimit(1).font(.system(size: 11, design: .serif).italic())
                                .foregroundStyle(Theme.dim)
                                .padding(.horizontal, 10).padding(.vertical, 5)
                                .overlay(Capsule().strokeBorder(Theme.hairline))
                                .contentShape(Capsule())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .frame(maxWidth: 820)
                HStack(spacing: 10) {
                    Chip { Stepper(value: $opts.phrases, in: 2...32) { Text("\(opts.phrases) 个乐句") } }
                    Chip { Stepper(value: $opts.bars, in: 2...16) { Text("每句约 \(opts.bars) 小节") } }
                    Chip {
                        Picker("", selection: $opts.realtime) {
                            Text("现场 · 边想边演").tag(true)
                            Text("录音室 · 先写后放").tag(false)
                        }
                        .pickerStyle(.menu).labelsHidden().fixedSize()
                    }
                    Chip { Toggle("离线演示", isOn: $opts.mock).toggleStyle(.switch).controlSize(.mini) }
                }
                GlowButton(systemImage: "play.fill") { session.start(opts) }
                    .keyboardShortcut(.return, modifiers: .command)
                    .padding(.top, 6)
                if case .failed(let m) = session.status {
                    Text(m).font(.system(size: 12)).foregroundStyle(Theme.color(0)).frame(maxWidth: 600)
                }
                Spacer()
                if showConsole {
                    ConsoleView(console: console, session: session)
                        .padding(.horizontal, 24)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                }
                HStack {
                    GhostButton(title: "唱片架", systemImage: "square.stack", action: onShelf)
                    GhostButton(title: "打开 .agbsong", systemImage: "doc.badge.arrow.up") { openSong() }
                    PlaylistControls(playlist: playlist, session: session, transport: false)
                    GhostButton(title: showConsole ? "收起控制台" : "控制台 · 疗愈声场 · 打击垫", systemImage: "dial.medium") {
                        withAnimation(.easeInOut(duration: 0.35)) { showConsole.toggle() }
                    }
                    Spacer()
                    Text("⌘↩ 开始演奏").font(.system(size: 11, design: .rounded)).foregroundStyle(Theme.faint)
                    Spacer()
                    GhostButton(title: "设置", systemImage: "slider.horizontal.3", action: onSettings)
                }
                .padding(.horizontal, 24).padding(.bottom, 18)
            }
        }
        .onAppear { focused = true }
    }

    /// An `.agbsong` is a zip file or a directory with the same layout.
    private func openSong() {
        if let url = SongOpener.choose() { session.open(song: url) }
    }
}

// MARK: - Stage

struct StageScreen: View {
    @ObservedObject var session: Session
    @ObservedObject var console: Console
    var playlist: Playlist?
    let onNew: () -> Void
    let onShelf: () -> Void
    let onSettings: () -> Void
    let onContinue: (Int) -> Void
    @State private var showLog = false
    @State private var showConsole = true
    @State private var stageMode: StageMode = .orbit
    /// The video fills the window, with the chat box below and the band's words overlaid.
    @State private var theater = false

    var body: some View {
        Group {
            if theater, let video = session.videoURL {
                VideoTheater(session: session, video: video, playlist: playlist, onExit: { theater = false })
                    .transition(.opacity)
            } else {
                stage.transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.3), value: theater)
        .onChange(of: session.videoURL) { _, v in if v == nil { theater = false } }
        .onAppear { if ProcessInfo.processInfo.environment["AGB_STAGE_THEATER"] != nil { theater = true } }
    }

    private var stage: some View {
        VStack(spacing: 0) {
            topBar
            HStack(spacing: 28) {
                ZStack {
                    switch activeMode {
                    case .cover:
                        if let cover = session.coverURL { CoverView(url: cover).padding(.top, 40).padding(.bottom, 8).transition(.opacity) }
                    case .video:
                        if let video = session.videoURL {
                            SongVideoView(url: video, followSong: session.videoFollowsSong, songSeconds: { [weak session] in session?.videoSeconds }, songRate: { [weak session] in session?.videoRate ?? 1 })
                                .id(video)
                                .aspectRatio(session.videoAspect, contentMode: .fit)
                                .padding(.top, 40).padding(.bottom, 8).transition(.opacity)
                        }
                    case .orbit:
                        ZStack {
                            OrbitView(session: session, ambient: session.tracks.isEmpty)
                                .opacity(session.tracks.isEmpty ? 0.28 : 1)
                                .animation(.easeInOut(duration: 1.2), value: session.tracks.isEmpty)
                            clock
                        }
                        .aspectRatio(1, contentMode: .fit)
                        .transition(.opacity)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .overlay(alignment: .topLeading) { if activeMode == .orbit { OrbitLegend(session: session).padding(.top, 4) } }
                .overlay(alignment: .top) {
                    if stageModes.count > 1 { StageModePicker(modes: stageModes, mode: $stageMode) }
                }
                .overlay(alignment: .bottomLeading) { if activeMode != .orbit { miniClock } }
                .overlay(alignment: .topTrailing) {
                    if activeMode == .video {
                        GhostButton(title: "全屏", systemImage: "arrow.up.left.and.arrow.down.right") { theater = true }
                            .help("视频全屏：下方可以和乐手对话，右侧叠加乐手的回答和演出文字")
                    }
                }
                .onChange(of: session.coverURL) { _, _ in stageMode = defaultMode }
                .onChange(of: session.videoURL) { _, _ in stageMode = defaultMode }
                .onAppear {
                    // AGB_STAGE_MODE=cover|orbit|video picks the view for self-tests.
                    let forced = ProcessInfo.processInfo.environment["AGB_STAGE_MODE"]
                    stageMode = ["cover": .cover, "orbit": .orbit, "video": .video][forced ?? ""] ?? defaultMode
                }
                VStack(alignment: .leading, spacing: 14) {
                    NowPlaying(session: session, onContinue: onContinue, onNew: onNew)
                    Rectangle().fill(Theme.hairline).frame(height: 1)
                    ChatPanel(session: session)
                }
                .frame(width: 400)
                .padding(.vertical, 8)
            }
            .padding(.horizontal, 32)
            .frame(maxHeight: .infinity)
            ScoreRiver(session: session)
                .frame(height: CGFloat(max(2, min(session.tracks.count, 8))) * (showConsole ? 11 : 20) + 12)
                .padding(.horizontal, 32)
                .padding(.bottom, 10)
            if showConsole {
                ConsoleView(console: console, session: session)
                    .padding(.horizontal, 32)
                    .padding(.bottom, 14)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
            }
            if showLog { logPanel }
        }
        .overlay(alignment: .top) { CueBanner(session: session).padding(.top, 58) }
        .animation(.easeInOut(duration: 0.25), value: session.cue?.url)
    }

    private var topBar: some View {
        HStack(spacing: 12) {
            Eyebrow("Agent Groovebox", color: Theme.accent)
            Text(session.brief.isEmpty ? "即兴演奏" : "“\(session.brief)”")
                .font(Theme.voice(13)).foregroundStyle(Theme.dim).lineLimit(1)
            Spacer()
            statusPill
            if let playlist, session.songURL != nil { PlaylistControls(playlist: playlist, session: session) }
            if session.songURL != nil {
                GhostButton(title: "打开", systemImage: "doc.badge.arrow.up") {
                    if let url = SongOpener.choose() { session.open(song: url) }
                }
                .help("打开另一个 .agbsong 文件")
            }
            GhostButton(title: showConsole ? "收起控制台" : "控制台", systemImage: "dial.medium") {
                withAnimation(.easeInOut(duration: 0.35)) { showConsole.toggle() }
            }
            GhostButton(title: showLog ? "收起日志" : "日志", systemImage: "text.alignleft") { showLog.toggle() }
            GhostButton(title: "唱片架", systemImage: "square.stack", action: onShelf)
            GhostButton(title: "设置", systemImage: "slider.horizontal.3", action: onSettings)
            if session.canTalk {
                GhostButton(title: session.finishing ? "正在收尾" : "收尾", systemImage: "moon.stars") { session.finish() }
                    .disabled(session.finishing || session.phrases.isEmpty)
                    .help("请乐队写一个终章，自然结束")
            }
            if session.canStop {
                GlowButton(systemImage: "stop.fill", size: 30, color: Theme.color(0)) { session.stop() }
                    .help("立即停止")
            }
        }
        .padding(.leading, 84).padding(.trailing, 20)
        .frame(height: 52)
    }

    private var statusPill: some View {
        let (t, c): (String, Color) = {
            switch session.status {
            case .preparing: return ("准备中", Theme.color(1))
            case .playing: return ("演奏中", Theme.color(2))
            case .finished: return ("终章", Theme.color(4))
            case .stopped: return ("已停止", Theme.faint)
            case .failed: return ("出错", Theme.color(0))
            case .idle: return ("待机", Theme.faint)
            }
        }()
        return HStack(spacing: 6) {
            Circle().fill(c).frame(width: 6, height: 6).shadow(color: c, radius: 4)
            Text(t).font(Theme.label(11)).foregroundStyle(c)
        }
    }

    /// Views this piece offers: the cover and the video exist only if the song file ships them.
    private var stageModes: [StageMode] {
        (session.coverURL != nil ? [.cover] : []) + [.orbit] + (session.videoURL != nil ? [.video] : [])
    }

    /// A song opens on its own picture: the video if it ships one, else the cover.
    private var defaultMode: StageMode {
        session.videoURL != nil ? .video : session.coverURL != nil ? .cover : .orbit
    }

    private var activeMode: StageMode { stageModes.contains(stageMode) ? stageMode : .orbit }

    /// Position, tempo and key while the cover or the video fills the stage.
    private var miniClock: some View {
        TimelineView(.periodic(from: .now, by: 1.0 / 10)) { _ in
            let bt = Double(session.barTicks)
            HStack(spacing: 10) {
                if let t = session.playheadTick {
                    Text("\(Int(t / bt) + 1)·\(Int(t.truncatingRemainder(dividingBy: bt) / session.ppqn) + 1)")
                        .font(Theme.numerals(18)).foregroundStyle(Theme.text).monospacedDigit()
                }
                Text("\(Int(session.currentBpm.rounded())) BPM").font(Theme.label(10)).tracking(2).foregroundStyle(Theme.dim)
                if let key = session.currentKeyLabel {
                    Text(key).font(Theme.display(14)).foregroundStyle(Theme.accent)
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 6)
            .background(Color.black.opacity(0.45), in: Capsule())
        }
    }

    private var clock: some View {
        TimelineView(.periodic(from: .now, by: 1.0 / 20)) { _ in
            let bt = Double(session.barTicks)
            VStack(spacing: 4) {
                if let t = session.playheadTick {
                    Text("\(Int(t / bt) + 1)·\(Int(t.truncatingRemainder(dividingBy: bt) / session.ppqn) + 1)")
                        .font(Theme.numerals(44)).foregroundStyle(Theme.text).monospacedDigit()
                    Text("\(Int(session.currentBpm.rounded())) BPM").font(Theme.label(10)).tracking(2).foregroundStyle(Theme.faint)
                    if let key = session.currentKeyLabel {
                        Text(key).font(Theme.display(17)).foregroundStyle(Theme.accent).padding(.top, 2)
                    } else if session.currentTranspose != 0 {
                        Text("移调 \(KeyName.signed(session.currentTranspose))").font(Theme.label(11)).foregroundStyle(Theme.accent)
                    }
                } else {
                    ThinkingDots()
                    Text("AI 正在构思开场").font(Theme.voice(14)).foregroundStyle(Theme.dim).padding(.top, 6)
                }
            }
        }
    }

    private var logPanel: some View {
        ScrollView {
            Text(session.log.joined(separator: "\n"))
                .font(.system(size: 10, design: .monospaced)).foregroundStyle(Theme.dim)
                .frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
        }
        .frame(height: 110).padding(10).glass(12)
        .padding(.horizontal, 32).padding(.bottom, 14)
    }
}

/// What the band is playing right now (compact, above the chat).
struct NowPlaying: View {
    @ObservedObject var session: Session
    let onContinue: (Int) -> Void
    let onNew: () -> Void

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.25)) { _ in
            let cur = session.currentPhrase(at: session.playheadTick ?? -1)
            VStack(alignment: .leading, spacing: 8) {
                if let ph = cur {
                    HStack(spacing: 8) {
                        Eyebrow("乐句 \(ph.id + 1) / \(max(session.totalPhrases, ph.id + 1))", color: Theme.accent)
                        if ph.answer { Eyebrow("回应你", color: Theme.color(3)) }
                        if ph.lateBars > 0 { Eyebrow("AI 晚到 \(ph.lateBars) 小节", color: Theme.faint) }
                    }
                    Text(ph.title)
                        .font(Theme.display(28)).foregroundStyle(Theme.text).lineLimit(2)
                        .id("t\(ph.id)").transition(.opacity)
                    Text(ph.commentary)
                        .font(Theme.voice(14)).foregroundStyle(Theme.text.opacity(0.75)).lineSpacing(4).lineLimit(4)
                        .fixedSize(horizontal: false, vertical: true)
                        .id("c\(ph.id)").transition(.opacity)
                } else {
                    Eyebrow("开场", color: Theme.accent)
                    Text("乐手正在调音").font(Theme.display(28)).foregroundStyle(Theme.text)
                    Text(session.songURL != nil ? "正在读取乐谱并合成声音。" : "AI 正在为“\(session.brief.isEmpty ? "即兴" : session.brief)”挑选乐器，写下第一个动机。")
                        .font(Theme.voice(14)).foregroundStyle(Theme.dim).lineSpacing(4)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if session.isRunning {
                    HStack(spacing: 8) {
                        ThinkingDots().frame(width: 24)
                        Text(session.activity).font(Theme.voice(12)).foregroundStyle(Theme.dim)
                    }
                }
                footer
            }
            .animation(.easeInOut(duration: 0.5), value: cur?.id)
        }
    }

    @ViewBuilder private var footer: some View {
        if session.status == .finished || session.status == .stopped {
            VStack(alignment: .leading, spacing: 8) {
                // go on with the music
                HStack(spacing: 8) {
                    if let song = session.songURL {
                        GhostButton(title: "再次播放", systemImage: "arrow.counterclockwise") {
                            session.open(song: song, mock: session.songMock, animated: false)
                        }
                        if session.canContinue {
                            GhostButton(title: "继续创作", systemImage: "sparkles") { onContinue(4) }
                                .help("AI 乐手从这首曲子的最后一小节接着往下写 4 句，可以继续和它对话")
                        }
                    } else {
                        GhostButton(title: "再演 4 句", systemImage: "arrow.forward") { onContinue(4) }
                            .help("AI 乐手接着往下写 4 句，可以继续和它对话")
                    }
                    if session.canExport || session.exporting {
                        GhostButton(title: session.exporting ? "正在保存…" : "另存为 agbsong", systemImage: "square.and.arrow.down") {
                            if let url = SongOpener.chooseSave(name: session.brief) { session.exportSong(to: url) }
                        }
                        .disabled(session.exporting)
                        .help("把这次演出保存成一首新的曲子：实际奏出的音符，连同演出过程中的全部改动")
                    }
                }
                // leave
                HStack(spacing: 8) {
                    GhostButton(title: "回到主界面", systemImage: "house", action: onNew)
                        .help("回到开始页，描述一段新的音乐，让 AI 现场创作")
                    GhostButton(title: "打开曲目", systemImage: "doc.badge.arrow.up") {
                        if let url = SongOpener.choose() { session.open(song: url) }
                    }
                    if let wav = session.fullWav, session.songURL == nil {
                        GhostButton(title: "在 Finder 中显示", systemImage: "folder") {
                            NSWorkspace.shared.activateFileViewerSelecting([wav])
                        }
                    }
                }
            }
        } else if case .failed(let m) = session.status {
            VStack(alignment: .leading, spacing: 8) {
                Text(m).font(.system(size: 12)).foregroundStyle(Theme.color(0))
                GhostButton(title: "返回", systemImage: "arrow.uturn.left", action: onNew)
            }
        }
    }
}

// MARK: - Shelf (library)

struct LibraryItem: Identifiable, Equatable {
    let id: String
    let url: URL
    let brief: String
    let phrases: Int
    let titles: [(String, String)]
    let date: Date
    var title: String { brief.isEmpty ? "即兴演奏" : brief }
    var dateText: String { date.formatted(date: .abbreviated, time: .shortened) }
    var mix: URL { url.appendingPathComponent("renders/full/mix.wav") }
    static func == (a: LibraryItem, b: LibraryItem) -> Bool { a.id == b.id && a.phrases == b.phrases }
}

@MainActor
final class Library: ObservableObject {
    @Published var items: [LibraryItem] = []

    func refresh() {
        let fm = FileManager.default
        let dirs = (try? fm.contentsOfDirectory(at: AppSettings.libraryDir, includingPropertiesForKeys: [.creationDateKey])) ?? []
        items = dirs.compactMap { d in
            guard let data = try? Data(contentsOf: d.appendingPathComponent("perform.json")),
                  let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
            let ph = o["phrases"] as? [[String: Any]] ?? []
            let date = (try? d.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            return LibraryItem(id: d.lastPathComponent, url: d, brief: o["brief"] as? String ?? "", phrases: ph.count,
                               titles: ph.map { ($0["title"] as? String ?? "", $0["commentary"] as? String ?? "") }, date: date)
        }
        .sorted { $0.date > $1.date }
    }
}

struct ShelfView: View {
    @ObservedObject var library: Library
    let onContinue: (LibraryItem, Int) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var selected: LibraryItem?
    @State private var player: AVAudioPlayer?
    @State private var playing: String?

    var body: some View {
        ZStack {
            InkBackground()
            HStack(spacing: 0) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 4) {
                        Eyebrow("唱片架", color: Theme.accent).padding(.bottom, 10)
                        if library.items.isEmpty {
                            Text("还没有演奏记录").font(Theme.voice(14)).foregroundStyle(Theme.faint)
                        }
                        ForEach(Array(library.items.enumerated()), id: \.element.id) { i, item in
                            Button { selected = item } label: {
                                HStack(spacing: 12) {
                                    RoundedRectangle(cornerRadius: 2).fill(Theme.color(i)).frame(width: 3, height: 34)
                                    VStack(alignment: .leading, spacing: 3) {
                                        Text(item.title).font(.system(size: 13, design: .serif)).foregroundStyle(Theme.text).lineLimit(1)
                                        Text("\(item.dateText) · \(item.phrases) 句").font(Theme.label(9)).foregroundStyle(Theme.faint)
                                    }
                                    Spacer()
                                }
                                .padding(8)
                                .background(selected?.id == item.id ? Color.white.opacity(0.07) : .clear,
                                            in: RoundedRectangle(cornerRadius: 8))
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(20)
                }
                .frame(width: 290)
                Rectangle().fill(Theme.hairline).frame(width: 1)
                detail.frame(maxWidth: .infinity)
            }
            VStack {
                HStack { Spacer(); GhostButton(title: "关闭", systemImage: "xmark") { stop(); dismiss() } }
                Spacer()
            }
            .padding(14)
        }
        .preferredColorScheme(.dark)
        .onAppear { library.refresh(); selected = library.items.first }
        .onDisappear { stop() }
    }

    @ViewBuilder private var detail: some View {
        if let item = selected {
            VStack(alignment: .leading, spacing: 16) {
                Spacer().frame(height: 30)
                Text(item.title).font(Theme.display(26)).foregroundStyle(Theme.text).fixedSize(horizontal: false, vertical: true)
                Text(item.dateText).font(Theme.label(10)).foregroundStyle(Theme.faint)
                HStack(spacing: 8) {
                    GlowButton(systemImage: playing == item.id ? "pause.fill" : "play.fill", size: 42) { toggle(item) }
                        .disabled(!FileManager.default.fileExists(atPath: item.mix.path))
                    GhostButton(title: "在 Finder 中显示", systemImage: "folder") {
                        NSWorkspace.shared.activateFileViewerSelecting([item.mix])
                    }
                    GhostButton(title: "接着演 4 句", systemImage: "arrow.forward") { stop(); onContinue(item, 4) }
                }
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        ForEach(Array(item.titles.enumerated()), id: \.offset) { i, t in
                            VStack(alignment: .leading, spacing: 4) {
                                Text("\(i + 1)  \(t.0)").font(.system(size: 14, design: .serif)).foregroundStyle(Theme.text)
                                Text(t.1).font(Theme.voice(13)).foregroundStyle(Theme.dim).lineSpacing(3)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(.horizontal, 28).padding(.bottom, 20)
        } else {
            Text("选一张唱片").font(Theme.voice(16)).foregroundStyle(Theme.faint)
        }
    }

    private func toggle(_ item: LibraryItem) {
        if playing == item.id { player?.pause(); playing = nil; return }
        stop()
        player = try? AVAudioPlayer(contentsOf: item.mix)
        player?.play()
        playing = player == nil ? nil : item.id
    }

    private func stop() {
        player?.stop(); player = nil; playing = nil
    }
}

// MARK: - Settings

struct SettingsView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var key = KeyStore.load() ?? ""
    @State private var baseURL = AppSettings.baseURL
    @State private var models = AppSettings.models
    @State private var available: [String] = []
    @State private var message = ""
    @AppStorage(LoadingStyle.storageKey) private var loadingStyle = "random"

    var body: some View {
        Form {
            Section("装载动画") {
                Picker("打开曲目时", selection: $loadingStyle) {
                    Text("每次不同").tag("random")
                    ForEach(LoadingStyle.allCases) { s in Text(s.name).tag(s.rawValue) }
                }
            }
            Section("agentllm") {
                SecureField("API Key", text: $key)
                HStack {
                    Button("从文件导入…") { importKey() }
                    Button("导入 ~/Dev/AI-passport/private/agentllm.key") {
                        if let k = KeyStore.readKeyFile(KeyStore.defaultKeyFile) { key = k; message = "已读取默认 key 文件" }
                        else { message = "默认 key 文件不存在" }
                    }
                }
                .font(.caption)
                TextField("网关地址", text: $baseURL)
            }
            Section("模型（按顺序回退）") {
                TextField("逗号分隔", text: $models)
                HStack {
                    Button("获取可用模型") { Task { await refresh() } }
                    if !available.isEmpty {
                        Menu("设为首选…") {
                            ForEach(available, id: \.self) { m in
                                Button(m) {
                                    var list = models.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
                                    list.removeAll { $0 == m }
                                    list.insert(m, at: 0)
                                    models = list.joined(separator: ",")
                                }
                            }
                        }
                        .fixedSize()
                    }
                }
                .font(.caption)
            }
            if !message.isEmpty { Text(message).font(.caption).foregroundStyle(.secondary) }
            HStack {
                Spacer()
                Button("取消") { dismiss() }
                Button("保存") { save(); dismiss() }.buttonStyle(.borderedProminent)
            }
        }
        .formStyle(.grouped)
        .padding()
        .preferredColorScheme(.dark)
    }

    private func importKey() {
        let p = NSOpenPanel()
        p.canChooseFiles = true
        p.allowsMultipleSelection = false
        p.directoryURL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Dev")
        if p.runModal() == .OK, let u = p.url, let k = KeyStore.readKeyFile(u) {
            key = k
            message = "已从 \(u.lastPathComponent) 读取"
        }
    }

    private func save() {
        if !KeyStore.save(key) {
            // Never fail silently: the key would be missing the next time it is needed.
            let a = NSAlert()
            a.messageText = "API Key 没有保存成功"
            a.informativeText = "钥匙串拒绝了写入。请打开“钥匙串访问”，删除 app.agent-groovebox.agentllm 这一项后再保存一次。"
            a.runModal()
        }
        AppSettings.baseURL = baseURL.trimmingCharacters(in: .whitespaces)
        AppSettings.models = models
    }

    private func refresh() async {
        AppSettings.baseURL = baseURL
        do {
            available = try await AppSettings.fetchModels(key: key)
            message = "可用模型 \(available.count) 个"
        } catch {
            message = "获取失败：\(error.localizedDescription)"
        }
    }
}
