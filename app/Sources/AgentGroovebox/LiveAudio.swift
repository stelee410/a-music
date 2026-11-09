import AVFoundation
import Darwin

/// Parameters of the healing generators, written from the UI, read on the
/// audio thread (plain stored properties; values are smoothed per sample).
final class HealingParams {
    // targets (UI)
    var toneOn = false, toneHz = 432.0, toneLevel = 0.35
    var binOn = false, carrierHz = 200.0, beatHz = 6.0, binLevel = 0.35
    var noiseOn = false, noiseColor = 1, noiseLevel = 0.25 // 0 white, 1 pink, 2 brown
    // audio-thread state
    var curTone = 0.0, curBin = 0.0, curNoise = 0.0
    var curToneHz = 432.0, curCarrier = 200.0, curBeat = 6.0
    var phTone = 0.0, phL = 0.0, phR = 0.0
    var rng: UInt64 = 0x9E3779B97F4A7C15
    var b0 = 0.0, b1 = 0.0, b2 = 0.0, brown = 0.0
}

/// The app's real-time audio graph:
///
///     8 AI stem players → per-track faders → band bus → filter → tuning → delay → reverb ─┐
///     8 pad players ─────────────────────────────────────────────────────────────────┼→ main → limiter → out
///     healing generators (tone, binaural, noise) ────────────────────────────────────┘
///
/// AI stems are scheduled at exact sample positions on a shared player
/// timeline, so all tracks stay sample-aligned; every human control acts
/// immediately on the live signal.
final class LiveAudio {
    static let maxTracks = 8
    let engine = AVAudioEngine()
    let sampleRate: Double = 48000
    lazy var mono = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 1)!
    lazy var stereo = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 2)!

    private var slots: [AVAudioPlayerNode] = []
    private(set) var slotMixers: [AVAudioMixerNode] = []
    private let bandMixer = AVAudioMixerNode()
    private let eq = AVAudioUnitEQ(numberOfBands: 1)
    private let pitch = AVAudioUnitTimePitch()
    private let delay = AVAudioUnitDelay()
    private let reverb = AVAudioUnitReverb()
    private let padMixer = AVAudioMixerNode()
    private let padReverb = AVAudioUnitReverb()
    private(set) var sampler: PadSampler!
    private let healingMixer = AVAudioMixerNode()
    private var healingNode: AVAudioSourceNode!
    private let limiter: AVAudioUnitEffect
    let healing = HealingParams()

    private(set) var slotForTrack: [String: Int] = [:]
    private var baseSample: Int64 = 0
    private(set) var bandStarted = false
    private var recordFile: AVAudioFile?
    private(set) var recordingURL: URL?

    /// `silent`: output goes to a muted sink (headless checks still record at the limiter).
    let silent: Bool

    init(silent: Bool = false) {
        self.silent = silent
        let desc = AudioComponentDescription(componentType: kAudioUnitType_Effect, componentSubType: kAudioUnitSubType_PeakLimiter,
                                             componentManufacturer: kAudioUnitManufacturer_Apple, componentFlags: 0, componentFlagsMask: 0)
        limiter = AVAudioUnitEffect(audioComponentDescription: desc)
        let p = healing
        let sr = sampleRate
        healingNode = AVAudioSourceNode(format: stereo) { _, _, frameCount, abl -> OSStatus in
            let bufs = UnsafeMutableAudioBufferListPointer(abl)
            guard bufs.count >= 2, let l = bufs[0].mData?.assumingMemoryBound(to: Float.self),
                  let r = bufs[1].mData?.assumingMemoryBound(to: Float.self) else { return noErr }
            let k = 0.0015 // smoothing per sample (~15 ms)
            for i in 0..<Int(frameCount) {
                p.curTone += ((p.toneOn ? p.toneLevel * p.toneLevel * 0.35 : 0) - p.curTone) * k
                p.curBin += ((p.binOn ? p.binLevel * p.binLevel * 0.35 : 0) - p.curBin) * k
                p.curNoise += ((p.noiseOn ? p.noiseLevel * p.noiseLevel * 0.5 : 0) - p.curNoise) * k
                p.curToneHz += (p.toneHz - p.curToneHz) * k
                p.curCarrier += (p.carrierHz - p.curCarrier) * k
                p.curBeat += (p.beatHz - p.curBeat) * k
                var L = 0.0, R = 0.0
                if p.curTone > 1e-6 {
                    p.phTone += p.curToneHz / sr; if p.phTone >= 1 { p.phTone -= 1 }
                    let s = sin(2 * .pi * p.phTone) * p.curTone
                    L += s; R += s
                }
                if p.curBin > 1e-6 {
                    // binaural beat: left = carrier, right = carrier + beat
                    p.phL += p.curCarrier / sr; if p.phL >= 1 { p.phL -= 1 }
                    p.phR += (p.curCarrier + p.curBeat) / sr; if p.phR >= 1 { p.phR -= 1 }
                    L += sin(2 * .pi * p.phL) * p.curBin
                    R += sin(2 * .pi * p.phR) * p.curBin
                }
                if p.curNoise > 1e-6 {
                    p.rng ^= p.rng << 13; p.rng ^= p.rng >> 7; p.rng ^= p.rng << 17
                    let w = Double(p.rng % 2_000_001) / 1_000_000 - 1
                    var n: Double
                    switch p.noiseColor {
                    case 0: n = w * 0.5
                    case 2:
                        p.brown = (p.brown + 0.02 * w) / 1.02
                        n = p.brown * 3.5
                    default:
                        p.b0 = 0.99765 * p.b0 + w * 0.0990460
                        p.b1 = 0.96300 * p.b1 + w * 0.2965164
                        p.b2 = 0.57000 * p.b2 + w * 1.0526913
                        n = (p.b0 + p.b1 + p.b2 + w * 0.1848) * 0.2
                    }
                    L += n * p.curNoise; R += n * p.curNoise
                }
                l[i] = Float(L); r[i] = Float(R)
            }
            return noErr
        }
        build()
    }

    private func build() {
        let e = engine
        sampler = PadSampler(format: stereo)
        [bandMixer, eq, pitch, delay, reverb, padMixer, padReverb, sampler.node, healingMixer, healingNode, limiter].forEach { e.attach($0) }
        for _ in 0..<Self.maxTracks {
            let pl = AVAudioPlayerNode(), mx = AVAudioMixerNode()
            e.attach(pl); e.attach(mx)
            e.connect(pl, to: mx, format: mono)
            e.connect(mx, to: bandMixer, format: stereo)
            slots.append(pl); slotMixers.append(mx)
        }
        e.connect(bandMixer, to: eq, format: stereo)
        e.connect(eq, to: pitch, format: stereo)
        e.connect(pitch, to: delay, format: stereo)
        e.connect(delay, to: reverb, format: stereo)
        e.connect(reverb, to: e.mainMixerNode, format: stereo)
        e.connect(sampler.node, to: padReverb, format: stereo)
        e.connect(padReverb, to: padMixer, format: stereo)
        e.connect(padMixer, to: e.mainMixerNode, format: stereo)
        padReverb.loadFactoryPreset(.mediumHall)
        padReverb.wetDryMix = 12
        e.connect(healingNode, to: healingMixer, format: stereo)
        e.connect(healingMixer, to: e.mainMixerNode, format: stereo)
        e.disconnectNodeOutput(e.mainMixerNode)
        e.connect(e.mainMixerNode, to: limiter, format: stereo)
        if silent {
            let sink = AVAudioMixerNode()
            e.attach(sink)
            e.connect(limiter, to: sink, format: stereo)
            e.connect(sink, to: e.outputNode, format: nil)
            sink.outputVolume = 0
        } else {
            e.connect(limiter, to: e.outputNode, format: nil)
        }

        // defaults
        let band = eq.bands[0]
        band.filterType = .resonantLowPass
        band.frequency = 20000
        band.bandwidth = 0.6
        band.bypass = false
        pitch.bypass = true
        delay.wetDryMix = 0
        delay.feedback = 35
        delay.delayTime = 0.375
        reverb.loadFactoryPreset(.largeHall2)
        reverb.wetDryMix = 8
        // Small I/O buffer → pads respond quickly (256 frames ≈ 5.3 ms at 48 kHz).
        if let au = e.outputNode.audioUnit {
            var frames: UInt32 = 256
            AudioUnitSetProperty(au, kAudioDevicePropertyBufferFrameSize, kAudioUnitScope_Global, 0,
                                 &frames, UInt32(MemoryLayout<UInt32>.size))
        }
        e.prepare()
        try? e.start()
    }

    /// Current hardware I/O buffer size in frames (for diagnostics).
    var ioBufferFrames: UInt32 {
        guard let au = engine.outputNode.audioUnit else { return 0 }
        var frames: UInt32 = 0
        var size = UInt32(MemoryLayout<UInt32>.size)
        AudioUnitGetProperty(au, kAudioDevicePropertyBufferFrameSize, kAudioUnitScope_Global, 0, &frames, &size)
        return frames
    }

    // MARK: band (AI stems)

    func resetBand() {
        slots.forEach { $0.stop() }
        slotForTrack = [:]
        bandStarted = false
    }

    private func slot(for track: String) -> Int? {
        if let s = slotForTrack[track] { return s }
        guard slotForTrack.count < Self.maxTracks else { return nil }
        let s = slotForTrack.count
        slotForTrack[track] = s
        return s
    }

    /// Schedule one bar of per-track stems starting at `startSample` of the piece.
    func schedule(stems: [(String, URL)], startSample: Int64) {
        if !engine.isRunning { try? engine.start() }
        if !bandStarted {
            baseSample = startSample
            let t = AVAudioTime(hostTime: mach_absolute_time() + AVAudioTime.hostTime(forSeconds: 0.15))
            slots.forEach { $0.play(at: t) }
            bandStarted = true
        }
        for (track, url) in stems {
            guard let s = slot(for: track), let buf = Self.load(url, as: mono) else { continue }
            slots[s].scheduleBuffer(buf, at: AVAudioTime(sampleTime: startSample - baseSample, atRate: sampleRate))
        }
    }

    var positionSample: Int64? {
        guard bandStarted, let nt = slots[0].lastRenderTime, let pt = slots[0].playerTime(forNodeTime: nt) else { return nil }
        return baseSample + max(0, pt.sampleTime)
    }

    func setTrack(_ track: String, volume: Float, muted: Bool) {
        guard let s = slotForTrack[track] else { return }
        slotMixers[s].outputVolume = muted ? 0 : volume
    }

    // MARK: master FX (0…1 controls)

    /// 0 = dark (200 Hz) … 1 = open (bypass).
    func setFilter(_ v: Double) {
        let b = eq.bands[0]
        b.frequency = Float(200 * pow(100, v))
        b.bypass = v >= 0.995
    }
    func setReverb(_ v: Double) { reverb.wetDryMix = Float(v * 70) }
    func setDelay(_ v: Double, bpm: Double) {
        delay.wetDryMix = Float(v * 45)
        delay.delayTime = min(2, 60 / max(bpm, 20) * 0.75) // dotted eighth
    }
    /// Reference pitch A4 (Hz): shifts the whole band in real time (432 ≈ −31.8 cents).
    func setTuning(a4: Double) {
        let cents = 1200 * log2(a4 / 440)
        pitch.pitch = Float(cents)
        pitch.bypass = abs(cents) < 0.5
    }
    func setMaster(_ v: Double) { engine.mainMixerNode.outputVolume = Float(v) }
    func setPads(_ v: Double) { padMixer.outputVolume = Float(v) }

    // MARK: pads

    private var padSamples: [String: PadSample] = [:]

    func loadPad(_ key: String, url: URL) -> Bool {
        guard let b = Self.load(url, as: stereo), let smp = PadSample(b) else { return false }
        padSamples[key] = smp
        return true
    }

    func hasPad(_ key: String) -> Bool { padSamples[key] != nil }

    /// Trigger a pad now, or after `delaySeconds` (quantized to the band's grid).
    func triggerPad(_ key: String, settings: PadSettings, delaySeconds: Double? = nil) {
        guard let smp = padSamples[key] else { return }
        if !engine.isRunning { try? engine.start() }
        sampler.trigger(smp, settings: settings, delaySeconds: delaySeconds ?? 0)
    }

    func setPadSpace(_ v: Double) { padReverb.wetDryMix = Float(v * 70) }

    // MARK: recording (everything you hear)

    func startRecording(to url: URL) throws {
        stopRecording()
        let fmt = limiter.outputFormat(forBus: 0)
        let f = try AVAudioFile(forWriting: url, settings: fmt.settings)
        recordFile = f
        recordingURL = url
        limiter.installTap(onBus: 0, bufferSize: 4096, format: fmt) { [weak self] buf, _ in
            try? self?.recordFile?.write(from: buf)
        }
    }

    func stopRecording() {
        if recordFile != nil { limiter.removeTap(onBus: 0) }
        recordFile = nil
    }

    // MARK: util

    /// Load an audio file converted to `format` (any rate / channel count / codec).
    static func load(_ url: URL, as format: AVAudioFormat) -> AVAudioPCMBuffer? {
        guard let file = try? AVAudioFile(forReading: url) else { return nil }
        let src = file.processingFormat
        guard let inBuf = AVAudioPCMBuffer(pcmFormat: src, frameCapacity: AVAudioFrameCount(file.length)),
              (try? file.read(into: inBuf)) != nil else { return nil }
        if src.sampleRate == format.sampleRate && src.channelCount == format.channelCount && src.commonFormat == format.commonFormat {
            return inBuf
        }
        guard let conv = AVAudioConverter(from: src, to: format) else { return nil }
        let cap = AVAudioFrameCount(Double(inBuf.frameLength) * format.sampleRate / src.sampleRate) + 1024
        guard let out = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: cap) else { return nil }
        var fed = false
        var err: NSError?
        conv.convert(to: out, error: &err) { _, status in
            if fed { status.pointee = .endOfStream; return nil }
            fed = true
            status.pointee = .haveData
            return inBuf
        }
        return err == nil ? out : nil
    }
}
