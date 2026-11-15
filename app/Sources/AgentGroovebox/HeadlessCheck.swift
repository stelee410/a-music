import AppKit
import AVFoundation
import SwiftUI

/// `AGB_HEADLESS_CHECK=<out.json>`: exercises session + console + audio graph
/// without showing a window (no focus stealing), records what the app would
/// play, analyses the recording, writes a JSON report and quits.
@MainActor
enum HeadlessCheck {
    static func say(_ s: String) { FileHandle.standardError.write(Data("[headless] \(s)\n".utf8)) }

    static func run(out: String) {
        say("run")
        NSApp.setActivationPolicy(.accessory)
        for w in NSApp.windows { w.orderOut(nil) }
        let session = Session(silentAudio: true)
        say("session")
        let console = Console(session: session)
        say("console")
        var o = PerformOptions()
        o.mock = true
        o.phrases = 6
        o.bars = 4
        o.brief = "headless check"
        let rec = FileManager.default.temporaryDirectory.appendingPathComponent("agb-headless-\(ProcessInfo.processInfo.processIdentifier).caf")
        say("spawning task")
        Task { @MainActor in
            say("task begins")
            for w in NSApp.windows { w.orderOut(nil) }
            let t0 = Date()
            while !console.padsReady && Date().timeIntervalSince(t0) < 90 { try? await Task.sleep(for: .milliseconds(200)) }
            say("pads ready in \(String(format: "%.1f", Date().timeIntervalSince(t0))) s")
            // AGB_HEADLESS_ANIM=<dir>: render frames of every loading scene and quit.
            if let dir = ProcessInfo.processInfo.environment["AGB_HEADLESS_ANIM"] {
                let title = ProcessInfo.processInfo.environment["AGB_HEADLESS_ANIM_TITLE"] ?? "雾散时"
                for style in LoadingStyle.allCases { for p in [0.20, 0.50, 0.78, 0.86, 0.93] {
                    let view = ZStack { Color.black; LoadingScene(progress: p, style: style, seed: LoadingScene.seed(title)) }
                        .frame(width: 800, height: 500).environment(\.colorScheme, .dark)
                    let host = NSHostingView(rootView: view)
                    host.frame = NSRect(x: 0, y: 0, width: 800, height: 500)
                    let w = NSWindow(contentRect: NSRect(x: -20000, y: -20000, width: 800, height: 500), styleMask: [.borderless], backing: .buffered, defer: false)
                    w.contentView = host
                    w.orderBack(nil)
                    host.layoutSubtreeIfNeeded()
                    try? await Task.sleep(for: .milliseconds(300))
                    if let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) {
                        host.cacheDisplay(in: host.bounds, to: rep)
                        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: "\(dir)/anim-\(style.rawValue)-\(Int(p * 100)).png"))
                    }
                    w.orderOut(nil)
                } }
                exit(0)
            }
            // AGB_HEADLESS_SONG=<path>: play an .agbsong instead of a mock performance.
            if let song = ProcessInfo.processInfo.environment["AGB_HEADLESS_SONG"] {
                // AGB_HEADLESS_SONG_MOCK=1 answers with the offline performer instead of the real model.
                session.open(song: URL(fileURLWithPath: song), mock: ProcessInfo.processInfo.environment["AGB_HEADLESS_SONG_MOCK"] != nil, animated: false)
                try? await Task.sleep(for: .seconds(3))
                try? console.audio.startRecording(to: rec)
                session.say(ProcessInfo.processInfo.environment["AGB_HEADLESS_SAY"] ?? "踩镲停一下")
                if let n = Int(ProcessInfo.processInfo.environment["AGB_HEADLESS_TRANSPOSE"] ?? "") { session.setTranspose(n) }
                try? await Task.sleep(for: .seconds(Double(ProcessInfo.processInfo.environment["AGB_HEADLESS_SECONDS"] ?? "") ?? 14))
                // AGB_HEADLESS_AUTONEXT=<path>: a playlist of the song and this one; wait for the song to end and the next to start by itself.
                var autoReport: [String: Any] = [:]
                if let next = ProcessInfo.processInfo.environment["AGB_HEADLESS_AUTONEXT"] {
                    let playlist = Playlist(session: session, persistent: false)
                    playlist.auto = true; playlist.aligned = true
                    playlist.add([URL(fileURLWithPath: song), URL(fileURLWithPath: next)])
                    let before = session.pieceSerial, bar = Double(session.barTicks), end = Double(session.renderedUntil)
                    var lastTick = -1.0
                    let t0 = Date()
                    while session.pieceSerial == before, Date().timeIntervalSince(t0) < 240 {
                        lastTick = session.playheadTick ?? lastTick
                        try? await Task.sleep(for: .milliseconds(5))
                    }
                    autoReport = ["switched": session.pieceSerial != before, "lastBarPositionBeforeSwitch": lastTick / bar + 1,
                                  "renderedBarsWhenAsked": end / bar, "items": playlist.items.map { "\($0.title) — \($0.detail)" },
                                  "currentIndex": playlist.currentIndex ?? -1]
                    try? await Task.sleep(for: .seconds(5))
                }
                // AGB_HEADLESS_NEXT=<path>: switch to another song on a bar line and report where it landed.
                var cueReport: [String: Any] = [:]
                if let next = ProcessInfo.processInfo.environment["AGB_HEADLESS_NEXT"] {
                    let before = session.songURL
                    let bar = Double(session.barTicks), fromTick = session.playheadTick ?? -1
                    session.cue(song: URL(fileURLWithPath: next))
                    snapshot(session: session, console: console, to: out.replacingOccurrences(of: ".json", with: "-cue.png"))
                    var lastTick = fromTick
                    let t0 = Date()
                    while session.songURL == before, Date().timeIntervalSince(t0) < 20 {
                        lastTick = session.playheadTick ?? lastTick
                        try? await Task.sleep(for: .milliseconds(5))
                    }
                    cueReport = ["askedAtBar": fromTick / bar + 1, "lastBarPositionBeforeSwitch": lastTick / bar + 1,
                                 "waitedSeconds": Date().timeIntervalSince(t0), "switched": session.songURL != before,
                                 "newPlayheadTickAtSwitch": session.playheadTick ?? -1]
                    try? await Task.sleep(for: .seconds(6))
                }
                // AGB_HEADLESS_CONTINUE=<out.agbsong>: after the song has ended, let the (offline) performer go on
                // for two phrases, talk to it, then save the whole thing as a new song.
                var continueReport: [String: Any] = [:]
                if let saveTo = ProcessInfo.processInfo.environment["AGB_HEADLESS_CONTINUE"] {
                    let t0 = Date()
                    while session.status != .finished, Date().timeIntervalSince(t0) < 60 { try? await Task.sleep(for: .milliseconds(100)) }
                    while (session.secondsToEnd ?? 0) > 0, Date().timeIntervalSince(t0) < 90 { try? await Task.sleep(for: .milliseconds(100)) }
                    let songBars = session.renderedUntil / max(session.barTicks, 1)
                    continueReport = ["songEnded": "\(session.status)", "songBars": songBars, "canContinue": session.canContinue, "canExportBefore": session.canExport]
                    var o = PerformOptions(brief: ""); o.bars = 4; o.mock = true
                    session.continueCreating(o, more: 2)
                    try? await Task.sleep(for: .seconds(4))
                    continueReport["statusWhileContinuing"] = "\(session.status)"
                    continueReport["canTalkWhileContinuing"] = session.canTalk
                    continueReport["songURLCleared"] = session.songURL == nil
                    session.say("鼓再密一点")
                    let t1 = Date()
                    while session.status != .finished, Date().timeIntervalSince(t1) < 90 {
                        if case .failed = session.status { break }
                        try? await Task.sleep(for: .milliseconds(200))
                    }
                    continueReport["statusAfter"] = "\(session.status)"
                    continueReport["barsAfter"] = session.renderedUntil / max(session.barTicks, 1)
                    continueReport["phrasesAfter"] = session.phrases.count
                    session.exportSong(to: URL(fileURLWithPath: saveTo))
                    let t2 = Date()
                    while session.exporting, Date().timeIntervalSince(t2) < 60 { try? await Task.sleep(for: .milliseconds(100)) }
                    continueReport["exported"] = FileManager.default.fileExists(atPath: saveTo)
                }
                snapshot(session: session, console: console, to: out.replacingOccurrences(of: ".json", with: "-stage.png"))
                let playhead = session.playheadTick ?? -1
                console.audio.stopRecording()
                let report: [String: Any] = [
                    "status": "\(session.status)",
                    "activity": session.activity,
                    "playheadTick": playhead,
                    "renderedUntil": session.renderedUntil,
                    "tracks": session.tracks.map(\.id),
                    "phrases": session.phrases.map { ["title": $0.title, "tick": $0.tick] },
                    "feed": session.feed.map { "\($0.kind): \($0.text) \($0.detail)" },
                    "events": session.events.count,
                    "canTalk": session.canTalk,
                    "key": session.currentKeyLabel ?? "-",
                    "transpose": session.currentTranspose,
                    "cover": session.coverURL?.path ?? "-",
                    "video": session.videoURL?.path ?? "-",
                    "recording": analyse(rec),
                    "cue": cueReport,
                    "auto": autoReport,
                    "continue": continueReport,
                    "title": session.brief,
                    "log": session.log,
                ]
                if let d = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]) {
                    try? d.write(to: URL(fileURLWithPath: out))
                }
                if let keep = ProcessInfo.processInfo.environment["AGB_HEADLESS_KEEP_REC"] {
                    try? FileManager.default.removeItem(atPath: keep)
                    try? FileManager.default.copyItem(at: rec, to: URL(fileURLWithPath: keep))
                }
                session.stop()
                try? FileManager.default.removeItem(at: rec)
                exit(0)
            }
            session.start(o)
            say("started: \(session.status)")
            try? await Task.sleep(for: .seconds(4))
            try? console.audio.startRecording(to: rec)
            try? await Task.sleep(for: .seconds(3))
            session.say("再安静一点")
            console.binOn = true; console.beatHz = 6; console.carrierHz = 200
            console.toneOn = true; console.toneHz = 528
            console.filter = 0.6
            console.tuningA4 = 432
            if let first = session.tracks.first?.id { console.setVolume(first, 0.5) }
            try? await Task.sleep(for: .seconds(1))
            console.bank = 0
            for i in [0, 2, 1, 3] { console.hit(i); try? await Task.sleep(for: .milliseconds(300)) }
            console.bank = 2
            console.hit(0)
            console.updateSelected { $0.pitch = 5; $0.decay = 0.6 }
            console.hit(0)
            console.hit(7)
            console.bank = 3
            console.hit(5)
            console.requestTempo(100)
            try? await Task.sleep(for: .seconds(9))
            snapshot(session: session, console: console, to: out.replacingOccurrences(of: ".json", with: "-stage.png"))
            say("collecting")
            let playhead = session.playheadTick ?? -1
            console.audio.stopRecording()
            let analysis = analyse(rec)
            session.finish()
            try? await Task.sleep(for: .seconds(1))
            let report: [String: Any] = [
                "status": "\(session.status)",
                "playheadTick": playhead,
                "tracks": session.tracks.map(\.id),
                "phrases": session.phrases.map { ["title": $0.title, "tick": $0.tick, "answer": $0.answer] },
                "feed": session.feed.map { "\($0.kind): \($0.text) \($0.detail)" },
                "pads": console.banks.enumerated().map { b, row in "\(PadBank.banks[b].title): " + row.map { "\($0.name)\($0.url != nil ? "" : "✗")" }.joined(separator: " ") },
                "ioBufferFrames": console.audio.ioBufferFrames,
                "humanState": console.summary(),
                "recording": analysis,
                "log": session.log,
            ]
            if let d = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]) {
                try? d.write(to: URL(fileURLWithPath: out))
            }
            session.stop()
            try? FileManager.default.removeItem(at: rec)
            exit(0)
        }
    }

    /// Render the stage in an off-screen window (never visible, no screen capture).
    static func snapshot(session: Session, console: Console, to path: String) {
        // With a song on stage, show the playlist controls too (the song and AGB_HEADLESS_NEXT).
        var playlist: Playlist?
        if let song = session.songURL {
            let p = Playlist(session: session, persistent: false)
            p.add([song] + (ProcessInfo.processInfo.environment["AGB_HEADLESS_NEXT"].map { [URL(fileURLWithPath: $0)] } ?? []))
            playlist = p
        }
        let view = ZStack {
            InkBackground()
            StageScreen(session: session, console: console, playlist: playlist, onNew: {}, onShelf: {}, onSettings: {}, onContinue: { _ in })
        }
        .frame(width: 1280, height: 820)
        .environment(\.colorScheme, .dark)
        let host = NSHostingView(rootView: view)
        host.frame = NSRect(x: 0, y: 0, width: 1280, height: 820)
        let w = NSWindow(contentRect: NSRect(x: -20000, y: -20000, width: 1280, height: 820),
                         styleMask: [.borderless], backing: .buffered, defer: false)
        w.contentView = host
        w.appearance = NSAppearance(named: .darkAqua)
        w.orderBack(nil)
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(1.0))
        if let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) {
            host.cacheDisplay(in: host.bounds, to: rep)
            if let png = rep.representation(using: .png, properties: [:]) {
                try? png.write(to: URL(fileURLWithPath: path))
            }
        }
        w.orderOut(nil)
    }

    /// RMS, peak, and left/right difference (binaural beats make L ≠ R).
    static func analyse(_ url: URL) -> [String: Any] {
        guard let f = try? AVAudioFile(forReading: url),
              let b = AVAudioPCMBuffer(pcmFormat: f.processingFormat, frameCapacity: AVAudioFrameCount(f.length)),
              (try? f.read(into: b)) != nil, let ch = b.floatChannelData else { return ["error": "unreadable"] }
        let n = Int(b.frameLength), c = Int(f.processingFormat.channelCount)
        var sq = 0.0, peak = 0.0, diff = 0.0
        for i in 0..<n {
            let l = Double(ch[0][i]), r = c > 1 ? Double(ch[1][i]) : l
            sq += l * l; peak = max(peak, abs(l), abs(r)); diff += (l - r) * (l - r)
        }
        return ["seconds": Double(n) / f.processingFormat.sampleRate, "rms": (sq / Double(max(n, 1))).squareRoot(),
                "peak": peak, "lrDiffRms": (diff / Double(max(n, 1))).squareRoot(), "channels": c]
    }
}
