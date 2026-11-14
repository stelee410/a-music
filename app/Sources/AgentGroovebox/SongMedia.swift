import AppKit
import AVFoundation
import SwiftUI

// MARK: - Key names

enum KeyName {
    private static let pitchClass: [Character: Int] = ["C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11]
    private static let names = ["C", "D♭", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"]
    private static let scales = [
        "major": "大调", "minor": "小调", "dorian": "多利亚", "phrygian": "弗里几亚", "lydian": "利底亚",
        "mixolydian": "混合利底亚", "locrian": "洛克里亚", "pentatonic_major": "大调五声", "pentatonic_minor": "小调五声",
    ]

    /// "Eb" + "major" transposed by 2 → "F 大调". `nil` when the tonic cannot be read.
    static func label(tonic: String, scale: String?, transpose: Int) -> String? {
        guard let first = tonic.uppercased().first, var pc = pitchClass[first] else { return nil }
        for ch in tonic.dropFirst() {
            if ch == "#" || ch == "♯" { pc += 1 } else if ch == "b" || ch == "♭" { pc -= 1 }
        }
        let name = names[((pc + transpose) % 12 + 12) % 12]
        guard let scale, !scale.isEmpty else { return name }
        return "\(name) \(scales[scale] ?? scale)"
    }

    static func signed(_ n: Int) -> String { n > 0 ? "+\(n)" : "\(n)" }
}

// MARK: - Stage media (cover / video shipped inside the song file)

enum StageMode: String, CaseIterable, Identifiable {
    case cover = "封面", orbit = "圆环", video = "视频"
    var id: String { rawValue }
}

/// Segmented switch shown only for the views the song actually has.
struct StageModePicker: View {
    let modes: [StageMode]
    @Binding var mode: StageMode
    var body: some View {
        HStack(spacing: 2) {
            ForEach(modes) { m in
                Button { withAnimation(.easeInOut(duration: 0.35)) { mode = m } } label: {
                    Text(m.rawValue).font(Theme.label(11))
                        .foregroundStyle(mode == m ? Theme.ink : Theme.dim)
                        .padding(.horizontal, 12).padding(.vertical, 5)
                        .background(mode == m ? Theme.accent : Color.clear, in: Capsule())
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(3)
        .background(Color.white.opacity(0.06), in: Capsule())
    }
}

struct CoverView: View {
    let url: URL
    var body: some View {
        if let img = NSImage(contentsOf: url) {
            Image(nsImage: img).resizable().interpolation(.high).aspectRatio(contentMode: .fit)
                .clipShape(RoundedRectangle(cornerRadius: 18))
                .shadow(color: .black.opacity(0.5), radius: 30, y: 12)
        } else {
            VStack(spacing: 8) {
                Image(systemName: "photo").font(.system(size: 28)).foregroundStyle(Theme.faint)
                Text("封面无法显示").font(Theme.voice(13)).foregroundStyle(Theme.dim)
            }
        }
    }
}

/// Plays the song's own video, muted (the music comes from the engine).
/// `followSong`: video time tracks song time; otherwise it simply loops.
struct SongVideoView: NSViewRepresentable {
    let url: URL
    let followSong: Bool
    /// Seconds into the song, or nil while nothing is playing.
    let songSeconds: () -> Double?
    var songRate: () -> Double = { 1 }

    final class PlayerView: NSView {
        let player = AVQueuePlayer()
        var looper: AVPlayerLooper?
        var timer: Timer?
        override init(frame: NSRect) {
            super.init(frame: frame)
            wantsLayer = true
            let l = AVPlayerLayer(player: player)
            l.videoGravity = .resizeAspect
            l.cornerRadius = 18
            l.masksToBounds = true
            layer = l
        }
        required init?(coder: NSCoder) { fatalError() }
        deinit { timer?.invalidate() }
    }

    func makeNSView(context: Context) -> PlayerView {
        let v = PlayerView()
        let item = AVPlayerItem(url: url)
        v.player.isMuted = true
        if followSong {
            v.player.insert(item, after: nil)
            v.player.actionAtItemEnd = .pause
        } else {
            v.looper = AVPlayerLooper(player: v.player, templateItem: item)
        }
        let seconds = songSeconds
        let follow = followSong
        let rate = songRate
        v.timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak v] _ in
            guard let v else { return }
            guard let t = seconds() else { v.player.pause(); return }
            if follow {
                let now = v.player.currentTime().seconds
                if abs(now - t) > 0.35 {
                    v.player.seek(to: CMTime(seconds: t, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero)
                }
            }
            if follow {
                let r = Float(rate())
                if abs(v.player.rate - r) > 0.005 { v.player.rate = r }
            } else if v.player.rate == 0 {
                v.player.play()
            }
        }
        return v
    }

    func updateNSView(_ v: PlayerView, context: Context) {}

    static func dismantleNSView(_ v: PlayerView, coordinator: ()) {
        v.timer?.invalidate()
        v.player.pause()
    }
}

// MARK: - Opening files from Finder

/// Songs opened by double-click (or `open file.agbsong`) arrive through the app
/// delegate, possibly before the window exists.
enum SongOpener {
    static let notification = Notification.Name("agb.openSong")
    static var pending: URL?

    static func open(_ url: URL) {
        pending = url
        NotificationCenter.default.post(name: notification, object: url)
    }

    /// Ask for a song: an `.agbsong` file, or a source folder with the same layout.
    @MainActor static func choose() -> URL? {
        let panel = NSOpenPanel()
        panel.title = "打开 .agbsong"
        panel.message = "选择一个 .agbsong 文件（也可以选创作用的源文件夹）"
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        return panel.runModal() == .OK ? panel.url : nil
    }

    static func takePending() -> URL? {
        defer { pending = nil }
        return pending
    }
}
