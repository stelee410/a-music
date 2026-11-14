import AppKit
import SwiftUI

/// Full-screen video: the song's own video fills the window, the chat box sits
/// at the bottom, and what the band says and plays is overlaid on the right
/// (the overlay can be switched off).
struct VideoTheater: View {
    @ObservedObject var session: Session
    let video: URL
    let onExit: () -> Void

    @AppStorage("theaterShowText") private var showText = true
    @State private var text = ""
    @State private var enteredFullScreen = false
    @FocusState private var focused: Bool

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            SongVideoView(url: video, followSong: session.videoFollowsSong, songSeconds: { [weak session] in session?.videoSeconds }, songRate: { [weak session] in session?.videoRate ?? 1 })
                .id(video)
                .aspectRatio(session.videoAspect, contentMode: .fit)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .overlay(alignment: .trailing) {
            if showText { textOverlay.transition(.move(edge: .trailing).combined(with: .opacity)) }
        }
        .overlay(alignment: .topTrailing) { controls }
        .overlay(alignment: .bottom) { bottomBar }
        .animation(.easeInOut(duration: 0.3), value: showText)
        .onExitCommand(perform: onExit)
        .onAppear {
            focused = true
            if let w = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.isVisible }), !w.styleMask.contains(.fullScreen) {
                enteredFullScreen = true
                w.toggleFullScreen(nil)
            }
        }
        .onDisappear {
            if enteredFullScreen, let w = NSApp.keyWindow ?? NSApp.windows.first(where: { $0.isVisible }), w.styleMask.contains(.fullScreen) {
                w.toggleFullScreen(nil)
            }
        }
    }

    // MARK: top-right controls

    private var controls: some View {
        HStack(spacing: 8) {
            theaterButton(showText ? "隐藏文字" : "显示文字", showText ? "text.bubble.fill" : "text.bubble") { showText.toggle() }
                .help("显示或隐藏右侧的乐手回答和演出文字")
            theaterButton("退出全屏", "arrow.down.right.and.arrow.up.left", action: onExit)
                .help("退出全屏（Esc）")
            if session.isRunning {
                GlowButton(systemImage: "stop.fill", size: 28, color: Theme.color(0)) { session.stop() }
                    .help("立即停止")
            }
        }
        .padding(.top, 14).padding(.trailing, 18)
    }

    private func theaterButton(_ title: String, _ icon: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, systemImage: icon)
                .font(.system(size: 12, weight: .medium, design: .rounded)).foregroundStyle(Theme.text)
                .padding(.horizontal, 12).padding(.vertical, 7)
                .background(Color.black.opacity(0.55), in: Capsule())
                .overlay(Capsule().strokeBorder(Color.white.opacity(0.12)))
        }
        .buttonStyle(.plain)
    }

    // MARK: right overlay: what is playing + the conversation

    private var textOverlay: some View {
        TimelineView(.periodic(from: .now, by: 0.25)) { _ in
            let cur = session.currentPhrase(at: session.playheadTick ?? -1)
            VStack(alignment: .leading, spacing: 10) {
                Spacer(minLength: 60)
                if let ph = cur {
                    Eyebrow(ph.answer ? "回应你" : "正在演奏", color: ph.answer ? Theme.color(3) : Theme.accent)
                    Text(ph.title).font(Theme.display(26)).foregroundStyle(Theme.text).lineLimit(2)
                    if !ph.commentary.isEmpty {
                        Text(ph.commentary).font(Theme.voice(14)).foregroundStyle(Theme.text.opacity(0.8)).lineSpacing(4)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                // The conversation and what the listener changed; housekeeping notes stay in the log.
                let talk = Array(session.feed.filter { $0.kind != .phrase && $0.detail != "详见日志" }.suffix(5))
                if !talk.isEmpty {
                    Rectangle().fill(Color.white.opacity(0.14)).frame(height: 1).padding(.vertical, 4)
                    ForEach(talk) { item in row(item) }
                }
                if session.aiThinkingAboutListener {
                    HStack(spacing: 8) { ThinkingDots(); Text("乐手在想…").font(Theme.voice(12)).foregroundStyle(Theme.dim) }
                }
                Spacer(minLength: 96)
            }
            .padding(.leading, 56).padding(.trailing, 26)
            .frame(width: 420, alignment: .leading)
            .frame(maxHeight: .infinity)
            .background(
                LinearGradient(colors: [.clear, Color.black.opacity(0.55), Color.black.opacity(0.78)], startPoint: .leading, endPoint: .trailing)
            )
            .shadow(color: .black.opacity(0.8), radius: 6)
            .allowsHitTesting(false)
        }
    }

    @ViewBuilder private func row(_ item: FeedItem) -> some View {
        switch item.kind {
        case .listener:
            Text(item.text).font(Theme.display(13)).foregroundStyle(Theme.ink)
                .padding(.horizontal, 10).padding(.vertical, 5)
                .background(Theme.accent.opacity(0.92), in: RoundedRectangle(cornerRadius: 11))
                .frame(maxWidth: .infinity, alignment: .trailing)
        case .ai:
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Circle().fill(Theme.color(3)).frame(width: 6, height: 6)
                Text(item.text).font(Theme.display(15)).foregroundStyle(Theme.text).lineSpacing(3)
                    .fixedSize(horizontal: false, vertical: true)
            }
        default:
            Text(item.detail.isEmpty ? item.text : "\(item.text) · \(item.detail)")
                .font(.system(size: 11, design: .rounded)).foregroundStyle(Theme.dim)
        }
    }

    // MARK: bottom: position + chat box

    private var bottomBar: some View {
        HStack(alignment: .center, spacing: 14) {
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
                .padding(.horizontal, 14).padding(.vertical, 9)
                .background(Color.black.opacity(0.55), in: Capsule())
            }
            HStack(spacing: 8) {
                TextField("对乐手说点什么…", text: $text)
                    .textFieldStyle(.plain)
                    .font(Theme.display(15))
                    .foregroundStyle(Theme.text)
                    .focused($focused)
                    .onSubmit(send)
                Button(action: send) {
                    Image(systemName: "arrow.up").font(.system(size: 11, weight: .bold)).foregroundStyle(Theme.ink)
                        .frame(width: 26, height: 26)
                        .background(text.isEmpty ? Theme.faint : Theme.accent, in: Circle())
                }
                .buttonStyle(.plain)
                .disabled(text.isEmpty || !session.canTalk)
            }
            .padding(.horizontal, 16).padding(.vertical, 10)
            .background(Color.black.opacity(0.62), in: Capsule())
            .overlay(Capsule().strokeBorder(Color.white.opacity(0.14)))
            .frame(maxWidth: 640)
            .disabled(!session.canTalk)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 22).padding(.bottom, 18)
    }

    private func send() {
        session.say(text)
        text = ""
    }
}
