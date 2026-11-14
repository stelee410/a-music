import Foundation
import SwiftUI

struct NoteEvt {
    let tick: Int
    let track: String
    let pitch: Int
    let velocity: Double
    let dur: Int
}

struct PhraseChange: Identifiable, Equatable {
    let id: Int // phrase index
    let tick: Int
    let bars: Int
    let title: String
    let commentary: String
    let model: String
    let lateBars: Int
    var answer = false
}

struct LayerInfo: Identifiable, Equatable {
    var id: String { "\(track)/\(layer)" }
    let track: String
    let layer: String
    let length: Int
    let offset: Int
    let mute: Bool
    let noteSteps: Set<Int>

    /// Local step at a global tick (perform never launches scenes → origin 0).
    func step(atTick t: Double, ticksPerStep: Double) -> Int {
        let g = Int((t / ticksPerStep).rounded(.down))
        let n = max(length, 1)
        return ((g + offset) % n + n) % n
    }
}

struct TrackInfo: Identifiable, Equatable {
    let id: String
    let engine: String
    let preset: String?
    let mute: Bool
    var layers: [LayerInfo] = []

    var shortPreset: String {
        guard let p = preset else { return engine }
        let parts = p.replacingOccurrences(of: "preset:", with: "").split(separator: ":")
        guard parts.count >= 3 else { return p }
        return "\(parts[1]) · \(parts[2].split(separator: "@").first ?? "")"
    }
}

/// One line of the stage conversation feed.
struct FeedItem: Identifiable, Equatable {
    enum Kind { case listener, ai, phrase, event }
    let id = UUID()
    let kind: Kind
    let text: String
    var detail: String = ""
}

struct PerformOptions {
    var brief = ""
    var phrases = 8
    var bars = 8
    var realtime = true
    var mock = false
}

@MainActor
final class Session: ObservableObject {
    enum Status: Equatable {
        case idle, preparing, playing, finished, stopped
        case failed(String)
    }

    @Published var status: Status = .idle
    @Published var activity = ""
    @Published var phrases: [PhraseChange] = []
    @Published var tracks: [TrackInfo] = []
    @Published var feed: [FeedItem] = []
    @Published var aiThinkingAboutListener = false
    @Published var log: [String] = []
    @Published var totalPhrases = 0
    @Published var projectDir: URL?
    @Published var fullWav: URL?
    @Published var brief = ""
    @Published var finishing = false
    /// Set while playing an `.agbsong` file.
    @Published var songURL: URL?
    /// The tape-loading animation is on screen (name on the cassette, start time).
    @Published var loadingSong: (title: String, started: Date)?
    private var openGeneration = 0
    private(set) var videoAspect: Double = 16.0 / 9.0
    /// Whether the engine playing the song has an API key (so the AI can answer).
    private var performerConnected = true
    private var keyNote = ""
    /// Cover image and video shipped inside the song file, if any.
    @Published var coverURL: URL?
    @Published var videoURL: URL?
    private(set) var videoFollowsSong = false
    /// Tempo the file was written at (nil when it has a tempo map).
    private(set) var writtenBpm: Double?
    /// Tonic and scale as written in the file (before transposition).
    @Published var songKey: String?
    @Published var songScale: String?
    /// Transposition the listener asked for (semitones relative to the written score).
    @Published var transposeTarget = 0
    @Published var transposeRange: ClosedRange<Int> = -12...12
    /// (tick, semitones) in force from each tick on.
    private(set) var transposeMap: [(Double, Int)] = [(0, 0)]

    private(set) var events: [NoteEvt] = []
    private(set) var tempo: Double = 120
    private(set) var ppqn: Double = 960
    private(set) var barTicks: Int = 3840
    private(set) var ticksPerStep: Double = 240
    private(set) var renderedUntil: Int = 0
    /// (tick, startSample, bpm)
    private(set) var tempoMap: [(Double, Double, Double)] = [(0, 0, 120)]
    let audio: LiveAudio

    init(silentAudio: Bool = false) {
        audio = LiveAudio(silent: silentAudio)
    }
    private var process: Process?
    private var stdin: FileHandle?
    private var buffer = Data()

    var isRunning: Bool { status == .preparing || status == .playing }
    /// A performer is listening (chat, tempo, finish).
    var canTalk: Bool { isRunning }

    // MARK: transport

    var playheadTick: Double? {
        guard let s = audio.positionSample else { return nil }
        let x = Double(s)
        let seg = tempoMap.last(where: { $0.1 <= x }) ?? tempoMap[0]
        return seg.0 + (x - seg.1) * seg.2 * ppqn / (60 * audio.sampleRate)
    }

    /// Tempo at the playhead.
    var currentBpm: Double {
        guard let t = playheadTick else { return tempo }
        return (tempoMap.last(where: { $0.0 <= t }) ?? tempoMap[0]).2
    }

    /// Transposition at the playhead.
    var currentTranspose: Int {
        guard let t = playheadTick else { return transposeMap[0].1 }
        return (transposeMap.last(where: { $0.0 <= t }) ?? transposeMap[0]).1
    }

    /// The key being heard right now, e.g. "F 大调"; nil when the piece does not state its key.
    var currentKeyLabel: String? {
        guard let k = songKey else { return nil }
        return KeyName.label(tonic: k, scale: songScale, transpose: currentTranspose)
    }

    /// Seconds into the piece at the playhead.
    var playheadSeconds: Double? {
        guard isRunning || status == .finished, let s = audio.positionSample else { return nil }
        return Double(s) / audio.sampleRate
    }

    /// Where a `sync: "song"` video should be: the musical position, at the tempo the file was
    /// written at, so the picture stays on its bar when the listener changes the tempo.
    var videoSeconds: Double? {
        guard isRunning || status == .finished else { return nil }
        if let w = writtenBpm, w > 0, let t = playheadTick { return t / ppqn * 60 / w }
        return playheadSeconds
    }

    /// Playback rate that keeps such a video in step with the current tempo.
    var videoRate: Double {
        guard let w = writtenBpm, w > 0 else { return 1 }
        return min(max(currentBpm / w, 0.25), 4)
    }

    func setTranspose(_ semitones: Int) {
        guard canTalk else { return }
        let n = min(max(semitones, transposeRange.lowerBound), transposeRange.upperBound)
        guard n != transposeTarget else { return }
        transposeTarget = n
        send(["type": "set_transpose", "semitones": n])
    }

    func currentPhrase(at tick: Double) -> PhraseChange? {
        phrases.last(where: { Double($0.tick) <= tick })
    }

    // MARK: listener → performer

    private func send(_ obj: [String: Any]) {
        guard let h = stdin, let d = try? JSONSerialization.data(withJSONObject: obj) else { return }
        h.write(d + Data([0x0A]))
    }

    func say(_ text: String) {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, canTalk else { return }
        feed.append(FeedItem(kind: .listener, text: t))
        // A song opened before the key was saved: hand the key to the running engine now.
        if songURL != nil, !performerConnected, let key = KeyStore.current() {
            send(["type": "set_key", "key": key])
            performerConnected = true
        }
        aiThinkingAboutListener = true
        send(["type": "chat", "text": t])
    }

    func setTempo(_ bpm: Double) {
        guard canTalk else { return }
        send(["type": "set_tempo", "bpm": bpm])
    }

    func sendHumanState(_ s: [String: Any]) {
        guard canTalk else { return }
        send(["type": "human_state", "state": s])
    }

    func finish() {
        guard canTalk, !finishing else { return }
        finishing = true
        send(["type": "finish"])
    }

    // MARK: control

    func start(_ o: PerformOptions, continuing dir: URL? = nil) {
        stop(final: false)
        guard let bin = AppSettings.agbBinary else {
            status = .failed("找不到 agb 引擎（请先运行 app/build-app.sh）")
            return
        }
        var key: String?
        if !o.mock {
            if let envKey = ProcessInfo.processInfo.environment["AGENTLLM_API_KEY"], !envKey.isEmpty {
                key = envKey // developer override
            } else {
                KeyStore.bootstrap() // first use: import ~/Dev/AI-passport/private/agentllm.key
                key = KeyStore.load()
            }
            if key == nil {
                status = .failed("没有 API Key：请在 设置 中填写或导入 agentllm key")
                return
            }
        }
        let project: URL
        if let dir {
            project = dir
        } else {
            let f = DateFormatter()
            f.dateFormat = "yyyyMMdd-HHmmss"
            project = AppSettings.libraryDir.appendingPathComponent(f.string(from: Date()))
        }
        reset()
        songURL = nil
        brief = o.brief
        projectDir = project
        status = .preparing
        activity = "AI 正在构思开场"

        var args = ["perform", "--project", project.path, "--phrases", String(o.phrases), "--bars", String(o.bars)]
        if !o.brief.isEmpty { args += ["--brief", o.brief] }
        if o.realtime { args.append("--realtime") }
        if o.mock { args.append("--mock") }
        if dir != nil { args.append("--continue") }
        let models = AppSettings.models.trimmingCharacters(in: .whitespaces)
        if !models.isEmpty { args += ["--model", models] }

        var env = ProcessInfo.processInfo.environment
        if let key { env["AGENTLLM_API_KEY"] = key }
        env["AGENTLLM_API_BASE"] = AppSettings.baseURL
        launch(bin: bin, args: args, env: env)
    }

    /// Play an `.agbsong` file (directory or zip). The engine streams the same
    /// events as a performance, so the stage, faders and FX work unchanged.
    /// `animated`: show the robot loading the cassette; the engine is started so
    /// that the music begins as the play button goes down.
    func open(song url: URL, transpose: Int = 0, bpm: Double? = nil, mock: Bool = false, animated: Bool = true) {
        stop(final: false)
        guard let bin = AppSettings.agbBinary else {
            status = .failed("找不到 agb 引擎（请先运行 app/build-app.sh）")
            return
        }
        reset()
        songURL = url
        brief = url.deletingPathExtension().lastPathComponent
        let work = FileManager.default.temporaryDirectory
            .appendingPathComponent("agbsong-play-\(UUID().uuidString)")
        status = .preparing
        activity = "正在读取 \(url.lastPathComponent)"
        var args = ["song", "play", url.path, "--work-dir", work.path, "--realtime"]
        if transpose != 0 { args += ["--transpose", String(transpose)] }
        if let bpm { args += ["--bpm", String(bpm)] }
        if mock { args.append("--mock") }
        let models = AppSettings.models.trimmingCharacters(in: .whitespaces)
        if !models.isEmpty { args += ["--model", models] }
        // The AI performer answers the listener while the song plays. Without a key the song still plays.
        var env = ProcessInfo.processInfo.environment
        if !mock, (env["AGENTLLM_API_KEY"] ?? "").isEmpty {
            KeyStore.bootstrap()
            if let key = KeyStore.current() { env["AGENTLLM_API_KEY"] = key }
        }
        keyNote = mock ? "离线乐手" : (env["AGENTLLM_API_KEY"] ?? "").isEmpty ? "没有 API Key" : "API Key 已就绪（\(KeyStore.load() != nil ? "钥匙串" : "key 文件或环境变量")）"
        performerConnected = mock || !(env["AGENTLLM_API_KEY"] ?? "").isEmpty
        env["AGENTLLM_API_BASE"] = AppSettings.baseURL
        openGeneration += 1
        let generation = openGeneration
        let launchEnv = env
        guard animated else {
            loadingSong = nil
            launch(bin: bin, args: args, env: launchEnv)
            return
        }
        let total = TapeLoadingScene.duration
        loadingSong = (brief, Date())
        Task { @MainActor in
            // Starting the engine and rendering the first bar takes a moment: begin a little before the press.
            try? await Task.sleep(for: .seconds(total * TapeLoadingScene.pressAt - 0.45))
            guard generation == self.openGeneration, self.status == .preparing else { return }
            self.launch(bin: bin, args: args, env: launchEnv)
        }
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(total + 0.15))
            if generation == self.openGeneration { self.loadingSong = nil }
        }
    }

    private func launch(bin: URL, args: [String], env: [String: String]) {
        let p = Process()
        p.executableURL = bin
        p.arguments = args
        p.environment = env
        let out = Pipe(), err = Pipe(), inp = Pipe()
        p.standardOutput = out
        p.standardError = err
        p.standardInput = inp
        out.fileHandleForReading.readabilityHandler = { [weak self] h in
            let d = h.availableData
            guard !d.isEmpty else { return }
            Task { @MainActor in self?.ingest(d) }
        }
        err.fileHandleForReading.readabilityHandler = { [weak self] h in
            let d = h.availableData
            guard !d.isEmpty, let s = String(data: d, encoding: .utf8) else { return }
            Task { @MainActor in self?.append("stderr: \(s.trimmingCharacters(in: .whitespacesAndNewlines))") }
        }
        p.terminationHandler = { [weak self] proc in
            let code = proc.terminationStatus
            Task { @MainActor in self?.processEnded(code) }
        }
        do {
            try p.run()
            process = p
            stdin = inp.fileHandleForWriting
            append("▶︎ agb \(args.joined(separator: " "))")
        } catch {
            status = .failed("无法启动引擎：\(error.localizedDescription)")
        }
    }

    func stop(final: Bool = true) {
        openGeneration += 1
        loadingSong = nil
        if let p = process, p.isRunning { p.terminate() }
        process = nil
        stdin = nil
        audio.resetBand()
        if final, isRunning { status = .stopped; activity = "已停止" }
    }

    private func reset() {
        phrases = []
        tracks = []
        log = []
        feed = []
        events = []
        fullWav = nil
        renderedUntil = 0
        buffer = Data()
        totalPhrases = 0
        finishing = false
        aiThinkingAboutListener = false
        tempoMap = [(0, 0, 120)]
        transposeMap = [(0, 0)]
        transposeTarget = 0
        transposeRange = -12...12
        songKey = nil
        songScale = nil
        coverURL = nil
        videoURL = nil
        videoFollowsSong = false
        writtenBpm = nil
        audio.resetBand()
    }

    private func processEnded(_ code: Int32) {
        process = nil
        stdin = nil
        guard code != 0, status != .stopped, status != .finished else { return }
        if case .failed = status { return }
        status = .failed("引擎退出（代码 \(code)），详见日志")
    }

    private func append(_ s: String) {
        log.append(s)
        if log.count > 400 { log.removeFirst(log.count - 400) }
    }

    // MARK: event stream

    private func ingest(_ d: Data) {
        buffer.append(d)
        while let nl = buffer.firstIndex(of: 0x0A) {
            let line = buffer[buffer.startIndex..<nl]
            buffer.removeSubrange(buffer.startIndex...nl)
            guard let obj = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else { continue }
            handle(obj)
        }
    }

    private func bar(_ tick: Int) -> Int { tick / max(barTicks, 1) + 1 }

    private func handle(_ e: [String: Any]) {
        let type = e["type"] as? String ?? ""
        let phraseNo = ((e["phrase"] as? Int) ?? 0) + 1
        switch type {
        case "session":
            totalPhrases = e["phrases"] as? Int ?? 0
            barTicks = e["barTicks"] as? Int ?? 3840
            ppqn = Double(e["ppqn"] as? Int ?? 960)
            if e["song"] != nil {
                append("AI 乐手：\(keyNote)")
                songKey = e["key"] as? String
                songScale = e["scale"] as? String
                if let r = e["transposeRange"] as? [Int], r.count == 2, r[0] <= r[1] { transposeRange = r[0]...r[1] }
                transposeTarget = e["transpose"] as? Int ?? 0
                transposeMap = [(0, transposeTarget)]
                coverURL = (e["cover"] as? String).map { URL(fileURLWithPath: $0) }
                videoURL = (e["video"] as? String).map { URL(fileURLWithPath: $0) }
                videoFollowsSong = e["videoSync"] as? String == "song"
                writtenBpm = e["writtenBpm"] as? Double
                videoAspect = e["videoAspect"] as? Double ?? 16.0 / 9.0
                if let t = e["title"] as? String { brief = t }
                activity = "正在播放"
                let skipped = e["skipped"] as? [[String: Any]] ?? []
                for s in skipped { append("跳过：\(s["message"] as? String ?? "")（\(s["path"] as? String ?? "")）") }
                if !skipped.isEmpty {
                    feed.append(FeedItem(kind: .event, text: "这个播放器跳过了文件里的 \(skipped.count) 项内容", detail: "详见日志"))
                }
            }
        case "phrase_request":
            if !aiThinkingAboutListener {
                activity = phrases.isEmpty ? "AI 正在构思开场" : ((e["final"] as? Bool ?? false) ? "AI 正在写终章" : "AI 正在构思第 \(phraseNo) 句")
            } else {
                activity = "乐手正在回应你"
            }
            if e["answer"] as? Bool == true { append("→ 已把你的话交给乐手（第 \((e["attempt"] as? Int ?? 0) + 1) 次）") }
            if (e["attempt"] as? Int ?? 0) > 0 { append("↻ 第 \(phraseNo) 句重新请求（第 \((e["attempt"] as? Int ?? 0) + 1) 次）") }
        case "llm_reply":
            let ms = e["elapsedMs"] as? Int ?? 0
            append("第 \(phraseNo) 句：\(e["model"] as? String ?? "?") 用时 \(String(format: "%.1f", Double(ms) / 1000)) s")
        case "llm_error":
            append("⚠︎ 模型调用失败：\(e["message"] as? String ?? "")")
            activity = "模型暂时没有回应，正在重试"
        case "txn_rejected":
            let code = e["code"] as? String ?? ""
            append("✗ 引擎退回第 \(phraseNo) 句（\(code)）：\(e["message"] as? String ?? "")")
            activity = "引擎退回了第 \(phraseNo) 句（\(code)），AI 正在修正"
        case "phrase_skipped":
            append("… 第 \(phraseNo) 句无可用回复，继续循环")
        case "chat":
            if e["who"] as? String == "ai", let t = e["text"] as? String {
                feed.append(FeedItem(kind: .ai, text: t))
                aiThinkingAboutListener = false
            }
        case "change":
            let c = PhraseChange(
                id: e["phrase"] as? Int ?? 0, tick: e["tick"] as? Int ?? 0, bars: e["bars"] as? Int ?? 0,
                title: e["title"] as? String ?? "", commentary: e["commentary"] as? String ?? "",
                model: e["model"] as? String ?? "", lateBars: e["lateBars"] as? Int ?? 0, answer: e["answer"] as? Bool ?? false)
            phrases.append(c)
            feed.append(FeedItem(kind: .phrase, text: c.title, detail: "第 \(bar(c.tick)) 小节 · \(c.commentary)"))
            append("✓ 第 \(c.id + 1) 句「\(c.title)」第 \(bar(c.tick)) 小节进入\(c.lateBars > 0 ? "（晚 \(c.lateBars) 小节）" : "")")
            let isFinal = e["final"] as? Bool ?? false
            if songURL != nil {
                if c.answer { aiThinkingAboutListener = false }
                activity = c.answer ? "乐手改了：\(c.title)" : "正在播放「\(c.title)」"
                break
            }
            activity = isFinal ? "终章已就位" : "AI 正在构思第 \(c.id + 2) 句"
        case "human_change":
            if e["kind"] as? String == "transpose", let n = e["semitones"] as? Int {
                let key = songKey.flatMap { KeyName.label(tonic: $0, scale: songScale, transpose: n) }
                feed.append(FeedItem(kind: .event, text: "移调 \(KeyName.signed(n))\(key.map { " → \($0)" } ?? "")",
                                     detail: "第 \(bar(e["tick"] as? Int ?? 0)) 小节起"))
            } else if let bpm = e["bpm"] as? Double {
                feed.append(FeedItem(kind: .event, text: "BPM → \(Int(bpm))", detail: "第 \(bar(e["tick"] as? Int ?? 0)) 小节起"))
            }
        case "human_rejected":
            append("✗ 操作未生效：\(e)")
        case "finishing":
            activity = "AI 正在写终章"
            feed.append(FeedItem(kind: .event, text: "你请乐队收尾"))
        case "chunk":
            tempo = e["tempoBpm"] as? Double ?? tempo
            ticksPerStep = Double(e["ticksPerStep"] as? Int ?? 240)
            renderedUntil = e["endTick"] as? Int ?? renderedUntil
            if let tm = e["tempoMap"] as? [[Double]] {
                tempoMap = tm.compactMap { $0.count == 3 ? ($0[0], $0[1], $0[2]) : nil }
            }
            if let tm = e["transposeMap"] as? [[Int]], !tm.isEmpty {
                transposeMap = tm.compactMap { $0.count == 2 ? (Double($0[0]), $0[1]) : nil }
                // The AI may transpose too: keep the control in step with the latest value.
                if let last = transposeMap.last?.1, last != transposeTarget { transposeTarget = last }
            }
            if let ts = e["tracks"] as? [[String: Any]] {
                let t = ts.map { d -> TrackInfo in
                    let tid = d["trackId"] as? String ?? ""
                    let layers = (d["layers"] as? [[String: Any]] ?? []).map { l in
                        LayerInfo(track: tid, layer: l["layerId"] as? String ?? "", length: l["lengthSteps"] as? Int ?? 16,
                                  offset: l["phaseOffsetSteps"] as? Int ?? 0, mute: l["mute"] as? Bool ?? false,
                                  noteSteps: Set(l["noteSteps"] as? [Int] ?? []))
                    }
                    return TrackInfo(id: tid, engine: d["engineId"] as? String ?? "", preset: d["presetId"] as? String,
                                     mute: d["mute"] as? Bool ?? false, layers: layers)
                }
                if t != tracks { tracks = t }
            }
            if let evs = e["events"] as? [[Any]] {
                for a in evs where a.count >= 6 {
                    events.append(NoteEvt(tick: a[0] as? Int ?? 0, track: a[1] as? String ?? "", pitch: a[3] as? Int ?? 60,
                                          velocity: a[4] as? Double ?? 0.8, dur: a[5] as? Int ?? 240))
                }
            }
            let stems = (e["stems"] as? [[String: Any]] ?? []).compactMap { s -> (String, URL)? in
                guard let t = s["trackId"] as? String, let w = s["wav"] as? String else { return nil }
                return (t, URL(fileURLWithPath: w))
            }
            audio.schedule(stems: stems, startSample: Int64(e["startSample"] as? Int ?? 0))
            onTracksChanged?()
            if status == .preparing { status = .playing }
        case "done":
            if let p = e["fullWav"] as? String { fullWav = URL(fileURLWithPath: p) }
            activity = songURL != nil ? "已全部送出，播放到结尾" : "演奏完成"
            append("■ 完成：\(String(format: "%.0f", e["durationSec"] as? Double ?? 0)) 秒")
            status = .finished
        case "fatal":
            status = .failed(e["message"] as? String ?? "未知错误")
            append("✗ \(e["message"] as? String ?? "")")
        default:
            break
        }
    }

    /// Called after each chunk so the console can re-apply fader settings to new tracks.
    var onTracksChanged: (() -> Void)?
}
