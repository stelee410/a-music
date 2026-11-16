import AppKit
import AVFoundation
import ScreenCaptureKit
import SwiftUI

/// Records the app's own window, picture and sound, to an MP4: a performance as it was seen and
/// heard. Started on the main page it shows the whole app; started in the full-screen theater it is
/// cropped to the picture.
///
/// The system asks once for the "screen recording" permission; nothing but this window is captured.
@MainActor
final class StageRecorder: NSObject, ObservableObject {
    static let shared = StageRecorder()

    @Published private(set) var isRecording = false
    @Published private(set) var startedAt: Date?
    /// Starting or finishing: the button waits.
    @Published private(set) var busy = false
    /// What happened last, for the log: a saved file or the reason nothing was recorded.
    @Published var message: String?

    /// Name used for the file: the piece on stage.
    var titleProvider: () -> String = { "" }
    /// Width ÷ height of the picture while the theater is open; nil on the main page.
    var theaterAspect: Double?

    private var stream: SCStream?
    private var sink: Sink?

    /// Where recordings go.
    static var folder: URL {
        let movies = FileManager.default.urls(for: .moviesDirectory, in: .userDomainMask).first ?? FileManager.default.homeDirectoryForCurrentUser
        return movies.appendingPathComponent("Agent Groovebox", isDirectory: true)
    }

    func toggle() {
        if isRecording { stop() } else { start() }
    }

    func start(to file: URL? = nil) {
        guard !isRecording, !busy else { return }
        guard let window = NSApp.keyWindow ?? NSApp.mainWindow ?? NSApp.windows.first(where: { $0.isVisible && $0.canBecomeMain }) else {
            message = "✗ 没有可以录制的窗口"
            return
        }
        guard CGPreflightScreenCaptureAccess() else {
            CGRequestScreenCaptureAccess()
            message = "✗ 录制需要“屏幕录制”权限：在 系统设置 → 隐私与安全性 → 录屏与系统录音 里允许 Agent Groovebox，然后退出并重新打开 app"
            return
        }
        busy = true
        let crop = theaterAspect
        let title = titleProvider().trimmingCharacters(in: .whitespacesAndNewlines)
        let name = title.isEmpty ? "演出" : title
        Task {
            do {
                let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
                guard let target = content.windows.first(where: { $0.windowID == CGWindowID(window.windowNumber) }) else {
                    throw RecordError.noWindow
                }
                let size = window.frame.size
                var source = CGRect(origin: .zero, size: size)
                if let a = crop, a > 0 {
                    // The theater shows the picture as large as fits: record that rectangle only.
                    source = size.width / size.height > a
                        ? CGRect(x: (size.width - size.height * a) / 2, y: 0, width: size.height * a, height: size.height)
                        : CGRect(x: 0, y: (size.height - size.width / a) / 2, width: size.width, height: size.width / a)
                }
                // In pixels, at most 2560 on the long side, even numbers (the encoder needs them).
                let scale = window.backingScaleFactor
                let k = min(1, 2560 / (max(source.width, source.height) * scale))
                let w = Int(source.width * scale * k) & ~1, h = Int(source.height * scale * k) & ~1

                let cfg = SCStreamConfiguration()
                cfg.width = w
                cfg.height = h
                if crop != nil { cfg.sourceRect = source }
                cfg.scalesToFit = true
                cfg.minimumFrameInterval = CMTime(value: 1, timescale: 60)
                cfg.queueDepth = 6
                cfg.pixelFormat = kCVPixelFormatType_32BGRA
                cfg.showsCursor = false
                cfg.ignoreShadowsSingleWindow = true
                cfg.capturesAudio = true
                cfg.excludesCurrentProcessAudio = false
                cfg.sampleRate = 48000
                cfg.channelCount = 2

                let url = file ?? Self.newFile(named: name)
                try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
                try? FileManager.default.removeItem(at: url)
                let sink = try Sink(url: url, width: w, height: h)
                let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: target), configuration: cfg, delegate: nil)
                try stream.addStreamOutput(sink, type: .screen, sampleHandlerQueue: sink.queue)
                try stream.addStreamOutput(sink, type: .audio, sampleHandlerQueue: sink.queue)
                // In the theater the buttons fade out when a recording starts: wait for that.
                if crop != nil { try? await Task.sleep(for: .seconds(0.45)) }
                try await stream.startCapture()
                self.stream = stream
                self.sink = sink
                self.isRecording = true
                self.startedAt = Date()
                self.message = "● 开始录制（\(w)×\(h)）"
            } catch {
                self.message = "✗ 无法开始录制：\(error.localizedDescription)"
            }
            self.busy = false
        }
    }

    func stop(reveal: Bool = true, then done: ((URL?) -> Void)? = nil) {
        guard isRecording, let stream, let sink else { done?(nil); return }
        self.stream = nil
        self.sink = nil
        isRecording = false
        startedAt = nil
        busy = true
        Task {
            try? await stream.stopCapture()
            let url = await sink.finish()
            if let url {
                self.message = "■ 录制已保存：\(url.path)"
                if reveal { NSWorkspace.shared.activateFileViewerSelecting([url]) }
            } else {
                self.message = "✗ 录制没有保存下来：\(sink.failure ?? "没有收到画面")"
            }
            self.busy = false
            done?(url)
        }
    }

    private static func newFile(named name: String) -> URL {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH.mm.ss"
        let safe = name.map { "/:\\".contains($0) ? "-" : $0 }.prefix(60)
        return folder.appendingPathComponent("\(String(safe)) \(f.string(from: Date())).mp4")
    }

    enum RecordError: LocalizedError {
        case noWindow
        var errorDescription: String? { "系统没有列出这个窗口" }
    }
}

/// Receives the frames and the sound and writes them to the file. Lives on its own queue.
private final class Sink: NSObject, SCStreamOutput, @unchecked Sendable {
    let queue = DispatchQueue(label: "agb.stage-recorder")
    private let url: URL
    private let writer: AVAssetWriter
    private let video: AVAssetWriterInput
    private let audio: AVAssetWriterInput
    private var started = false
    private(set) var failure: String?

    init(url: URL, width: Int, height: Int) throws {
        self.url = url
        writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        video = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: max(6_000_000, width * height * 7),
                AVVideoExpectedSourceFrameRateKey: 60,
                AVVideoMaxKeyFrameIntervalKey: 120,
                AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel,
            ] as [String: Any],
        ])
        video.expectsMediaDataInRealTime = true
        audio = AVAssetWriterInput(mediaType: .audio, outputSettings: [
            AVFormatIDKey: kAudioFormatMPEG4AAC,
            AVSampleRateKey: 48000,
            AVNumberOfChannelsKey: 2,
            AVEncoderBitRateKey: 256_000,
        ])
        audio.expectsMediaDataInRealTime = true
        writer.add(video)
        writer.add(audio)
        guard writer.startWriting() else { throw writer.error ?? StageRecorder.RecordError.noWindow }
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard sb.isValid, writer.status == .writing else { return }
        switch type {
        case .screen:
            // Only frames that carry a picture: the system also reports idle and blank ones.
            guard let info = (CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]])?.first,
                  let raw = info[.status] as? Int, SCFrameStatus(rawValue: raw) == .complete else { return }
            if !started {
                writer.startSession(atSourceTime: sb.presentationTimeStamp)
                started = true
            }
            if video.isReadyForMoreMediaData { video.append(sb) }
        case .audio:
            if started, audio.isReadyForMoreMediaData { audio.append(sb) }
        default:
            break
        }
    }

    /// Close the file. Nil when nothing was written.
    func finish() async -> URL? {
        let ok: Bool = queue.sync {
            guard started, writer.status == .writing else { return false }
            video.markAsFinished()
            audio.markAsFinished()
            return true
        }
        guard ok else {
            failure = writer.error?.localizedDescription
            writer.cancelWriting()
            return nil
        }
        await writer.finishWriting()
        if writer.status != .completed {
            failure = writer.error?.localizedDescription
            return nil
        }
        return url
    }
}

/// "录制" / "停止 0:12": starts and stops the recording of the window.
struct RecordButton: View {
    @ObservedObject var recorder = StageRecorder.shared
    /// The look of the surrounding buttons.
    var theater = false

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { ctx in
            let label: String = {
                guard recorder.isRecording, let t0 = recorder.startedAt else { return recorder.busy ? "请稍候" : "录制" }
                let s = max(0, Int(ctx.date.timeIntervalSince(t0)))
                return String(format: "停止 %d:%02d", s / 60, s % 60)
            }()
            Button { recorder.toggle() } label: {
                Label(label, systemImage: recorder.isRecording ? "stop.circle.fill" : "record.circle")
                    .font(.system(size: 12, weight: .medium, design: .rounded))
                    .lineLimit(1).fixedSize().monospacedDigit()
                    .foregroundStyle(recorder.isRecording ? Color(red: 1, green: 0.35, blue: 0.32) : (theater ? Theme.text : Theme.dim))
                    .padding(.horizontal, 12).padding(.vertical, 7)
                    .background(theater ? Color.black.opacity(0.55) : Color.white.opacity(0.05), in: Capsule())
                    .overlay(Capsule().strokeBorder(Color.white.opacity(theater ? 0.12 : 0)))
            }
            .buttonStyle(.plain)
            .disabled(recorder.busy)
            .help(recorder.isRecording ? "停止录制并保存（⌘R）" : (theater ? "把全屏画面和声音录成视频（⌘R）；录制时不动鼠标，按钮会自动隐藏" : "把这个窗口的画面和声音录成视频（⌘R）"))
        }
    }
}
