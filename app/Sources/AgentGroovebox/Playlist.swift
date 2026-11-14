import AppKit
import SwiftUI

struct PlaylistItem: Identifiable, Equatable {
    let url: URL
    var title: String
    var detail = ""
    var id: String { url.path }
}

/// A list of `.agbsong` files played one after another. The next song comes in on a
/// bar line of the current one (see `Session.cue`), by hand or when the song ends.
@MainActor
final class Playlist: ObservableObject {
    @Published private(set) var items: [PlaylistItem] = []
    /// Go to the next song when the current one ends.
    @Published var auto: Bool { didSet { if persistent { UserDefaults.standard.set(auto, forKey: "playlist.auto") } } }
    /// Switch on a bar line instead of at once.
    @Published var aligned: Bool { didSet { if persistent { UserDefaults.standard.set(aligned, forKey: "playlist.aligned") } } }
    /// After the last song, start again from the first.
    @Published var loop: Bool { didSet { if persistent { UserDefaults.standard.set(loop, forKey: "playlist.loop") } } }

    let session: Session
    private var timer: Timer?
    private var advancedPiece = -1
    private static let pathsKey = "playlist.paths"
    /// Remember the list between launches (off for self-tests).
    private let persistent: Bool

    init(session: Session, persistent: Bool = true) {
        self.session = session
        self.persistent = persistent
        let d = UserDefaults.standard
        auto = d.object(forKey: "playlist.auto") as? Bool ?? true
        aligned = d.object(forKey: "playlist.aligned") as? Bool ?? true
        loop = d.bool(forKey: "playlist.loop")
        if !persistent {
        } else if let paths = d.stringArray(forKey: Self.pathsKey) {
            add(paths.map { URL(fileURLWithPath: $0) }.filter { FileManager.default.fileExists(atPath: $0.path) }, save: false)
        } else if let dir = Self.bundledSongs {
            addFolder(dir)
        }
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.tick() }
        }
    }

    /// The `songs` folder next to the app when it runs from the project (dist/…app or `swift run`).
    private static var bundledSongs: URL? {
        let fm = FileManager.default
        var dir = Bundle.main.bundleURL
        for _ in 0..<5 {
            dir = dir.deletingLastPathComponent()
            let songs = dir.appendingPathComponent("songs")
            if fm.fileExists(atPath: songs.path) { return songs }
        }
        return nil
    }

    // MARK: contents

    private func same(_ a: URL, _ b: URL?) -> Bool { a.standardizedFileURL.path == b?.standardizedFileURL.path }

    func add(_ urls: [URL], save: Bool = true) {
        for url in urls where !items.contains(where: { same($0.url, url) }) {
            items.append(PlaylistItem(url: url, title: url.deletingPathExtension().lastPathComponent))
            Task.detached(priority: .utility) { [weak self] in
                guard let meta = SongMeta.read(url) else { return }
                await self?.fill(url, meta)
            }
        }
        if save { persist() }
    }

    func addFolder(_ dir: URL) {
        let files = (try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
        add(files.filter { $0.pathExtension == "agbsong" }.sorted { $0.lastPathComponent < $1.lastPathComponent })
    }

    func remove(_ item: PlaylistItem) {
        items.removeAll { $0.id == item.id }
        persist()
    }

    func move(_ item: PlaylistItem, by offset: Int) {
        guard let i = items.firstIndex(of: item), items.indices.contains(i + offset) else { return }
        items.swapAt(i, i + offset)
        persist()
    }

    private func fill(_ url: URL, _ meta: SongMeta) {
        guard let i = items.firstIndex(where: { same($0.url, url) }) else { return }
        items[i].title = meta.title
        items[i].detail = meta.detail
    }

    private func persist() {
        guard persistent else { return }
        UserDefaults.standard.set(items.map(\.url.path), forKey: Self.pathsKey)
    }

    /// Choose files or folders to add.
    func addFromPanel() {
        let panel = NSOpenPanel()
        panel.title = "加入歌单"
        panel.message = "选择 .agbsong 文件，或放着这些文件的文件夹"
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = true
        guard panel.runModal() == .OK else { return }
        for url in panel.urls {
            var isDir: ObjCBool = false
            let exists = FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir)
            if exists, isDir.boolValue, url.pathExtension != "src", !FileManager.default.fileExists(atPath: url.appendingPathComponent("song.json").path) {
                addFolder(url)
            } else {
                add([url])
            }
        }
    }

    // MARK: transport

    var currentIndex: Int? { items.firstIndex { same($0.url, session.songURL) } }
    var cuedIndex: Int? { items.firstIndex { same($0.url, session.cue?.url) } }

    var nextIndex: Int? {
        guard !items.isEmpty else { return nil }
        guard let i = currentIndex else { return 0 }
        return i + 1 < items.count ? i + 1 : (loop ? 0 : nil)
    }

    var previousIndex: Int? {
        guard !items.isEmpty, let i = currentIndex else { return nil }
        return i > 0 ? i - 1 : (loop ? items.count - 1 : nil)
    }

    func play(_ index: Int, atEnd: Bool = false) {
        guard items.indices.contains(index) else { return }
        session.cue(song: items[index].url, title: items[index].title, aligned: aligned, atEnd: atEnd)
    }

    func next() { if let i = nextIndex { play(i) } }
    func previous() { if let i = previousIndex { play(i) } }

    /// Ten times a second: keep the list in step with what is on stage, and move on when a song ends.
    private func tick() {
        if let url = session.songURL, currentIndex == nil, url.pathExtension == "agbsong" { add([url]) }
        guard auto, session.cue == nil, advancedPiece != session.pieceSerial,
              let left = session.secondsToEnd, let n = nextIndex else { return }
        // Aligned: prepare the next song just before the end, so that its first bar lands on the last bar line.
        if left <= (aligned ? Session.cuePrep + 0.65 : 0) {
            advancedPiece = session.pieceSerial
            play(n, atEnd: true)
        }
    }
}

/// Title, key, tempo and length read from a song file without playing it.
struct SongMeta {
    let title: String
    let detail: String

    static func read(_ url: URL) -> SongMeta? {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) else { return nil }
        let data: Data?
        if isDir.boolValue {
            data = try? Data(contentsOf: url.appendingPathComponent("song.json"))
        } else {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
            p.arguments = ["-p", url.path, "song.json"]
            let out = Pipe()
            p.standardOutput = out
            p.standardError = FileHandle.nullDevice
            guard (try? p.run()) != nil else { return nil }
            data = out.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
        }
        guard let data, let song = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        let meta = song["meta"] as? [String: Any] ?? [:]
        let time = song["time"] as? [String: Any] ?? [:]
        let tuning = song["tuning"] as? [String: Any] ?? [:]
        var parts: [String] = []
        if let k = tuning["key"] as? String, let label = KeyName.label(tonic: k, scale: tuning["scale"] as? String, transpose: 0) { parts.append(label) }
        let bpm = (time["bpm"] as? NSNumber)?.doubleValue ?? 0
        if bpm > 0 { parts.append("\(Int(bpm.rounded())) BPM") }
        if bpm > 0, let ticks = (time["lengthTicks"] as? NSNumber)?.doubleValue, let ppqn = (time["ppqn"] as? NSNumber)?.doubleValue, ppqn > 0 {
            let sec = Int((ticks / ppqn * 60 / bpm).rounded())
            parts.append(String(format: "%d:%02d", sec / 60, sec % 60))
        }
        return SongMeta(title: meta["title"] as? String ?? url.deletingPathExtension().lastPathComponent, detail: parts.joined(separator: " · "))
    }
}

// MARK: - views

/// The list itself, with the switches (shown in a popover).
struct PlaylistPanel: View {
    @ObservedObject var playlist: Playlist
    @ObservedObject var session: Session

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Eyebrow("歌单", color: Theme.accent)
                Spacer()
                GhostButton(title: "添加", systemImage: "plus") { playlist.addFromPanel() }
            }
            if playlist.items.isEmpty {
                Text("还没有歌曲。点“添加”选择 .agbsong 文件或文件夹。")
                    .font(.system(size: 12)).foregroundStyle(Theme.dim).padding(.vertical, 18)
            }
            ScrollView {
                VStack(spacing: 4) {
                    ForEach(Array(playlist.items.enumerated()), id: \.element.id) { i, item in
                        PlaylistRow(index: i, item: item, state: state(i),
                                    onPlay: { playlist.play(i) }, onRemove: { playlist.remove(item) },
                                    onUp: { playlist.move(item, by: -1) }, onDown: { playlist.move(item, by: 1) })
                    }
                }
            }
            .frame(maxHeight: 320)
            Rectangle().fill(Theme.hairline).frame(height: 1)
            Toggle("自动播放下一首", isOn: $playlist.auto)
            Toggle("对齐拍子：在小节线上切入", isOn: $playlist.aligned)
            Toggle("循环歌单", isOn: $playlist.loop)
            Text(playlist.aligned
                 ? "手动切歌时，下一首从当前曲子的下一条小节线开始；自动切歌时，紧接最后一小节。"
                 : "切歌立即生效，不等小节线。")
                .font(.system(size: 11)).foregroundStyle(Theme.faint).fixedSize(horizontal: false, vertical: true)
        }
        .toggleStyle(.switch).controlSize(.small)
        .font(.system(size: 12, design: .rounded)).foregroundStyle(Theme.text)
        .padding(16)
        .frame(width: 380)
        .preferredColorScheme(.dark)
    }

    private func state(_ i: Int) -> PlaylistRow.State {
        if playlist.cuedIndex == i { return .cued }
        if playlist.currentIndex == i, session.isRunning || session.status == .finished { return .playing }
        return .idle
    }
}

struct PlaylistRow: View {
    enum State { case idle, playing, cued }
    let index: Int
    let item: PlaylistItem
    let state: State
    let onPlay: () -> Void
    let onRemove: () -> Void
    let onUp: () -> Void
    let onDown: () -> Void
    @SwiftUI.State private var hover = false

    var body: some View {
        HStack(spacing: 10) {
            Group {
                switch state {
                case .playing: Image(systemName: "waveform").foregroundStyle(Theme.accent)
                case .cued: Image(systemName: "forward.end.fill").foregroundStyle(Theme.color(2))
                case .idle: Text("\(index + 1)").foregroundStyle(Theme.faint).monospacedDigit()
                }
            }
            .font(.system(size: 12, weight: .medium, design: .rounded))
            .frame(width: 20)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).font(.system(size: 13, weight: .medium)).foregroundStyle(state == .idle ? Theme.text : Theme.accent).lineLimit(1)
                Text(state == .cued ? "下一条小节线切入" : item.detail)
                    .font(.system(size: 11, design: .rounded)).foregroundStyle(state == .cued ? Theme.color(2) : Theme.dim).lineLimit(1)
            }
            Spacer(minLength: 4)
            if hover {
                rowButton("chevron.up", action: onUp)
                rowButton("chevron.down", action: onDown)
                rowButton("xmark", action: onRemove)
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 7)
        .background(Color.white.opacity(hover ? 0.09 : state == .idle ? 0.03 : 0.07), in: RoundedRectangle(cornerRadius: 9))
        .contentShape(RoundedRectangle(cornerRadius: 9))
        .onTapGesture(perform: onPlay)
        .onHover { hover = $0 }
        .help(state == .playing ? "正在播放" : "点击切到这一首")
    }

    private func rowButton(_ icon: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: 10, weight: .semibold)).foregroundStyle(Theme.dim)
                .frame(width: 20, height: 20).background(Color.white.opacity(0.08), in: Circle())
        }
        .buttonStyle(.plain)
    }
}

/// Previous / playlist / next, for the stage's top bar and the idle screen.
struct PlaylistControls: View {
    @ObservedObject var playlist: Playlist
    @ObservedObject var session: Session
    var transport = true
    @State private var showList = false

    var body: some View {
        HStack(spacing: 6) {
            if transport, playlist.previousIndex != nil {
                iconButton("backward.end.fill") { playlist.previous() }
                    .keyboardShortcut(.leftArrow, modifiers: .command)
                    .help("上一首（⌘←）")
            }
            GhostButton(title: playlist.items.isEmpty ? "歌单" : "歌单 \((playlist.currentIndex.map { "\($0 + 1)/" } ?? ""))\(playlist.items.count)", systemImage: "music.note.list") {
                showList.toggle()
            }
            .popover(isPresented: $showList, arrowEdge: .bottom) { PlaylistPanel(playlist: playlist, session: session) }
            if transport, playlist.nextIndex != nil {
                iconButton("forward.end.fill") { playlist.next() }
                    .keyboardShortcut(.rightArrow, modifiers: .command)
                    .help(playlist.aligned ? "下一首：在下一条小节线切入（⌘→）" : "下一首：立即切换（⌘→）")
            }
        }
    }

    private func iconButton(_ icon: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: 12, weight: .medium)).foregroundStyle(Theme.dim)
                .frame(width: 30, height: 30).background(Color.white.opacity(0.05), in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
    }
}

/// "Next: … on the bar line", shown while a song is cued.
struct CueBanner: View {
    @ObservedObject var session: Session

    var body: some View {
        if let c = session.cue {
            HStack(spacing: 8) {
                Image(systemName: "forward.end.fill")
                Text("下一首《\(c.title)》在小节线切入")
                Button("取消") { session.cancelCue() }.buttonStyle(.plain).foregroundStyle(Theme.dim)
            }
            .font(.system(size: 12, weight: .medium, design: .rounded)).foregroundStyle(Theme.color(2))
            .padding(.horizontal, 14).padding(.vertical, 7)
            .background(Color.black.opacity(0.6), in: Capsule())
            .overlay(Capsule().strokeBorder(Theme.color(2).opacity(0.5)))
            .transition(.opacity)
        }
    }
}
