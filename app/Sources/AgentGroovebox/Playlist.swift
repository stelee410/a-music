import AppKit
import SwiftUI

struct PlaylistItem: Identifiable, Equatable {
    let url: URL
    var title: String
    var detail = ""
    var id: String { url.path }
}

/// A playlist as it is stored: its name and the files in it.
struct SavedPlaylist: Codable, Equatable {
    var name: String
    var paths: [String]
}

/// The listener's playlists: lists of `.agbsong` files played one after another. The next song
/// comes in on a bar line of the current one (see `Session.cue`), by hand or when the song ends.
/// Nothing is put on a list except by the listener: opening a song only opens that song.
@MainActor
final class Playlist: ObservableObject {
    /// The songs of the playlist that is selected.
    @Published private(set) var items: [PlaylistItem] = []
    /// All playlists, and which one is selected.
    @Published private(set) var lists: [SavedPlaylist] = []
    @Published private(set) var selected = 0
    var name: String { lists.indices.contains(selected) ? lists[selected].name : "歌单" }
    /// Go to the next song when the current one ends.
    @Published var auto: Bool { didSet { if persistent { UserDefaults.standard.set(auto, forKey: "playlist.auto") } } }
    /// Switch on a bar line instead of at once.
    @Published var aligned: Bool { didSet { if persistent { UserDefaults.standard.set(aligned, forKey: "playlist.aligned") } } }
    /// After the last song, start again from the first.
    @Published var loop: Bool { didSet { if persistent { UserDefaults.standard.set(loop, forKey: "playlist.loop") } } }

    let session: Session
    private var timer: Timer?
    private var advancedPiece = -1
    private static let listsKey = "playlists.v2"
    private static let selectedKey = "playlists.selected"
    private static let defaultName = "我的歌单"
    /// Remember the list between launches (off for self-tests).
    private let persistent: Bool

    init(session: Session, persistent: Bool = true) {
        self.session = session
        self.persistent = persistent
        let d = UserDefaults.standard
        auto = d.object(forKey: "playlist.auto") as? Bool ?? true
        aligned = d.object(forKey: "playlist.aligned") as? Bool ?? true
        loop = d.bool(forKey: "playlist.loop")
        if persistent, let data = d.data(forKey: Self.listsKey), let saved = try? JSONDecoder().decode([SavedPlaylist].self, from: data), !saved.isEmpty {
            lists = saved
            selected = min(max(d.integer(forKey: Self.selectedKey), 0), saved.count - 1)
        } else {
            lists = [SavedPlaylist(name: Self.defaultName, paths: [])]
        }
        // The list an earlier version filled by itself from the songs folder is not the listener's: drop it.
        if persistent { d.removeObject(forKey: "playlist.paths") }
        load()
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.tick() }
        }
    }

    // MARK: playlists

    /// Show the selected playlist's songs.
    private func load() {
        items = []
        guard lists.indices.contains(selected) else { return }
        add(lists[selected].paths.map { URL(fileURLWithPath: $0) }.filter { FileManager.default.fileExists(atPath: $0.path) }, save: false)
    }

    func select(_ index: Int) {
        guard lists.indices.contains(index), index != selected else { return }
        selected = index
        load()
        persist()
    }

    /// A new, empty playlist, selected.
    func create(_ name: String) {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        lists.append(SavedPlaylist(name: n.isEmpty ? "歌单 \(lists.count + 1)" : n, paths: []))
        selected = lists.count - 1
        load()
        persist()
    }

    func rename(_ name: String) {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !n.isEmpty, lists.indices.contains(selected) else { return }
        lists[selected].name = n
        persist()
    }

    /// Delete the selected playlist (the files stay where they are). There is always one playlist.
    func deleteSelected() {
        guard lists.indices.contains(selected) else { return }
        lists.remove(at: selected)
        if lists.isEmpty { lists = [SavedPlaylist(name: Self.defaultName, paths: [])] }
        selected = min(selected, lists.count - 1)
        load()
        persist()
    }

    /// The song on stage, when it is not on the selected playlist.
    var songOffList: URL? {
        guard let url = session.songURL, currentIndex == nil else { return nil }
        return url
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
        if lists.indices.contains(selected) { lists[selected].paths = items.map(\.url.path) }
        guard persistent else { return }
        if let data = try? JSONEncoder().encode(lists) { UserDefaults.standard.set(data, forKey: Self.listsKey) }
        UserDefaults.standard.set(selected, forKey: Self.selectedKey)
    }

    /// Choose songs to add: only the files that are picked, one by one.
    func addFromPanel() {
        let panel = NSOpenPanel()
        panel.title = "加入歌单"
        panel.message = "选择要加入“\(name)”的 .agbsong 文件（可以多选）"
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = true
        guard panel.runModal() == .OK else { return }
        add(panel.urls.filter { $0.pathExtension == "agbsong" })
    }

    // MARK: transport

    var currentIndex: Int? { items.firstIndex { same($0.url, session.songURL) } }
    var cuedIndex: Int? { items.firstIndex { same($0.url, session.cue?.url) } }

    var nextIndex: Int? {
        guard !items.isEmpty else { return nil }
        // A song that was opened by itself is not part of the list: nothing follows it.
        guard let i = currentIndex else { return session.songURL == nil ? 0 : nil }
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

    /// Ten times a second: move on when a song of the list ends.
    private func tick() {
        guard auto, !session.paused, session.cue == nil, advancedPiece != session.pieceSerial,
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
    private enum Naming { case new, rename }
    @State private var naming: Naming?
    @State private var draft = ""
    @FocusState private var nameFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Menu {
                    ForEach(Array(playlist.lists.enumerated()), id: \.offset) { i, l in
                        Button { playlist.select(i) } label: {
                            if i == playlist.selected { Label("\(l.name)（\(l.paths.count)）", systemImage: "checkmark") } else { Text("\(l.name)（\(l.paths.count)）") }
                        }
                    }
                    Divider()
                    Button("新建歌单…") { draft = ""; naming = .new; nameFocused = true }
                    Button("重命名…") { draft = playlist.name; naming = .rename; nameFocused = true }
                    Button("删除这个歌单", role: .destructive) { playlist.deleteSelected() }
                } label: {
                    HStack(spacing: 5) {
                        Text(playlist.name).font(.system(size: 14, weight: .semibold, design: .rounded)).foregroundStyle(Theme.accent).lineLimit(1)
                        Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).foregroundStyle(Theme.dim)
                    }
                }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                .help("切换、新建、重命名或删除歌单")
                Spacer()
                GhostButton(title: "添加曲目", systemImage: "plus") { playlist.addFromPanel() }
            }
            if let mode = naming {
                HStack(spacing: 8) {
                    TextField(mode == .new ? "新歌单的名字" : "歌单的名字", text: $draft)
                        .textFieldStyle(.roundedBorder).focused($nameFocused).onSubmit { commitName(mode) }
                    Button(mode == .new ? "新建" : "改名") { commitName(mode) }.keyboardShortcut(.defaultAction)
                    Button("取消") { naming = nil }
                }
                .controlSize(.small)
            }
            if let url = playlist.songOffList {
                GhostButton(title: "把正在播放的《\(session.brief.isEmpty ? url.deletingPathExtension().lastPathComponent : session.brief)》加入这个歌单", systemImage: "text.badge.plus") {
                    playlist.add([url])
                }
            }
            if playlist.items.isEmpty {
                Text("这个歌单还是空的。点“添加曲目”选择 .agbsong 文件。打开一首曲子不会改动歌单。")
                    .font(.system(size: 12)).foregroundStyle(Theme.dim).padding(.vertical, 18)
                    .fixedSize(horizontal: false, vertical: true)
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

    private func commitName(_ mode: Naming) {
        if mode == .new { playlist.create(draft) } else { playlist.rename(draft) }
        naming = nil
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
            GhostButton(title: playlist.currentIndex.map { "\(playlist.name) \($0 + 1)/\(playlist.items.count)" } ?? "歌单", systemImage: "music.note.list") {
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
