import AppKit
import Foundation
import SwiftUI

/// One sample pad (bank `bank`, slot `id` 0…7).
struct Pad: Identifiable, Equatable {
    let id: Int
    let bank: Int
    var name: String
    var url: URL?
    var key: String
    var settings = PadSettings()
    var flash: Date = .distantPast
    var sampleKey: String { "\(bank)-\(id)" }
}

/// Healing-frequency presets.
enum Healing {
    static let solfeggio: [(Double, String)] = [
        (174, "174 · 安定"), (285, "285 · 修复"), (396, "396 · 释放"), (417, "417 · 转化"),
        (432, "432 · 自然"), (528, "528 · 爱与修复"), (639, "639 · 连接"), (741, "741 · 清理"),
        (852, "852 · 直觉"), (963, "963 · 觉知"),
    ]
    static let bands: [(String, String, Double)] = [
        ("δ", "Delta 深睡", 2), ("θ", "Theta 冥想", 6), ("α", "Alpha 放松", 10), ("β", "Beta 专注", 18), ("γ", "Gamma 觉察", 40),
    ]
    static func bandName(_ hz: Double) -> String {
        switch hz {
        case ..<4: return "δ Delta"
        case ..<8: return "θ Theta"
        case ..<13: return "α Alpha"
        case ..<30: return "β Beta"
        default: return "γ Gamma"
        }
    }
}

/// Everything the human controls directly. Changes go straight to the audio
/// graph (instant) and are summarised for the AI (debounced).
@MainActor
final class Console: ObservableObject {
    let session: Session
    var audio: LiveAudio { session.audio }

    // band mixer
    @Published var volumes: [String: Double] = [:]
    @Published var mutes: Set<String> = []
    // master
    @Published var filter = 1.0 { didSet { audio.setFilter(filter); touch() } }
    @Published var reverb = 0.12 { didSet { audio.setReverb(reverb); touch() } }
    @Published var delay = 0.0 { didSet { audio.setDelay(delay, bpm: session.currentBpm); touch() } }
    @Published var tuningA4 = 440.0 { didSet { audio.setTuning(a4: tuningA4); touch() } }
    @Published var master = 0.9 { didSet { audio.setMaster(master) } }
    @Published var bpmTarget: Double = 0
    // pads
    @Published var banks: [[Pad]] = []
    @Published var bank = 0 { didSet { audio.setPadSpace(space[bank]) } }
    @Published var selected = 0
    @Published var quantize = false
    @Published var space: [Double] = PadBank.banks.map(\.space) { didSet { audio.setPadSpace(space[bank]); PadBank.saveSpace(space) } }
    @Published var padLevel = 0.85 { didSet { audio.setPads(padLevel) } }
    @Published var padsReady = false
    private var padHits: [(Date, String)] = []
    var pads: [Pad] { banks.indices.contains(bank) ? banks[bank] : [] }
    var selectedPad: Pad? { pads.indices.contains(selected) ? pads[selected] : nil }
    // healing
    @Published var toneOn = false { didSet { audio.healing.toneOn = toneOn; touch() } }
    @Published var toneHz = 432.0 { didSet { audio.healing.toneHz = toneHz; touch() } }
    @Published var toneLevel = 0.35 { didSet { audio.healing.toneLevel = toneLevel } }
    @Published var binOn = false { didSet { audio.healing.binOn = binOn; touch() } }
    @Published var carrierHz = 200.0 { didSet { audio.healing.carrierHz = carrierHz; touch() } }
    @Published var beatHz = 6.0 { didSet { audio.healing.beatHz = beatHz; touch() } }
    @Published var binLevel = 0.35 { didSet { audio.healing.binLevel = binLevel } }
    @Published var noiseOn = false { didSet { audio.healing.noiseOn = noiseOn; touch() } }
    @Published var noiseColor = 1 { didSet { audio.healing.noiseColor = noiseColor; touch() } }
    @Published var noiseLevel = 0.25 { didSet { audio.healing.noiseLevel = noiseLevel } }
    // recording
    @Published var recording = false
    @Published var lastRecording: URL?

    private var stateTask: Task<Void, Never>?
    private var tempoTask: Task<Void, Never>?
    private var keyMonitor: Any?

    init(session: Session) {
        self.session = session
        audio.setReverb(reverb)
        audio.setMaster(master)
        banks = PadBank.load()
        space = PadBank.loadSpace()
        audio.setPadSpace(space[bank])
        reloadPads()
        installKeys()
        session.onTracksChanged = { [weak self] in self?.syncTracks() }
        Task {
            await PadBank.ensureDefaults()
            self.banks = PadBank.load()
            self.reloadPads()
            self.padsReady = true
        }
    }

    // MARK: band

    func volume(_ t: String) -> Double { volumes[t] ?? 0.85 }

    func setVolume(_ t: String, _ v: Double) {
        volumes[t] = v
        audio.setTrack(t, volume: Float(v), muted: mutes.contains(t))
        touch()
    }

    func toggleMute(_ t: String) {
        if mutes.contains(t) { mutes.remove(t) } else { mutes.insert(t) }
        audio.setTrack(t, volume: Float(volume(t)), muted: mutes.contains(t))
        touch()
    }

    /// Re-apply fader settings (new tracks appear as chunks arrive).
    func syncTracks() {
        for t in session.tracks {
            audio.setTrack(t.id, volume: Float(volume(t.id)), muted: mutes.contains(t.id))
        }
    }

    func requestTempo(_ bpm: Double) {
        bpmTarget = bpm
        tempoTask?.cancel()
        tempoTask = Task {
            try? await Task.sleep(for: .milliseconds(600))
            guard !Task.isCancelled else { return }
            session.setTempo(bpm.rounded())
            audio.setDelay(delay, bpm: bpm)
        }
    }

    // MARK: pads

    func reloadPads() {
        for b in banks { for p in b { if let u = p.url { _ = audio.loadPad(p.sampleKey, url: u) } } }
    }

    func hit(_ i: Int) {
        guard pads.indices.contains(i) else { return }
        let p = pads[i]
        var delay: Double? = nil
        if quantize, session.isRunning, let tick = session.playheadTick {
            // next 16th note
            let tps = session.ticksPerStep
            let next = (tick / tps).rounded(.down) * tps + tps
            delay = (next - tick) * 60 / (session.currentBpm * session.ppqn)
        }
        audio.triggerPad(p.sampleKey, settings: p.settings, delaySeconds: delay)
        banks[bank][i].flash = Date()
        selected = i
        padHits.append((Date(), "\(PadBank.banks[bank].title)·\(p.name)"))
        touch()
    }

    func updateSelected(_ f: (inout PadSettings) -> Void) {
        guard banks.indices.contains(bank), banks[bank].indices.contains(selected) else { return }
        f(&banks[bank][selected].settings)
        PadBank.save(banks)
    }

    func resetSelected() { updateSelected { $0 = PadSettings() } }

    func assign(_ i: Int, file: URL) {
        guard let u = PadBank.importSample(file, bank: bank, pad: i) else { return }
        banks[bank][i].url = u
        banks[bank][i].name = String(file.deletingPathExtension().lastPathComponent.prefix(6))
        PadBank.save(banks)
        _ = audio.loadPad(banks[bank][i].sampleKey, url: u)
    }

    private func installKeys() {
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] ev in
            // don't steal typing
            if NSApp.keyWindow?.firstResponder is NSTextView { return ev }
            guard ev.modifierFlags.intersection([.command, .control, .option]).isEmpty,
                  let ch = ev.charactersIgnoringModifiers?.lowercased(), ev.isARepeat == false else { return ev }
            guard let self else { return ev }
            if let n = Int(ch), (1...PadBank.banks.count).contains(n) {
                self.bank = n - 1
                return nil
            }
            guard let i = self.pads.firstIndex(where: { $0.key == ch }) else { return ev }
            self.hit(i)
            return nil
        }
    }

    // MARK: recording

    func toggleRecording() {
        if recording {
            audio.stopRecording()
            recording = false
            lastRecording = audio.recordingURL
        } else {
            let dir = session.projectDir ?? AppSettings.libraryDir
            let f = DateFormatter(); f.dateFormat = "HHmmss"
            let url = dir.appendingPathComponent("live-\(f.string(from: Date())).caf")
            do { try audio.startRecording(to: url); recording = true } catch { recording = false }
        }
    }

    // MARK: report to the AI

    private func touch() {
        stateTask?.cancel()
        stateTask = Task {
            try? await Task.sleep(for: .milliseconds(1500))
            guard !Task.isCancelled else { return }
            session.sendHumanState(summary())
        }
    }

    func summary() -> [String: Any] {
        let recent = padHits.filter { $0.0 > Date().addingTimeInterval(-60) }
        padHits = recent
        var mixer: [String: Any] = [:]
        for t in session.tracks {
            let v = volume(t.id)
            if mutes.contains(t.id) || abs(v - 0.85) > 0.05 {
                mixer[t.id] = ["volume": (v * 100).rounded() / 100, "muted": mutes.contains(t.id)]
            }
        }
        var s: [String: Any] = [
            "masterFilter": filter >= 0.995 ? "open" : "low-pass at \(Int(200 * pow(100, filter))) Hz",
            "reverb": (reverb * 100).rounded() / 100,
            "delay": (delay * 100).rounded() / 100,
            "tuningA4Hz": tuningA4,
        ]
        if !mixer.isEmpty { s["bandMixer"] = mixer }
        if !recent.isEmpty {
            s["padsLastMinute"] = ["hits": recent.count, "sounds": Array(Set(recent.map(\.1)))]
        }
        if toneOn { s["toneGenerator"] = ["hz": toneHz] }
        if binOn { s["binauralBeat"] = ["carrierHz": carrierHz, "beatHz": beatHz, "band": Healing.bandName(beatHz)] }
        if noiseOn { s["noise"] = ["white", "pink", "brown"][max(0, min(2, noiseColor))] }
        return s
    }
}

/// Pad banks: sounds rendered by the engine on first launch (all original synthesis).
enum PadBank {
    struct Def {
        let file: String
        let name: String
        let args: [String]
    }
    struct Bank {
        let title: String
        let space: Double
        let pads: [Def]
    }

    static var dir: URL {
        let d = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("AgentGroovebox/pads-v2")
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }
    static let keys = ["a", "s", "d", "f", "g", "h", "j", "k"]

    private static func drum(_ file: String, _ name: String, _ pitch: Int, preset: String = "classic_kit", tail: Int = 2500) -> Def {
        Def(file: file, name: name, args: ["--engine", "drum.analog808style", "--preset", "preset:drum.analog808style:drums:\(preset)@v1.0",
                                           "--pitch", "\(pitch)", "--len-ms", "100", "--tail-ms", "\(tail)"])
    }
    private static func modal(_ file: String, _ name: String, preset: String, pitch: String = "C4", hz: Double? = nil, tail: Int = 15000) -> Def {
        var a = ["--engine", "synth.modal", "--preset", "preset:synth.modal:\(preset)@v1.0", "--pitch", pitch, "--len-ms", "100", "--tail-ms", "\(tail)"]
        if let hz { a += ["--params", "{\"freqHz\":\(hz)}"] }
        return Def(file: file, name: name, args: a)
    }
    private static func synth(_ file: String, _ name: String, _ params: String, pitch: String, len: Int, tail: Int) -> Def {
        Def(file: file, name: name, args: ["--engine", "synth.subtractive", "--params", params, "--pitch", pitch,
                                           "--len-ms", "\(len)", "--tail-ms", "\(tail)"])
    }

    static let banks: [Bank] = [
        Bank(title: "鼓组", space: 0.06, pads: [
            drum("kit-kick", "底鼓", 36), drum("kit-punch", "重底鼓", 35, preset: "punchy_kit"),
            drum("kit-snare", "军鼓", 38), drum("kit-clap", "拍手", 39),
            drum("kit-ch", "闭镲", 42), drum("kit-oh", "开镲", 46),
            drum("kit-rim", "边击", 37), drum("kit-crash", "吊镲", 49, tail: 5000),
        ]),
        Bank(title: "打击乐", space: 0.1, pads: [
            drum("perc-tomlo", "低嗵", 41), drum("perc-tomhi", "高嗵", 48),
            drum("perc-bongo", "手鼓", 60), drum("perc-conga", "康加", 63),
            drum("perc-shaker", "沙锤", 70), drum("perc-tamb", "铃鼓", 54),
            drum("perc-claves", "响棒", 75), drum("perc-ride", "叮叮镲", 51, tail: 5000),
        ]),
        Bank(title: "颂钵", space: 0.22, pads: [
            modal("bowl-c", "C 根", preset: "bowl:singing_bowl", pitch: "C3"),
            modal("bowl-d", "D 腹", preset: "bowl:singing_bowl", pitch: "D3"),
            modal("bowl-e", "E 脐", preset: "bowl:singing_bowl", pitch: "E3"),
            modal("bowl-f", "F 心", preset: "bowl:singing_bowl", pitch: "F3"),
            modal("bowl-g", "G 喉", preset: "bowl:singing_bowl", pitch: "G3"),
            modal("bowl-a", "A 眉", preset: "bowl:singing_bowl", pitch: "A3"),
            modal("bowl-b", "B 顶", preset: "bowl:singing_bowl", pitch: "B3"),
            modal("bowl-crystal", "水晶钵", preset: "bowl:crystal_bowl", pitch: "C4"),
        ]),
        Bank(title: "音叉", space: 0.15, pads: [
            modal("fork-om", "OM 136", preset: "fork:tuning_fork", hz: 136.1, tail: 12000),
            modal("fork-174", "174", preset: "fork:tuning_fork", hz: 174, tail: 12000),
            modal("fork-285", "285", preset: "fork:tuning_fork", hz: 285, tail: 12000),
            modal("fork-396", "396", preset: "fork:tuning_fork", hz: 396, tail: 12000),
            modal("fork-417", "417", preset: "fork:tuning_fork", hz: 417, tail: 12000),
            modal("fork-528", "528", preset: "fork:tuning_fork", hz: 528, tail: 12000),
            modal("fork-639", "639", preset: "fork:tuning_fork", hz: 639, tail: 12000),
            modal("fork-741", "741", preset: "fork:tuning_fork", hz: 741, tail: 12000),
        ]),
        Bank(title: "钟与氛围", space: 0.3, pads: [
            modal("amb-chime", "管钟", preset: "chime:tubular_chime", pitch: "G5", tail: 7000),
            modal("amb-bell", "寺钟", preset: "bell:temple_bell", pitch: "C4", tail: 10000),
            modal("amb-gong", "风锣", preset: "gong:wind_gong", pitch: "A1", tail: 14000),
            modal("amb-deepbowl", "深钵", preset: "bowl:deep_bowl", pitch: "F2", tail: 18000),
            synth("amb-drone", "低吟", #"{"osc.wave":"saw","osc2.wave":"saw","osc2.level":0.5,"osc2.detuneCents":9,"filter.cutoffHz":700,"filter.envAmount":0.5,"amp.attackMs":900,"amp.sustain":0.85,"amp.releaseMs":2500,"gainDb":-10}"#, pitch: "A1", len: 4000, tail: 2600),
            synth("amb-wind", "风声", #"{"osc.wave":"noise","filter.type":"bp","filter.cutoffHz":500,"filter.resonance":0.6,"filter.envAmount":2.5,"fenv.attackMs":2200,"fenv.sustain":0.6,"amp.attackMs":1500,"amp.sustain":0.8,"amp.releaseMs":2000,"gainDb":-12}"#, pitch: "C3", len: 3500, tail: 2200),
            synth("amb-waves", "海浪", #"{"osc.wave":"noise","filter.cutoffHz":900,"filter.resonance":0.1,"filter.envAmount":1.5,"fenv.attackMs":2500,"fenv.decayMs":3000,"fenv.sustain":0.2,"amp.attackMs":2200,"amp.decayMs":2500,"amp.sustain":0.4,"amp.releaseMs":3000,"gainDb":-9}"#, pitch: "C3", len: 4500, tail: 3200),
            synth("amb-rain", "雨声", #"{"osc.wave":"noise","filter.type":"hp","filter.cutoffHz":2500,"filter.resonance":0.2,"filter.envAmount":0,"amp.attackMs":600,"amp.sustain":0.7,"amp.releaseMs":1500,"gainDb":-16}"#, pitch: "C3", len: 5000, tail: 1600),
        ]),
    ]

    private static var configURL: URL { dir.appendingPathComponent("pads.json") }
    private static var spaceURL: URL { dir.appendingPathComponent("space.json") }

    static func load() -> [[Pad]] {
        var out: [[Pad]] = banks.enumerated().map { b, bank in
            bank.pads.enumerated().map { i, d in
                let u = dir.appendingPathComponent("\(d.file).wav")
                return Pad(id: i, bank: b, name: d.name, url: FileManager.default.fileExists(atPath: u.path) ? u : nil, key: keys[i])
            }
        }
        if let data = try? Data(contentsOf: configURL),
           let saved = try? JSONDecoder().decode([[SavedPad]].self, from: data) {
            for (b, row) in saved.enumerated() where b < out.count {
                for (i, sp) in row.enumerated() where i < out[b].count {
                    out[b][i].settings = sp.settings
                    if let p = sp.path, FileManager.default.fileExists(atPath: p) {
                        out[b][i].url = URL(fileURLWithPath: p)
                        out[b][i].name = sp.name
                    }
                }
            }
        }
        return out
    }

    private struct SavedPad: Codable {
        var name: String
        var path: String?
        var settings: PadSettings
    }

    static func save(_ banks: [[Pad]]) {
        let rows = banks.map { $0.map { p -> SavedPad in
            let isUser = p.url.map { $0.lastPathComponent.hasPrefix("user-") } ?? false
            return SavedPad(name: p.name, path: isUser ? p.url?.path : nil, settings: p.settings)
        } }
        if let d = try? JSONEncoder().encode(rows) { try? d.write(to: configURL) }
    }

    static func loadSpace() -> [Double] {
        if let d = try? Data(contentsOf: spaceURL), let v = try? JSONDecoder().decode([Double].self, from: d), v.count == banks.count { return v }
        return banks.map(\.space)
    }

    static func saveSpace(_ v: [Double]) {
        if let d = try? JSONEncoder().encode(v) { try? d.write(to: spaceURL) }
    }

    static func importSample(_ src: URL, bank: Int, pad: Int) -> URL? {
        let dst = dir.appendingPathComponent("user-\(bank)-\(pad)-\(src.lastPathComponent)")
        try? FileManager.default.removeItem(at: dst)
        do { try FileManager.default.copyItem(at: src, to: dst) } catch { return nil }
        return dst
    }

    /// Render missing default sounds with the bundled engine (4 at a time).
    static func ensureDefaults() async {
        guard let bin = AppSettings.agbBinary else { return }
        let all = banks.flatMap(\.pads)
        let missing = all.filter { !FileManager.default.fileExists(atPath: dir.appendingPathComponent("\($0.file).wav").path) }
        guard !missing.isEmpty else { return }
        let target = dir
        await withTaskGroup(of: Void.self) { g in
            var it = missing.makeIterator()
            func next() -> Def? { it.next() }
            for _ in 0..<4 {
                guard let d = next() else { break }
                g.addTask { render(d, bin: bin, dir: target) }
            }
            while await g.next() != nil {
                if let d = next() { g.addTask { render(d, bin: bin, dir: target) } }
            }
        }
    }

    private static func render(_ d: Def, bin: URL, dir: URL) {
        let p = Process()
        p.executableURL = bin
        p.arguments = ["oneshot"] + d.args + ["--out", dir.appendingPathComponent("\(d.file).wav").path]
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        try? p.run()
        p.waitUntilExit()
    }
}
