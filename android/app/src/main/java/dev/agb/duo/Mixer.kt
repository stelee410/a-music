package dev.agb.duo

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.os.Process
import android.util.Log
import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * The sound of a performance: the band (each bar comes as one sound per track, mixed here so that
 * a fader or a mute is heard at once), the effects on the band, the pads, and the healing sounds.
 * It always runs, so pads and healing sounds can be played with no music.
 *
 * Positions are samples of the performance, from its first bar.
 */
class LiveMixer {
    /** One bar: a sound for each track that plays in it (by the track's slot), all `length` long. */
    class Chunk(val start: Long, val length: Int, val stems: Array<FloatArray?>)

    /** A pad that is sounding: `fade` is what its level is multiplied by every sample, `open` how much of its brightness is let through. */
    private class PadVoice(val sound: FloatArray, var gain: Float, val space: Float, var at: Double, val rate: Double, val fade: Float, val open: Float, val echoOnly: Boolean) {
        var low = 0f
    }

    // ---- what the listener sets (any thread)
    val volume = FloatArray(SLOTS) { DEFAULT_VOLUME }
    val muted = BooleanArray(SLOTS)
    /** 0…1: low-pass from 200 Hz to open. */
    @Volatile var filter = 1f
    @Volatile var reverb = 0.12f
    @Volatile var delay = 0f
    @Volatile var master = 0.9f
    /**
     * The band's pitch as a ratio (A4 / 440): it is read this much faster or slower. Its tempo goes
     * with it, so whoever sets this asks the engine for the tempo divided by it. Pads and healing
     * sounds are not touched.
     */
    @Volatile var rate = 1f
    /** The tempo that is being heard: the delay repeats in dotted eighths of it. */
    @Volatile var bpm = 120.0
    @Volatile var toneOn = false
    @Volatile var toneHz = 432f
    @Volatile var toneLevel = 0.25f
    @Volatile var binauralOn = false
    @Volatile var carrierHz = 200f
    @Volatile var beatHz = 6f
    @Volatile var binauralLevel = 0.25f
    @Volatile var noiseOn = false
    /** 0 white, 1 pink, 2 brown. */
    @Volatile var noiseColor = 1
    @Volatile var noiseLevel = 0.2f

    // ---- the band
    private val lock = Any()
    private val chunks = ArrayList<Chunk>()
    /** Where the band is being read, in samples of the performance: not a whole number when it is tuned. */
    private var position = 0.0
    /** Where the performance ends, once that is known. */
    private var end = -1L
    @Volatile var playing = false
        private set
    /** The band has been heard to its end. */
    @Volatile var ended = false
        private set
    /** Waiting for a bar that has not come. */
    @Volatile var dry = false
        private set
    @Volatile var onEnded: (() -> Unit)? = null
    @Volatile private var heardPosition = 0L
    @Volatile private var levelNow = 0f
    @Volatile private var lowNow = 0f
    /** How hard each slot was just playing, for the lamps of the tracks. */
    val slotLevel = FloatArray(SLOTS)

    private val pads = ArrayList<PadVoice>()
    /** Kept running although nothing sounds: the listener is at the console, and a pad must sound the moment it is hit (an output that rests takes a while to start). */
    @Volatile var awake = false
    /** What the output was given: how many frames it holds, and whether it is on the device's short path. */
    @Volatile var bufferFrames = 0
        private set
    @Volatile var fastPath = false
        private set
    /** How often the output ran dry (a click each time): too many means the buffer is too short for this device. */
    @Volatile var gaps = 0
        private set
    @Volatile private var recording: RandomAccessFile? = null
    private var recorded = 0L
    @Volatile private var quit = false

    init {
        Thread({ run() }, "agb-mixer").start()
    }

    // ------------------------------------------------------------------ the band

    fun add(chunk: Chunk) = synchronized(lock) {
        chunks.add(chunk)
        dry = false
    }

    /** The performance ends at this sample (what rings on after the last bar included). */
    fun endsAt(sample: Long) = synchronized(lock) { end = sample }

    fun play() {
        ended = false
        playing = true
    }

    fun pause() {
        playing = false
    }

    /** Forget the band: before another performance. */
    fun reset() = synchronized(lock) {
        playing = false
        ended = false
        dry = false
        chunks.clear()
        position = 0.0
        end = -1
        heardPosition = 0
        for (i in 0 until SLOTS) {
            volume[i] = DEFAULT_VOLUME
            muted[i] = false
        }
    }

    /** The sample of the performance that is being heard. */
    fun heard() = heardPosition
    fun level() = levelNow
    fun low() = lowNow

    /** How much has come and not been heard yet, in samples. */
    fun ahead(): Long = synchronized(lock) { (chunks.lastOrNull()?.let { it.start + it.length } ?: position.toLong()) - position.toLong() }

    // ------------------------------------------------------------------ pads and recording

    /**
     * Play a sound once. `semitones` moves its pitch (and its length); `space` is how much of it
     * goes to the reverb; `decay` 0…1 lets it ring to its end (1) or cuts it short; `tone` 0…1 is
     * dark to as it is. `echoOnly`: the sound itself is being played by a [PadPlayer] (sooner than
     * this mixer could); only its reverb comes from here, and its place in a recording.
     */
    fun hit(sound: FloatArray, gain: Float, space: Float, semitones: Float = 0f, decay: Float = 1f, tone: Float = 1f, echoOnly: Boolean = false) = synchronized(pads) {
        if (pads.size >= 24) pads.removeAt(0)
        // at decay 0 the sound is down by 60 dB after 80 ms; at 1 it is left alone
        val seconds = 0.08 * 400.0.pow(decay.toDouble().coerceIn(0.0, 1.0))
        val fade = if (decay >= 0.995f) 1f else exp(-6.9 / (seconds * RATE)).toFloat()
        val open = if (tone >= 0.995f) 1f else (1 - exp(-2.0 * PI * 300.0 * 60.0.pow(tone.toDouble().coerceIn(0.0, 1.0)) / RATE)).toFloat()
        pads.add(PadVoice(sound, gain, space, 0.0, 2.0.pow(semitones / 12.0), fade, open, echoOnly))
    }

    /** Write everything that is heard to a WAV file (16 bits, stereo) until [stopRecording]. */
    fun startRecording(file: File) {
        stopRecording()
        val f = RandomAccessFile(file, "rw")
        f.setLength(0)
        f.write(ByteArray(44))
        recorded = 0
        recording = f
    }

    val isRecording get() = recording != null

    fun stopRecording() {
        val f = recording ?: return
        recording = null
        synchronized(f) {
            val h = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
            h.put("RIFF".toByteArray()).putInt((36 + recorded).toInt()).put("WAVE".toByteArray())
            h.put("fmt ".toByteArray()).putInt(16).putShort(1).putShort(2).putInt(RATE).putInt(RATE * 4).putShort(4).putShort(16)
            h.put("data".toByteArray()).putInt(recorded.toInt())
            f.seek(0)
            f.write(h.array())
            f.close()
        }
    }

    fun release() {
        quit = true
        stopRecording()
    }

    // ------------------------------------------------------------------ the audio thread

    /** A feedback comb with a little damping, and an all-pass: the parts of a small plate-like reverb. */
    private class Comb(size: Int, val feedback: Float) {
        val buf = FloatArray(size)
        var i = 0
        var damp = 0f
        fun next(x: Float): Float {
            val y = buf[i]
            damp = y * 0.75f + damp * 0.25f
            buf[i] = x + damp * feedback
            if (++i == buf.size) i = 0
            return y
        }
    }

    private class AllPass(size: Int) {
        val buf = FloatArray(size)
        var i = 0
        fun next(x: Float): Float {
            val b = buf[i]
            val y = b - x
            buf[i] = x + b * 0.5f
            if (++i == buf.size) i = 0
            return y
        }
    }

    private fun run() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        // 16 bits, at the device's own rate, in blocks of its own size: what its short path (the one a pad needs) takes
        val format = AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(RATE).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build()
        val min = AudioTrack.getMinBufferSize(RATE, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_16BIT)
        val out = AudioTrack.Builder()
            .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setAudioFormat(format)
            .setBufferSizeInBytes(min)
            .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setSessionId(AudioManager.AUDIO_SESSION_ID_GENERATE)
            .build()
        // Room enough that the output never runs dry (a gap is a click, and a tuning fork shows every
        // one): 60 ms. What is heard is that much late, which the band's position allows for; the
        // pads, which must not be late, have outputs of their own ([PadPlayer]).
        out.bufferSizeInFrames = maxOf(BLOCK * 15, out.bufferSizeInFrames)
        bufferFrames = out.bufferSizeInFrames
        fastPath = out.performanceMode == AudioTrack.PERFORMANCE_MODE_LOW_LATENCY
        Log.i("agb", "mixer: buffer $bufferFrames frames, low latency $fastPath")
        out.play()

        val band = FloatArray(BLOCK)
        val source = FloatArray(BLOCK * 2 + 8)
        var before = 0f
        val send = FloatArray(BLOCK) // to the reverb
        val unheard = FloatArray(BLOCK) // pads that sound elsewhere: for the recording only
        val stereo = FloatArray(BLOCK * 2)
        val pcm = ByteArray(BLOCK * 4)
        val shorts = ShortArray(BLOCK * 2)
        val gain = FloatArray(SLOTS)
        var lp1 = 0f
        var lp2 = 0f
        val echo = FloatArray(RATE * 2)
        var echoAt = 0
        val combs = intArrayOf(1557, 1617, 1491, 1422, 1277, 1356).map { Comb(it, 0.84f) }
        val passes = intArrayOf(225, 556, 441).map { AllPass(it) }
        var written = 0L
        var tonePhase = 0.0
        var leftPhase = 0.0
        var rightPhase = 0.0
        var seed = 0x2545F491
        var pink = 0f
        var brown = 0f
        var lowState = 0f
        val lowK = exp(-2.0 * PI * 150 / RATE).toFloat()

        var quiet = 0
        var resting = false
        while (!quit) {
            // With nothing to sound for a while (the reverb has died away too) the output rests.
            val busy = awake || playing || toneOn || binauralOn || noiseOn || recording != null || synchronized(pads) { pads.isNotEmpty() }
            quiet = if (busy) 0 else quiet + 1
            if (quiet > RATE * 4 / BLOCK) {
                if (!resting) {
                    out.pause()
                    resting = true
                }
                android.os.SystemClock.sleep(15)
                continue
            }
            if (resting) {
                out.play()
                resting = false
            }
            band.fill(0f)
            send.fill(0f)
            unheard.fill(0f)
            var finished = false
            // ---- the band
            synchronized(lock) {
                if (playing) {
                    // The band at whole samples from where we are, a little more than a block of it
                    // (more when it is read faster), then read at `rate` with a smooth curve between samples.
                    val r = rate.toDouble().coerceIn(0.9, 1.1)
                    val base = Math.floor(position).toLong()
                    val need = (Math.floor(position + BLOCK * r).toLong() - base).toInt() + 4
                    source.fill(0f, 0, need)
                    var covered = 0
                    for (c in chunks) {
                        val a = maxOf(base, c.start)
                        val b = minOf(base + need, c.start + c.length)
                        if (b <= a) continue
                        covered = maxOf(covered, (b - base).toInt())
                        for (s in 0 until SLOTS) {
                            val stem = c.stems[s] ?: continue
                            val want = if (muted[s]) 0f else volume[s]
                            var g = gain[s]
                            val step = (want - g) / need
                            var peak = 0f
                            var from = (a - c.start).toInt()
                            for (i in (a - base).toInt() until (b - base).toInt()) {
                                g += step
                                val x = stem[from++]
                                source[i] += x * g
                                if (abs(x) > peak) peak = abs(x)
                            }
                            slotLevel[s] = maxOf(peak, slotLevel[s] * 0.9f)
                        }
                    }
                    for (s in 0 until SLOTS) gain[s] = if (muted[s]) 0f else volume[s]
                    when {
                        covered > 0 -> {
                            if (r == 1.0 && position == base.toDouble()) System.arraycopy(source, 0, band, 0, BLOCK)
                            else {
                                var at = position - base
                                for (i in 0 until BLOCK) {
                                    val k = at.toInt()
                                    val f = (at - k).toFloat()
                                    // a curve through four samples (Catmull-Rom); the one before the first is kept from the block before
                                    val y0 = if (k == 0) before else source[k - 1]
                                    val y1 = source[k]
                                    val y2 = source[k + 1]
                                    val y3 = source[k + 2]
                                    band[i] = y1 + 0.5f * f * (y2 - y0 + f * (2f * y0 - 5f * y1 + 4f * y2 - y3 + f * (3f * (y1 - y2) + y3 - y0)))
                                    at += r
                                }
                            }
                            position += BLOCK * r
                            val last = (Math.floor(position).toLong() - base).toInt() - 1
                            before = if (last in 0 until need) source[last] else 0f
                            dry = false
                        }
                        end in 0..base -> {
                            playing = false
                            finished = true
                        }
                        else -> dry = true // the bar has not come: wait where we are
                    }
                    // what has been heard is not needed again
                    while (chunks.size > 1 && chunks[0].start + chunks[0].length < base - RATE) chunks.removeAt(0)
                } else for (s in 0 until SLOTS) slotLevel[s] *= 0.9f
            }

            // ---- the effects on the band: low-pass, echo
            val f = filter
            if (f < 0.995f) {
                val k = 1f - exp(-2.0 * PI * 200.0 * 100.0.pow(f.toDouble()) / RATE).toFloat()
                for (i in 0 until BLOCK) {
                    lp1 += k * (band[i] - lp1)
                    lp2 += k * (lp1 - lp2)
                    band[i] = lp2
                }
            }
            val wetEcho = delay * 0.45f
            // a dotted eighth of the tempo
            val echoLength = (0.75 * 60.0 / bpm.coerceIn(30.0, 300.0) * RATE).toInt().coerceIn(1, echo.size - 1)
            for (i in 0 until BLOCK) {
                var from = echoAt - echoLength
                if (from < 0) from += echo.size
                val e = echo[from]
                echo[echoAt] = band[i] + e * 0.38f
                if (++echoAt == echo.size) echoAt = 0
                band[i] += e * wetEcho
                send[i] = band[i] * reverb * 0.7f
            }

            // ---- the pads: beside the band's effects, each with its own share of the reverb
            synchronized(pads) {
                val it = pads.iterator()
                while (it.hasNext()) {
                    val p = it.next()
                    for (i in 0 until BLOCK) {
                        val k = p.at.toInt()
                        if (k + 1 >= p.sound.size) break
                        val frac = (p.at - k).toFloat()
                        var x = (p.sound[k] + (p.sound[k + 1] - p.sound[k]) * frac) * p.gain
                        if (p.open < 1f) {
                            p.low += p.open * (x - p.low)
                            x = p.low
                        }
                        p.gain *= p.fade
                        if (p.echoOnly) unheard[i] += x else band[i] += x
                        send[i] += x * p.space
                        p.at += p.rate
                    }
                    if (p.at.toInt() + 1 >= p.sound.size || p.gain < 1e-4f) it.remove()
                }
            }

            // ---- reverb, loudness, and out
            var sum = 0f
            var sumLow = 0f
            val m = master
            val tone = if (toneOn) toneLevel else 0f
            val bin = if (binauralOn) binauralLevel else 0f
            val noise = if (noiseOn) noiseLevel else 0f
            val toneStep = toneHz.toDouble() / RATE
            val leftStep = (carrierHz - beatHz / 2).toDouble() / RATE
            val rightStep = (carrierHz + beatHz / 2).toDouble() / RATE
            for (i in 0 until BLOCK) {
                var wet = 0f
                val x = send[i]
                for (c in combs) wet += c.next(x)
                wet *= 0.16f
                for (a in passes) wet = a.next(wet)
                val y = (band[i] + wet) * m
                lowState = lowK * lowState + (1 - lowK) * y
                sum += y * y
                sumLow += lowState * lowState
                var l = y
                var r = y
                if (tone > 0f) {
                    val t = sin(2 * PI * tonePhase).toFloat() * tone
                    l += t
                    r += t
                    tonePhase += toneStep
                    if (tonePhase >= 1) tonePhase -= 1
                }
                if (bin > 0f) {
                    l += sin(2 * PI * leftPhase).toFloat() * bin
                    r += sin(2 * PI * rightPhase).toFloat() * bin
                    leftPhase += leftStep
                    if (leftPhase >= 1) leftPhase -= 1
                    rightPhase += rightStep
                    if (rightPhase >= 1) rightPhase -= 1
                }
                if (noise > 0f) {
                    seed = seed xor (seed shl 13)
                    seed = seed xor (seed ushr 17)
                    seed = seed xor (seed shl 5)
                    val white = seed / 2147483648f
                    pink = 0.97f * pink + 0.03f * white * 6f
                    brown = (brown + white * 0.02f) * 0.998f
                    val n = when (noiseColor) {
                        0 -> white * 0.5f
                        2 -> brown * 3f
                        else -> pink
                    } * noise
                    l += n
                    r += n
                }
                // never over full scale: gently above 0.8
                stereo[2 * i] = soft(l)
                stereo[2 * i + 1] = soft(r)
            }
            levelNow = sqrt(sum / BLOCK)
            lowNow = sqrt(sumLow / BLOCK)

            for (i in 0 until BLOCK * 2) shorts[i] = (stereo[i] * 32767f).toInt().coerceIn(-32768, 32767).toShort()
            recording?.let { file ->
                for (i in 0 until BLOCK * 2) {
                    val v = (shorts[i] + (unheard[i / 2] * master * 32767f).toInt()).coerceIn(-32768, 32767)
                    pcm[2 * i] = v.toByte()
                    pcm[2 * i + 1] = (v shr 8).toByte()
                }
                synchronized(file) {
                    if (recording === file) {
                        try {
                            file.write(pcm)
                            recorded += pcm.size
                        } catch (e: Exception) {
                            Log.w("agb", "recording: ${e.message}")
                        }
                    }
                }
            }

            val n = out.write(shorts, 0, shorts.size, AudioTrack.WRITE_BLOCKING)
            if (n > 0) written += n / 2
            // Dry more than now and then: this device needs more room. A block more, up to 60 ms.
            val dry = out.underrunCount
            if (dry != gaps) {
                gaps = dry
                if (dry % 4 == 0 && out.bufferSizeInFrames < BLOCK * 40) {
                    out.bufferSizeInFrames = out.bufferSizeInFrames + BLOCK
                    bufferFrames = out.bufferSizeInFrames
                }
            }
            // what is heard now was written a little while ago
            val late = (written - (out.playbackHeadPosition.toLong() and 0xffffffffL)).coerceIn(0, RATE.toLong())
            heardPosition = (synchronized(lock) { position }.toLong() - (late * rate).toLong()).coerceAtLeast(0)
            if (finished) {
                ended = true
                onEnded?.invoke()
            }
        }
        out.release()
    }

    private fun soft(x: Float): Float {
        val a = abs(x)
        if (a <= 0.8f) return x
        val over = a - 0.8f
        val y = 0.8f + 0.2f * (over / (over + 0.2f))
        return if (x < 0) -y else y
    }

    companion object {
        const val RATE = 48000
        const val SLOTS = 16
        const val DEFAULT_VOLUME = 0.85f
        /** Frames mixed at a time: what the device's output takes at a time (4 ms). */
        private const val BLOCK = 192
    }
}

/** Read a WAV file of 32-bit floats or 16-bit integers as one channel. */
fun readWav(file: File): FloatArray {
    val bytes = file.readBytes()
    val b = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
    var at = 12
    var float = false
    var bits = 16
    var channels = 1
    while (at + 8 <= bytes.size) {
        val id = String(bytes, at, 4)
        val size = b.getInt(at + 4)
        if (id == "fmt ") {
            val tag = b.getShort(at + 8).toInt() and 0xffff
            channels = b.getShort(at + 10).toInt().coerceAtLeast(1)
            bits = b.getShort(at + 22).toInt()
            // the extensible form says what it is in its sub-format
            float = tag == 3 || (tag == 0xfffe && size >= 26 && b.getShort(at + 32).toInt() == 3)
        } else if (id == "data") {
            val n = minOf(size, bytes.size - at - 8)
            val frame = bits / 8 * channels
            val out = FloatArray(n / frame)
            for (i in out.indices) {
                val p = at + 8 + i * frame
                out[i] = when {
                    float && bits == 32 -> b.getFloat(p)
                    bits == 16 -> b.getShort(p) / 32768f
                    bits == 24 -> ((bytes[p].toInt() and 0xff) or ((bytes[p + 1].toInt() and 0xff) shl 8) or (bytes[p + 2].toInt() shl 16)) / 8388608f
                    bits == 32 -> b.getInt(p) / 2147483648f
                    else -> 0f
                }
            }
            return out
        }
        at += 8 + size + (size and 1)
    }
    return FloatArray(0)
}


/**
 * The pads of the bank that is shown, each loaded whole into an output of its own, so that a hit
 * starts it at once (a sound that is fed to the output piece by piece, as the mixer feeds the
 * band, is heard a tenth of a second late on this kind of device; the keys of a game are not).
 * Decay and tone are worked into the loaded sound; pitch and volume are set as it is hit.
 */
class PadPlayer {
    private class Loaded(val key: String, val track: AudioTrack)

    private val slots = arrayOfNulls<Loaded>(8)

    /** Load pad `index` (again, if what it is made from has changed). Not on the main thread: a long sound takes a moment. */
    fun load(index: Int, sound: FloatArray, decay: Float, tone: Float) {
        val key = "${System.identityHashCode(sound)}/$decay/$tone"
        if (slots[index]?.key == key) return
        val n = sound.size.coerceAtLeast(1)
        val pcm = ShortArray(n)
        val seconds = 0.08 * 400.0.pow(decay.toDouble().coerceIn(0.0, 1.0))
        val fade = if (decay >= 0.995f) 1f else exp(-6.9 / (seconds * LiveMixer.RATE)).toFloat()
        val open = if (tone >= 0.995f) 1f else (1 - exp(-2.0 * PI * 300.0 * 60.0.pow(tone.toDouble().coerceIn(0.0, 1.0)) / LiveMixer.RATE)).toFloat()
        var gain = 1f
        var low = 0f
        for (i in sound.indices) {
            var x = sound[i] * gain
            if (open < 1f) {
                low += open * (x - low)
                x = low
            }
            gain *= fade
            pcm[i] = (x * 32767f).toInt().coerceIn(-32768, 32767).toShort()
        }
        val track = try {
            AudioTrack.Builder()
                .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_GAME).setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
                .setAudioFormat(AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(LiveMixer.RATE).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build())
                .setBufferSizeInBytes(n * 2)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .setTransferMode(AudioTrack.MODE_STATIC)
                .build()
                .also { it.write(pcm, 0, n) }
        } catch (e: Exception) {
            Log.w("agb", "pad $index: ${e.message}")
            return
        }
        val old = slots[index]
        slots[index] = Loaded(key, track)
        old?.track?.release()
    }

    fun ready(index: Int) = slots.getOrNull(index) != null

    /** Sound pad `index` now. False when it is not loaded. */
    fun hit(index: Int, volume: Float, semitones: Float): Boolean {
        val t = slots.getOrNull(index)?.track ?: return false
        return try {
            if (t.playState != AudioTrack.PLAYSTATE_STOPPED) t.stop()
            t.reloadStaticData()
            t.playbackRate = (LiveMixer.RATE * 2.0.pow(semitones / 12.0)).toInt()
            t.setVolume(volume.coerceIn(0f, 1f))
            t.play()
            true
        } catch (e: Exception) {
            false
        }
    }

    fun clear() {
        for (i in slots.indices) {
            slots[i]?.track?.release()
            slots[i] = null
        }
    }
}
