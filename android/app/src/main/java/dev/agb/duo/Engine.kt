package dev.agb.duo

/**
 * The sound engine (crates/agb-android, built by android/build.sh into libagb.so).
 *
 * One song at a time, and one caller at a time: [Player] calls it from its render thread only.
 * A call that fails returns null or false; [lastError] says why.
 */
object Engine {
    init {
        System.loadLibrary("agb")
    }

    @JvmStatic external fun version(): String
    @JvmStatic external fun lastError(): String

    /** A WAV file of the song, under its asset id. */
    @JvmStatic external fun sample(id: String, wav: ByteArray): Boolean
    @JvmStatic external fun clearSamples()

    /** Take `song.json`. Returns a JSON text about the song, or null. `bpm` ≤ 0: the song's own tempo. */
    @JvmStatic external fun load(json: ByteArray, transpose: Int, bpm: Double): String?

    /** The samples (mono) of ticks [from, to); with `last`, what rings on after the end too. */
    @JvmStatic external fun render(from: Long, to: Long, last: Boolean): FloatArray?
    @JvmStatic external fun tickToSample(tick: Long): Long

    /** Keep the calling thread and the threads the engine starts from it on the fastest cores. Returns how many. */
    @JvmStatic external fun useFastCores(): Int

    /** After a jump: instruments and singers start afresh at the next stretch. */
    @JvmStatic external fun forget()
}
