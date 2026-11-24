package dev.agb.duo

import org.json.JSONObject

/**
 * The instrument of the "jam" page (crates/agb-android/src/jam.rs): played with a finger on a pad,
 * with a four-track loop. Its sound is made in the engine and goes out by the device's
 * low-latency path, apart from everything else the app plays.
 */
object Jam {
    init {
        System.loadLibrary("agb")
    }

    @JvmStatic private external fun start(): Int
    @JvmStatic private external fun stop()
    /** The finger: x left to right, y bottom to top, 0…1. */
    @JvmStatic external fun touch(x: Float, y: Float, down: Boolean)
    @JvmStatic private external fun ask(what: Int, a: Int, b: Float)
    @JvmStatic private external fun state(out: FloatArray)
    @JvmStatic private external fun names(): String

    class Program(val name: String, val drums: Boolean)
    class Scale(val name: String, val steps: IntArray)

    val programs: List<Program>
    val rhythms: List<String>
    val scales: List<Scale>
    val keys = listOf("C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B")

    init {
        val o = JSONObject(names())
        programs = o.getJSONArray("programs").let { a -> List(a.length()) { a.getJSONObject(it).let { p -> Program(p.getString("name"), p.getBoolean("drums")) } } }
        rhythms = o.getJSONArray("rhythms").let { a -> List(a.length()) { a.getString(it) } }
        scales = o.getJSONArray("scales").let { a -> List(a.length()) { a.getJSONObject(it).let { s -> Scale(s.getString("name"), s.getJSONArray("steps").let { st -> IntArray(st.length()) { k -> st.getInt(k) } }) } } }
    }

    // What is set, kept here too so that the page can show it. The engine starts the same way.
    const val TRACKS = 4
    var track = 0
        set(v) {
            field = v.mod(TRACKS)
            ask(1, field, 0f)
        }
    val program = intArrayOf(0, 5, 9, 17)
    val muted = BooleanArray(TRACKS)
    val volume = FloatArray(TRACKS) { 0.8f }
    var rhythm = 0
        set(v) {
            field = v.mod(rhythms.size)
            ask(3, field, 0f)
        }
    var key = 0
        set(v) {
            field = v.mod(12)
            ask(4, field, 0f)
        }
    var scale = 3
        set(v) {
            field = v.mod(scales.size)
            ask(5, field, 0f)
        }
    var bpm = 110f
        set(v) {
            field = v.coerceIn(40f, 220f)
            ask(6, 0, field)
        }
    var bars = 4
        set(v) {
            field = v.coerceIn(1, 8)
            ask(7, field, 0f)
        }
    var click = false
        set(v) {
            field = v
            ask(10, if (v) 1 else 0, 0f)
        }

    fun setProgram(track: Int, index: Int) {
        program[track] = index.mod(programs.size)
        ask(2, track, program[track].toFloat())
    }

    fun record(on: Boolean) = ask(8, if (on) 1 else 0, 0f)
    fun play(on: Boolean) = ask(9, if (on) 1 else 0, 0f)
    fun clear(track: Int) = ask(11, track, 0f)

    fun mute(track: Int, on: Boolean) {
        muted[track] = on
        ask(12, track, if (on) 1f else 0f)
    }

    fun setVolume(track: Int, v: Float) {
        volume[track] = v.coerceIn(0f, 1.2f)
        ask(13, track, volume[track])
    }

    /** The note names of the scale across the pad, left to right. */
    fun columns(): List<String> {
        val steps = scales[scale].steps
        return List(2 * steps.size + 1) { keys[(key + steps[it % steps.size]).mod(12)] }
    }

    // What the engine says, read once a frame.
    private val now = FloatArray(14)
    @Volatile var running = false
        private set
    /** Why the sound could not be started, or "". */
    @Volatile var trouble = ""
        private set

    fun read() = state(now)
    val phase get() = now[0]
    val note get() = now[1].toInt()
    fun level(track: Int) = now[2 + track]
    fun filled(track: Int) = now[6 + track] > 0.5f
    val recording get() = now[10] > 0.5f
    val playing get() = now[11] > 0.5f
    val gaps get() = now[12].toInt()
    val burst get() = now[13].toInt()

    /** Start the sound (when the page is first shown). */
    fun wake() {
        if (running) return
        val rate = start()
        running = rate > 0
        trouble = if (rate > 0) "" else "低延迟输出没有打开（$rate）"
    }

    /** Stop the sound: the listener has left. What was recorded is kept. */
    fun rest() {
        if (!running) return
        touch(0.5f, 0.5f, false)
        stop()
        running = false
    }
}
