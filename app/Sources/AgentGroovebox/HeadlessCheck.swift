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
            // AGB_HEADLESS_SONG=<path>: play an .agbsong instead of a mock performance.
            if let song = ProcessInfo.processInfo.environment["AGB_HEADLESS_SONG"] {
                session.open(song: URL(fileURLWithPath: song))
                try? await Task.sleep(for: .seconds(3))
                try? console.audio.startRecording(to: rec)
                try? await Task.sleep(for: .seconds(12))
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
                    "recording": analyse(rec),
                    "log": session.log,
                ]
                if let d = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]) {
                    try? d.write(to: URL(fileURLWithPath: out))
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
        let view = ZStack {
            InkBackground()
            StageScreen(session: session, console: console, onNew: {}, onShelf: {}, onSettings: {}, onContinue: { _ in })
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
