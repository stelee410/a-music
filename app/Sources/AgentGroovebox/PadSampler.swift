import AVFoundation
import os

/// Per-pad sound settings (all real-time).
struct PadSettings: Equatable, Codable {
    /// Semitones, −12…+12 (playback-rate transposition).
    var pitch: Double = 0
    /// 0…1: fraction of the natural length to keep (1 = let it ring).
    var decay: Double = 1
    /// 0…1: low-pass brightness (1 = open).
    var tone: Double = 1
    /// 0…1
    var level: Double = 0.8
}

/// Decoded stereo sample (48 kHz).
final class PadSample {
    let l: [Float]
    let r: [Float]
    init?(_ b: AVAudioPCMBuffer) {
        guard let ch = b.floatChannelData else { return nil }
        let n = Int(b.frameLength)
        l = Array(UnsafeBufferPointer(start: ch[0], count: n))
        r = b.format.channelCount > 1 ? Array(UnsafeBufferPointer(start: ch[1], count: n)) : l
    }
    var count: Int { l.count }
}

/// Low-latency sampler: voices are mixed directly in the render callback, so a
/// pad sounds within one I/O buffer of the trigger (no player scheduling).
final class PadSampler {
    private struct Voice {
        let sample: PadSample
        var pos: Double
        let rate: Double
        let gain: Float
        var delay: Int
        let end: Double
        let fadeFrom: Double
        let lpA: Float
        var lpL: Float = 0
        var lpR: Float = 0
    }

    private var lock = os_unfair_lock()
    private var pending: [Voice] = []
    private var voices: [Voice] = []
    private let maxVoices = 48
    private let sampleRate: Double
    private(set) var node: AVAudioSourceNode!

    init(format: AVAudioFormat) {
        sampleRate = format.sampleRate
        voices.reserveCapacity(64)
        pending.reserveCapacity(16)
        node = AVAudioSourceNode(format: format) { [unowned self] _, _, frames, abl -> OSStatus in
            self.render(frames: Int(frames), abl: abl)
            return noErr
        }
    }

    /// Main thread.
    func trigger(_ s: PadSample, settings: PadSettings, delaySeconds: Double = 0) {
        let rate = pow(2, settings.pitch / 12)
        let natural = Double(s.count - 2)
        let keep = settings.decay >= 0.999 ? natural : natural * (0.03 + 0.97 * settings.decay * settings.decay)
        let fc = 180 * pow(110, settings.tone)
        let lpA = settings.tone >= 0.999 ? 1 : Float(1 - exp(-2 * Double.pi * fc / sampleRate))
        let v = Voice(sample: s, pos: 0, rate: rate, gain: Float(settings.level * settings.level * 1.4),
                      delay: max(0, Int(delaySeconds * sampleRate)), end: keep,
                      fadeFrom: settings.decay >= 0.999 ? keep : keep * 0.55, lpA: lpA)
        os_unfair_lock_lock(&lock)
        pending.append(v)
        os_unfair_lock_unlock(&lock)
    }

    func stopAll() {
        os_unfair_lock_lock(&lock)
        pending.removeAll()
        os_unfair_lock_unlock(&lock)
        stopRequested = true
    }
    private var stopRequested = false

    /// Audio thread.
    private func render(frames: Int, abl: UnsafeMutablePointer<AudioBufferList>) {
        let bufs = UnsafeMutableAudioBufferListPointer(abl)
        guard bufs.count >= 2, let outL = bufs[0].mData?.assumingMemoryBound(to: Float.self),
              let outR = bufs[1].mData?.assumingMemoryBound(to: Float.self) else { return }
        for i in 0..<frames { outL[i] = 0; outR[i] = 0 }
        if stopRequested { voices.removeAll(keepingCapacity: true); stopRequested = false }
        if os_unfair_lock_trylock(&lock) {
            if !pending.isEmpty {
                for v in pending {
                    if voices.count >= maxVoices { voices.removeFirst() }
                    voices.append(v)
                }
                pending.removeAll(keepingCapacity: true)
            }
            os_unfair_lock_unlock(&lock)
        }
        guard !voices.isEmpty else { return }
        var k = 0
        while k < voices.count {
            var v = voices[k]
            let l = v.sample.l, r = v.sample.r
            var done = false
            for i in 0..<frames {
                if v.delay > 0 { v.delay -= 1; continue }
                if v.pos >= v.end { done = true; break }
                let idx = Int(v.pos)
                let frac = Float(v.pos - Double(idx))
                var sl = l[idx] + (l[idx + 1] - l[idx]) * frac
                var sr = r[idx] + (r[idx + 1] - r[idx]) * frac
                if v.pos > v.fadeFrom {
                    let f = Float(1 - (v.pos - v.fadeFrom) / (v.end - v.fadeFrom))
                    sl *= f * f; sr *= f * f
                }
                if v.lpA < 1 {
                    v.lpL += v.lpA * (sl - v.lpL); v.lpR += v.lpA * (sr - v.lpR)
                    sl = v.lpL; sr = v.lpR
                }
                outL[i] += sl * v.gain
                outR[i] += sr * v.gain
                v.pos += v.rate
            }
            if done {
                voices.remove(at: k)
            } else {
                voices[k] = v
                k += 1
            }
        }
    }
}
