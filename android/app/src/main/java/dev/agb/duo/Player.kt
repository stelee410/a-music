package dev.agb.duo

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTimestamp
import android.media.AudioTrack
import android.os.Process
import android.os.SystemClock
import android.util.Log
import org.json.JSONObject
import kotlin.math.exp
import kotlin.math.sqrt

/**
 * Plays a [Song]: one thread renders it with the [Engine] a few bars at a time, ahead of where it
 * is being heard; another feeds what is ready to the sound output.
 *
 * Positions are given and asked for in "song time": seconds at the song's own tempo, the times of
 * the timeline. A change of key, of tempo or of which tracks sound renders the song anew from the
 * bar that is playing.
 *
 * Everything public may be called from any thread.
 */
class Player : Playing {
    enum class State { EMPTY, LOADING, READY, PLAYING, PAUSED, BUFFERING, ENDED, ERROR }

    @Volatile var state = State.EMPTY
        private set
    @Volatile var error: String? = null
        private set
    @Volatile override var song: Song? = null
        private set
    @Volatile var transpose = 0
        private set
    /** Tempo asked for, or 0 for the song's own. */
    @Volatile var bpm = 0.0
        private set
    @Volatile var muted: Set<String> = emptySet()
        private set
    /**
     * The pitch as a ratio (A4 / 440). The sound is played this much slower or faster, and the
     * engine is asked for the tempo divided by it: the pitch moves, the tempo stays.
     */
    @Volatile var tuning = 1.0
        private set
    /** How fast the last stretch was rendered, in times real time (0: not known yet). */
    @Volatile var renderSpeed = 0f
        private set
    /** Tracks the engine could not play as written (it says which, in logcat). */
    @Volatile var skipped = 0
        private set
    /** Called (on the audio thread) when a song has been heard to its end. */
    @Volatile var onEnded: (() -> Unit)? = null

    /** One loading of a song: what the engine said about it, and the sound rendered so far. */
    private class Take(val song: Song, info: JSONObject, val startAt: Long, val tuning: Double) {
        /** The engine has gone on to another song: what is rendered of this one is all there will be. */
        @Volatile var frozen = false
        var nearEndTold = false
        val sampleRate = info.getInt("sampleRate")
        val lengthTicks = info.getLong("lengthTicks")
        val lengthSamples = info.getLong("lengthSamples")
        val total = (lengthSamples + info.getLong("tailSamples")).toInt()
        val barTicks = info.getLong("ppqn") * 4 * song.beatsPerBar / song.beatUnit
        val bars = ((lengthTicks + barTicks - 1) / barTicks).toInt().coerceIn(1, song.bars.size)
        /** The sample at which each bar begins, and the end of the last. */
        val barSample = LongArray(bars + 1)
        val done = BooleanArray(bars)
        val pcm = FloatArray(total)
        val level = FloatArray(total / HOP + 2)
        val low = FloatArray(total / HOP + 2)

        fun barOf(sample: Long): Int {
            var lo = 0
            var hi = bars
            while (lo < hi) {
                val mid = (lo + hi) ushr 1
                if (barSample[mid] <= sample) lo = mid + 1 else hi = mid
            }
            return (lo - 1).coerceIn(0, bars - 1)
        }

        /** Where the sound of bar `i` ends: the last bar has what rings on after it. */
        fun endOf(i: Int) = if (i == bars - 1) total.toLong() else barSample[i + 1]

        /** The sample up to which everything from `from` on is rendered. */
        fun readyUntil(from: Long): Long {
            if (from >= total) return total.toLong()
            var i = barOf(from)
            if (!done[i]) return from
            while (i + 1 < bars && done[i + 1]) i++
            return endOf(i)
        }

        fun timeOf(sample: Long): Float {
            val i = barOf(sample)
            val a = barSample[i]
            val b = barSample[i + 1]
            val x = (sample - a).toFloat() / (b - a).coerceAtLeast(1)
            return song.barStart(i) + x * (song.barEnd(i) - song.barStart(i))
        }

        fun sampleOf(time: Float): Long {
            val i = song.barAt(time).coerceAtMost(bars - 1)
            val x = (time - song.barStart(i)) / (song.barEnd(i) - song.barStart(i)).coerceAtLeast(1e-4f)
            return (barSample[i] + (x * (barSample[i + 1] - barSample[i])).toLong()).coerceIn(0, total.toLong())
        }
    }

    private class Request(val song: Song, val time: Float, val play: Boolean)
    private class Cue(val song: Song, val atEnd: Boolean)

    private val lock = Object()
    private var request: Request? = null
    private var cueRequest: Cue? = null
    /** The next song, loaded and begun while this one plays, and the sample of this one (a bar line) at which it takes over. */
    @Volatile private var cued: Take? = null
    @Volatile private var cueAt = -1L
    /** The song that is waiting to come in on a bar line, or null. */
    val cuedSong: Song? get() = cued?.song
    /** Called (on the audio thread) when the song that was cued has taken over. */
    @Volatile var onSwitched: ((Song) -> Unit)? = null
    /** Called once (on the audio thread) when a song has a few seconds left. */
    @Volatile var onNearEnd: (() -> Unit)? = null
    /** Frames given to the output since it was last emptied. */
    private var fed = 0L
    @Volatile private var take: Take? = null
    @Volatile private var wantPlay = false
    @Volatile private var seekTo = -1L
    /** Where the output is being fed from: the render thread works ahead of this. */
    @Volatile private var writePos = 0L
    /** While a song is being loaded: where it will go on from. */
    @Volatile private var pendingTime = 0f
    private var loadedSamplesOf: Song? = null

    private val posLock = Any()
    private var track: AudioTrack? = null
    private var segStart = 0L
    private val stamp = AudioTimestamp()
    @Volatile private var quit = false

    init {
        Thread({ renderLoop() }, "agb-render").start()
        Thread({ audioLoop() }, "agb-audio").start()
    }

    // ------------------------------------------------------------------ asking

    fun load(song: Song, play: Boolean) {
        transpose = 0
        bpm = 0.0
        muted = emptySet()
        ask(Request(song, 0f, play))
    }

    /**
     * Go on to another song without a gap: it is loaded and begun while this one plays, and comes
     * in on this one's next bar line (`atEnd`: where this one's last bar ends). With nothing
     * playing it is simply loaded and played.
     */
    fun cue(next: Song, atEnd: Boolean = false) {
        if (take == null || !wantPlay || state != State.PLAYING) return load(next, true)
        synchronized(lock) {
            cueRequest = Cue(next, atEnd)
            lock.notifyAll()
        }
    }

    fun play() {
        val t = take ?: return
        if (state == State.ENDED || positionSample(t) >= t.total - 1) seekTo = 0
        wantPlay = true
        wake()
    }

    fun pause() {
        wantPlay = false
        wake()
    }

    override val waiting get() = state == State.LOADING || state == State.BUFFERING
    override val sounding get() = state == State.PLAYING

    fun toggle() = if (wantPlay) pause() else play()
    val playing get() = wantPlay

    /** Jump to a song time. */
    fun seek(time: Float) {
        if (cued != null) return // the next song is about to come in: this one is not moved about any more
        val t = take
        if (t == null) {
            synchronized(lock) { request?.let { request = Request(it.song, time, it.play) } }
            pendingTime = time
            return
        }
        seekTo = t.sampleOf(time.coerceAtLeast(0f))
        wake()
    }

    fun setTranspose(semitones: Int) {
        val v = semitones.coerceIn(-12, 12)
        if (v == transpose) return
        transpose = v
        again()
    }

    /** `value` ≤ 0: the song's own tempo. */
    fun setBpm(value: Double) {
        val own = song?.bpm ?: return
        val v = if (value <= 0 || kotlin.math.abs(value - own) < 0.01) 0.0 else value.coerceIn(own * 0.5, own * 1.5)
        if (v == bpm) return
        bpm = v
        again()
    }

    fun setTuning(ratio: Double) {
        val v = ratio.coerceIn(0.94, 1.06)
        if (kotlin.math.abs(v - tuning) < 1e-6) return
        tuning = v
        again()
    }

    fun toggleMute(trackId: String) {
        muted = if (trackId in muted) muted - trackId else muted + trackId
        again()
    }

    /** The tempo that is playing. */
    val tempo get() = if (bpm > 0) bpm else song?.bpm ?: 0.0

    /** Where the song is being heard, in song time. */
    override fun time(): Float {
        val t = take ?: return pendingTime
        return t.timeOf(positionSample(t))
    }

    /** The bars rendered so far (a copy), for whoever draws the progress. */
    fun barsDone(into: BooleanArray): Int {
        val t = take ?: return 0
        val n = minOf(into.size, t.bars)
        System.arraycopy(t.done, 0, into, 0, n)
        return n
    }

    /** Loudness where the song is being heard: of everything, and of the lows. */
    override fun loudness(out: FloatArray) {
        val t = take
        if (t == null || state != State.PLAYING) {
            out[0] = 0f
            out[1] = 0f
            return
        }
        val i = (positionSample(t) / HOP).toInt().coerceIn(0, t.level.size - 1)
        out[0] = t.level[i]
        out[1] = t.low[i]
    }

    fun release() {
        quit = true
        wake()
    }

    // ------------------------------------------------------------------ inside

    private fun wake() = synchronized(lock) { lock.notifyAll() }

    private fun ask(r: Request) {
        synchronized(lock) {
            request = r
            cueRequest = null
            cued = null
            cueAt = -1
            pendingTime = r.time
            wantPlay = r.play
            state = State.LOADING
            lock.notifyAll()
        }
    }

    /** The same song with what has changed, from the bar that is playing. */
    private fun again() {
        val s = song ?: return
        val now = time()
        ask(Request(s, s.barStart(s.barAt(now)), wantPlay))
    }

    private fun positionSample(t: Take): Long {
        synchronized(posLock) {
            val tr = track ?: return writePos.coerceAtMost(t.total.toLong())
            var frames = (tr.playbackHeadPosition.toLong() and 0xffffffffL)
            if (tr.playState == AudioTrack.PLAYSTATE_PLAYING && tr.getTimestamp(stamp)) {
                val ahead = ((System.nanoTime() - stamp.nanoTime) * t.sampleRate * t.tuning / 1e9).toLong()
                if (ahead in 0..t.sampleRate) frames = stamp.framePosition + ahead
            }
            return (segStart + frames).coerceIn(0, minOf(writePos, t.total.toLong()))
        }
    }

    private fun songJson(song: Song, muted: Set<String>): ByteArray {
        if (muted.isEmpty()) return song.songJson
        val o = JSONObject(String(song.songJson))
        val tracks = o.optJSONArray("tracks") ?: return song.songJson
        for (i in 0 until tracks.length()) {
            val t = tracks.getJSONObject(i)
            if (t.optString("id") in muted) t.put("mute", true)
        }
        return o.toString().toByteArray()
    }

    /** Give the engine a song (with the key, tempo, mutes and tuning that are set) and make the take for it. Null when it could not (then `error` says why). */
    private fun makeTake(next: Song): Take? {
        if (loadedSamplesOf !== next) {
            Engine.clearSamples()
            for ((id, wav) in next.samples) {
                if (!Engine.sample(id, wav)) Log.w(TAG, "sample $id: ${Engine.lastError()}")
            }
            loadedSamplesOf = next
        }
        // tuned: played slower or faster by `tuning`, so rendered at the tempo that makes up for it
        val tuned = tuning
        val asked = if (tuned == 1.0) bpm else (if (bpm > 0) bpm else next.bpm) / tuned
        val text = Engine.load(songJson(next, muted), transpose, asked)
        if (text == null) {
            error = Engine.lastError()
            Log.e(TAG, "load: $error")
            return null
        }
        val info = JSONObject(text)
        val t = try {
            Take(next, info, 0, tuned)
        } catch (e: OutOfMemoryError) {
            error = "内存不够放下这首曲子"
            return null
        }
        for (i in 0..t.bars) t.barSample[i] = if (i == t.bars) t.lengthSamples else Engine.tickToSample(i * t.barTicks)
        val skippedTracks = info.optJSONArray("skipped")
        skipped = skippedTracks?.length() ?: 0
        if (skipped > 0) Log.i(TAG, "played another way: $skippedTracks")
        return t
    }

    private fun doLoad(r: Request) {
        take = null
        song = r.song
        val t = makeTake(r.song)
        if (t == null) {
            state = State.ERROR
            wantPlay = false
            return
        }
        error = null
        renderSpeed = 0f
        run = 0
        expected = -1
        synchronized(lock) {
            if (request != null) return // asked for something else meanwhile
            seekTo = t.sampleOf(r.time)
            writePos = seekTo
            take = t
            state = if (wantPlay) State.BUFFERING else State.READY
        }
    }

    /**
     * Make the next song ready while this one plays. The engine holds one song: from here on it is
     * the next one's, and this one plays out what it has rendered, up to a bar line.
     */
    private fun doCue(c: Cue) {
        val old = take ?: return
        // the next song starts in its own key and tempo, with all its tracks
        transpose = 0
        bpm = 0.0
        muted = emptySet()
        old.frozen = true
        val t = makeTake(c.song)
        if (t == null) {
            // it cannot be played: this one goes on (rendered anew from where it is)
            again()
            return
        }
        run = 0
        expected = -1
        // enough of it to start with: two bars
        while (t.readyUntil(0) < t.barSample[minOf(2, t.bars)]) {
            if (synchronized(lock) { request != null || cueRequest != null } || !renderNext(t, 0)) break
        }
        synchronized(lock) {
            if (request != null || cueRequest != null || take !== old) return
            val from = writePos + old.sampleRate * 3 / 10 // not sooner than a third of a second from now
            val have = old.readyUntil(writePos)
            val line = if (c.atEnd) old.lengthSamples else old.barSample[(old.barOf(from) + 1).coerceAtMost(old.bars)]
            // on that bar line if this song is rendered that far, else on the last line it has
            cueAt = if (line <= have) line else old.barSample[old.barOf(have)].coerceAtLeast(writePos)
            cued = t
        }
    }

    private var run = 0
    private var expected = -1

    /** Render the next bars that are wanted. False when there is nothing left to render. */
    private fun renderNext(t: Take, from: Long = if (seekTo >= 0) seekTo else writePos): Boolean {
        if (t.frozen) return false
        val head = t.barOf(from)
        var i = (head until t.bars).firstOrNull { !t.done[it] } ?: (0 until head).firstOrNull { !t.done[it] } ?: return false
        if (i != expected) run = 0
        // Short stretches first so that it starts soon, then longer ones (less is rendered twice).
        val want = STRETCHES[minOf(run, STRETCHES.size - 1)]
        var n = 1
        while (n < want && i + n < t.bars && !t.done[i + n]) n++
        val last = i + n == t.bars
        val began = SystemClock.elapsedRealtimeNanos()
        val audio = Engine.render(i * t.barTicks, if (last) t.lengthTicks else (i + n) * t.barTicks, last)
        if (audio == null) {
            error = Engine.lastError()
            Log.e(TAG, "render bars $i..${i + n}: $error")
            state = State.ERROR
            wantPlay = false
            return false
        }
        if (t.frozen || synchronized(lock) { request != null }) return true
        val at = t.barSample[i].toInt()
        val len = minOf(audio.size, t.total - at)
        System.arraycopy(audio, 0, t.pcm, at, len)
        curves(t, at, len)
        for (k in i until i + n) t.done[k] = true
        val seconds = (SystemClock.elapsedRealtimeNanos() - began) / 1e9f
        renderSpeed = (len.toFloat() / t.sampleRate) / seconds.coerceAtLeast(1e-3f)
        Log.d(TAG, "bars $i..${i + n - 1}: ${"%.2f".format(seconds)} s, ${"%.1f".format(renderSpeed)}× real time")
        run++
        expected = i + n
        return true
    }

    /** Loudness of what was just rendered, HOP samples at a time: of everything, and below ≈150 Hz. */
    private fun curves(t: Take, at: Int, len: Int) {
        val k = exp(-2.0 * Math.PI * 150 / t.sampleRate).toFloat()
        var lp = 0f
        var i = at / HOP
        val end = at + len
        while (i * HOP < end) {
            val a = maxOf(at, i * HOP)
            val b = minOf(end, (i + 1) * HOP)
            var s = 0f
            var sl = 0f
            for (j in a until b) {
                val x = t.pcm[j]
                lp = k * lp + (1 - k) * x
                s += x * x
                sl += lp * lp
            }
            t.level[i] = sqrt(s / (b - a).coerceAtLeast(1))
            t.low[i] = sqrt(sl / (b - a).coerceAtLeast(1))
            i++
        }
    }

    private fun renderLoop() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_DISPLAY)
        Log.i(TAG, "rendering on ${Engine.useFastCores()} fast cores")
        var idle = false
        while (!quit) {
            val r = synchronized(lock) {
                if (request == null && cueRequest == null && (take == null || idle)) lock.wait(250)
                request.also { request = null }
            }
            if (r != null) {
                try {
                    doLoad(r)
                } catch (e: Exception) {
                    Log.e(TAG, "load", e)
                    error = e.message ?: e.toString()
                    state = State.ERROR
                    wantPlay = false
                }
                idle = false
                continue
            }
            val c = synchronized(lock) { cueRequest.also { cueRequest = null } }
            if (c != null) {
                try {
                    doCue(c)
                } catch (e: Exception) {
                    Log.e(TAG, "cue", e)
                }
                idle = false
                continue
            }
            // the song that is waiting to come in is rendered on from its start; else the one that plays
            val next = cued
            val t = next ?: take
            idle = t == null || state == State.ERROR || !(if (next != null) renderNext(next, 0) else renderNext(t))
            if (!idle) wake()
        }
    }

    private fun output(sampleRate: Int): AudioTrack {
        track?.takeIf { it.sampleRate == sampleRate }?.let { return it }
        track?.release()
        val format = AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_FLOAT).setSampleRate(sampleRate).setChannelMask(AudioFormat.CHANNEL_OUT_MONO).build()
        val min = AudioTrack.getMinBufferSize(sampleRate, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_FLOAT)
        val made = AudioTrack.Builder()
            .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setAudioFormat(format)
            .setBufferSizeInBytes(maxOf(min, sampleRate * 4 / 5)) // 0.2 s of mono floats
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setSessionId(AudioManager.AUDIO_SESSION_ID_GENERATE)
            .build()
        synchronized(posLock) { track = made }
        return made
    }

    private fun audioLoop() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
        var mine: Take? = null
        var sounding = false
        while (!quit) {
            val t = take
            if (t == null) {
                if (sounding) {
                    track?.pause()
                    sounding = false
                }
                mine = null
                synchronized(lock) { if (take == null && !quit) lock.wait(100) }
                continue
            }
            val out = output(t.sampleRate)
            // a tuned take is played at its own rate (the device resamples it)
            val rate = (t.sampleRate * t.tuning).toInt()
            if (out.playbackRate != rate) out.playbackRate = rate
            if (t !== mine || seekTo >= 0) {
                synchronized(posLock) {
                    out.pause()
                    out.flush()
                    fed = 0
                    val to = if (seekTo >= 0) seekTo else t.startAt
                    seekTo = -1
                    segStart = to - (out.playbackHeadPosition.toLong() and 0xffffffffL)
                    writePos = to
                }
                sounding = false
                mine = t
                if (state == State.ENDED) state = State.PAUSED
                wake() // the render thread works from the new place
            }
            if (!wantPlay || state == State.ERROR) {
                if (sounding) {
                    out.pause()
                    sounding = false
                }
                if (state == State.PLAYING || state == State.BUFFERING) state = State.PAUSED
                synchronized(lock) { if (!wantPlay && seekTo < 0 && take === t && !quit) lock.wait(100) }
                continue
            }
            var ready = t.readyUntil(writePos) - writePos
            val next = cued
            if (next != null && cueAt >= 0) {
                if (writePos >= cueAt) {
                    // The bar line: the next song takes over, with no gap. What is heard is still this
                    // song for as long as the output holds of it; the position counts from there.
                    synchronized(posLock) {
                        segStart = -fed
                        writePos = 0
                        cued = null
                        cueAt = -1
                        take = next
                        song = next.song
                    }
                    mine = next
                    onSwitched?.invoke(next.song)
                    wake()
                    continue
                }
                ready = minOf(ready, cueAt - writePos)
            } else if (!t.nearEndTold && t.readyUntil(writePos) >= t.lengthSamples && t.lengthSamples - writePos < t.sampleRate * 6L) {
                // a few seconds left, and all of it rendered: whoever keeps a playlist may cue the next
                t.nearEndTold = true
                onNearEnd?.invoke()
            }
            if (!sounding) {
                // Start (again) with something in hand: three quarters of a second, or all there is.
                val need = minOf(t.total - writePos, (t.sampleRate * 3 / 4).toLong())
                if (ready >= need && ready > 0) {
                    out.play()
                    sounding = true
                    state = State.PLAYING
                } else if (writePos >= t.total) {
                    ended(t, out)
                    sounding = false
                } else {
                    state = State.BUFFERING
                    synchronized(lock) { lock.wait(20) }
                }
                continue
            }
            if (ready > 0) {
                val n = out.write(t.pcm, writePos.toInt(), minOf(ready, 2048L).toInt(), AudioTrack.WRITE_BLOCKING)
                if (n > 0) {
                    writePos += n
                    fed += n
                } else if (n < 0) {
                    Log.e(TAG, "audio write: $n")
                    synchronized(posLock) {
                        track?.release()
                        track = null
                    }
                    mine = null
                    sounding = false
                }
            } else if (writePos >= t.total) {
                // Let what is in the output's buffer be heard, then it is over.
                val left = t.total - positionSample(t)
                if (left <= t.sampleRate / 50 || out.playState != AudioTrack.PLAYSTATE_PLAYING) {
                    ended(t, out)
                    sounding = false
                } else SystemClock.sleep(10)
            } else {
                // The engine has not got this far: the output runs dry, and goes on when there is more.
                SystemClock.sleep(60)
                out.pause()
                sounding = false
                state = State.BUFFERING
            }
        }
        track?.release()
    }

    private fun ended(t: Take, out: AudioTrack) {
        out.pause()
        wantPlay = false
        state = State.ENDED
        if (take === t) onEnded?.invoke()
    }

    companion object {
        private const val TAG = "agb"
        /** Samples per value of the loudness curves (60 a second at 48 kHz). */
        const val HOP = 800
        /** Bars rendered at a time, from a start or a jump on. */
        private val STRETCHES = intArrayOf(1, 1, 2, 4, 8)
    }
}
