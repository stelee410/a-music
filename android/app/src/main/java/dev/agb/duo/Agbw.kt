package dev.agb.duo

import android.graphics.Color
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.util.zip.ZipInputStream

// Opening an .agbw (AGBW.md): the manifest, the song for the engine, the timeline to draw from and
// the samples. Times of the timeline are seconds at the song's own tempo ("song time").

/**
 * The notes of a track, sorted by start: parallel arrays, for the frames that read them.
 *
 * A track of a performance that is still being played grows ([append]); the arrays are replaced,
 * never written into, so a frame that is being drawn goes on reading the ones it has.
 */
class Track(
    val id: String,
    val name: String,
    val role: String,
    val engine: String,
    val sung: Boolean,
    start: FloatArray,
    length: FloatArray,
    pitch: IntArray,
    velocity: FloatArray,
) {
    @Volatile var start = start
        private set
    @Volatile var length = length
        private set
    @Volatile var pitch = pitch
        private set
    @Volatile var velocity = velocity
        private set
    val count get() = start.size
    val drums = role == "drums" || engine.startsWith("drum.")
    @Volatile var lowest = pitch.minOrNull() ?: 60
        private set
    @Volatile var highest = pitch.maxOrNull() ?: 60
        private set

    /** More notes, all later than the ones there are. */
    fun append(s: FloatArray, l: FloatArray, p: IntArray, v: FloatArray) {
        if (s.isEmpty()) return
        lowest = if (count == 0) p.min() else minOf(lowest, p.min())
        highest = if (count == 0) p.max() else maxOf(highest, p.max())
        // `start` last: how many notes there are is read from it, and the others are then long enough
        length = length + l
        pitch = pitch + p
        velocity = velocity + v
        start = start + s
    }

    /** First index whose start is > t. */
    fun upper(t: Float): Int {
        val start = start
        var lo = 0
        var hi = start.size
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (start[mid] <= t) lo = mid + 1 else hi = mid
        }
        return lo
    }

    /** How hard the track was just struck: velocity of its latest notes, dying away over `decay` seconds. */
    fun pulse(t: Float, decay: Float = 0.25f): Float {
        val start = start
        val velocity = velocity
        var best = 0f
        var i = upper(t) - 1
        while (i >= 0) {
            val age = t - start[i]
            if (age > decay * 6) break
            val v = velocity[i] * kotlin.math.exp(-age / decay)
            if (v > best) best = v
            i--
        }
        return best
    }
}

class Word(val from: Float, val to: Float, val text: String)
class Line(val from: Float, val to: Float, val text: String, val words: List<Word>)
class Section(val id: String, val name: String, val from: Float, val to: Float)

/**
 * A song as the stage and the console draw it. One that is read from a file is whole; one that is
 * being performed grows bar by bar (the fields that can change are replaced, not written into).
 */
class Song(
    val title: String,
    val artist: String,
    val key: String,
    val scale: String,
    val bpm: Double,
    seconds: Float,
    /** Background, then three accents. */
    val palette: IntArray,
    val scene: String,
    tracks: List<Track>,
    /** Where each bar begins. */
    bars: FloatArray,
    val beatsPerBar: Int,
    /** The note value of a beat: 4 for quarters. */
    val beatUnit: Int,
    sections: List<Section>,
    val lyrics: List<Line>,
    val songJson: ByteArray,
    val samples: Map<String, ByteArray>,
) {
    @Volatile var seconds = seconds
    @Volatile var tracks = tracks
    @Volatile var bars = bars
    @Volatile var sections = sections
    /** Counts up when a track is added: who draws the tracks looks again. */
    @Volatile var revision = 0

    /** Index of the bar at t. */
    fun barAt(t: Float): Int {
        val bars = bars
        var lo = 0
        var hi = bars.size
        while (lo < hi) {
            val mid = (lo + hi) ushr 1
            if (bars[mid] <= t) lo = mid + 1 else hi = mid
        }
        return (lo - 1).coerceAtLeast(0)
    }

    fun barStart(i: Int) = bars.let { it[i.coerceIn(0, it.size - 1)] }
    fun barEnd(i: Int) = bars.let { if (i + 1 < it.size) it[i + 1] else maxOf(seconds, it[it.size - 1] + 0.001f) }
    fun barLength(t: Float): Float = barAt(t).let { barEnd(it) - barStart(it) }
    fun barPhase(t: Float): Float = barAt(t).let { ((t - barStart(it)) / (barEnd(it) - barStart(it))).coerceIn(0f, 1f) }
    fun beatPhase(t: Float): Float = (barPhase(t) * beatsPerBar).let { it - kotlin.math.floor(it) }
    fun sectionAt(t: Float): Section? = sections.firstOrNull { t >= it.from && t < it.to }
    fun lineAt(t: Float, before: Float, after: Float): Line? = lyrics.firstOrNull { t >= it.from - before && t < it.to + after }
}

/** What the stage and the console draw from: a song that is being heard, and where in it. */
interface Playing {
    val song: Song?
    /** Where the song is being heard, in song time (seconds). */
    fun time(): Float
    /** Loudness where the song is being heard: of everything (out[0]), and of the lows (out[1]). */
    fun loudness(out: FloatArray)
    /** Nothing to hear yet: being loaded, or the engine has not got this far. */
    val waiting: Boolean
    val sounding: Boolean
}

class AgbwException(message: String) : Exception(message)

private const val MIMETYPE = "application/vnd.agb.web+zip"
private val FALLBACK_PALETTE = intArrayOf(0xff0b0d14.toInt(), 0xffe8825a.toInt(), 0xfff4ead8.toInt(), 0xff5fb8b0.toInt())

private fun safe(p: String?) = !p.isNullOrEmpty() && !p.startsWith("/") && !p.contains('\\') && !p.split('/').contains("..")

private fun palette(manifest: JSONObject): IntArray {
    val given = ArrayList<Int>()
    val a = manifest.optJSONObject("visual")?.optJSONArray("palette") ?: JSONArray()
    for (i in 0 until a.length()) {
        val c = a.optString(i)
        if (Regex("^#[0-9a-fA-F]{6}$").matches(c)) given.add(Color.parseColor(c))
    }
    return IntArray(4) { i ->
        when {
            i < given.size -> given[i]
            i > 0 && given.size > 1 -> given[1 + (i - 1) % (given.size - 1)]
            else -> FALLBACK_PALETTE[i]
        }
    }
}

private fun track(o: JSONObject): Track {
    val notes = o.optJSONArray("notes") ?: JSONArray()
    val n = notes.length()
    val start = FloatArray(n)
    val length = FloatArray(n)
    val pitch = IntArray(n)
    val velocity = FloatArray(n)
    for (i in 0 until n) {
        val x = notes.getJSONArray(i)
        start[i] = x.getDouble(0).toFloat()
        length[i] = x.getDouble(1).toFloat()
        pitch[i] = Math.round(x.getDouble(2)).toInt()
        velocity[i] = x.getDouble(3).toFloat()
    }
    val id = o.optString("id")
    return Track(id, o.optString("name", id).ifEmpty { id }, o.optString("role"), o.optString("engine"), o.optBoolean("sung"), start, length, pitch, velocity)
}

/** Read an .agbw. The whole file is held in memory: the largest in this repository is 30 MB. */
fun openAgbw(input: InputStream): Song {
    val files = HashMap<String, ByteArray>()
    ZipInputStream(input.buffered()).use { zip ->
        val buf = ByteArray(1 shl 16)
        while (true) {
            val e = zip.nextEntry ?: break
            if (e.isDirectory || !safe(e.name)) continue
            val out = ByteArrayOutputStream()
            while (true) {
                val n = zip.read(buf)
                if (n < 0) break
                out.write(buf, 0, n)
            }
            files[e.name] = out.toByteArray()
        }
    }
    fun need(name: String) = files[name] ?: throw AgbwException(".agbw 里缺少 $name")
    if (String(need("mimetype")).trim() != MIMETYPE) throw AgbwException("这不是 .agbw 文件")
    val manifest = JSONObject(String(need("web.json")))
    if (manifest.optString("format") != "agbw") throw AgbwException("web.json：format 不是 agbw")
    val version = manifest.optString("formatVersion")
    if (version.substringBefore('.') != "0") throw AgbwException("这个播放器读不了 agbw $version 版的文件")
    val songName = manifest.optString("song", "song.json")
    val timelineName = manifest.optString("timeline", "timeline.json")
    if (!safe(songName) || !safe(timelineName)) throw AgbwException("web.json：文件名不合法")
    val songJson = need(songName)
    val timeline = JSONObject(String(need(timelineName)))

    val samples = HashMap<String, ByteArray>()
    val assets = JSONObject(String(songJson)).optJSONObject("assets")
    if (assets != null) {
        for (id in assets.keys()) {
            val path = assets.optJSONObject(id)?.optString("path")
            if (!safe(path) || !path!!.endsWith(".wav", ignoreCase = true)) continue
            files[path]?.let { samples[id] = it }
        }
    }

    val tracks = ArrayList<Track>()
    timeline.optJSONArray("tracks")?.let { a -> for (i in 0 until a.length()) tracks.add(track(a.getJSONObject(i))) }
    val bars = timeline.optJSONArray("bars")?.let { a -> FloatArray(a.length()) { a.getDouble(it).toFloat() } }?.takeIf { it.isNotEmpty() } ?: floatArrayOf(0f)
    val sections = ArrayList<Section>()
    timeline.optJSONArray("sections")?.let { a ->
        for (i in 0 until a.length()) {
            val s = a.getJSONObject(i)
            sections.add(Section(s.optString("id"), s.optString("name", s.optString("id")), s.optDouble("from", 0.0).toFloat(), s.optDouble("to", 0.0).toFloat()))
        }
    }
    val lyrics = ArrayList<Line>()
    timeline.optJSONArray("lyrics")?.let { a ->
        for (i in 0 until a.length()) {
            val l = a.getJSONObject(i)
            val words = ArrayList<Word>()
            l.optJSONArray("words")?.let { w ->
                for (k in 0 until w.length()) {
                    val x = w.getJSONArray(k)
                    words.add(Word(x.getDouble(0).toFloat(), x.getDouble(1).toFloat(), x.getString(2)))
                }
            }
            lyrics.add(Line(l.optDouble("from", 0.0).toFloat(), l.optDouble("to", 0.0).toFloat(), l.optString("text"), words))
        }
    }
    val artists = manifest.optJSONArray("artists")
    val artist = (0 until (artists?.length() ?: 0)).joinToString(" · ") { artists!!.getJSONObject(it).optString("name") }

    return Song(
        title = manifest.optString("title", "无题"),
        artist = artist,
        key = manifest.optString("key"),
        scale = manifest.optString("scale"),
        bpm = timeline.optDouble("bpm", manifest.optDouble("bpm", 120.0)),
        seconds = timeline.optDouble("seconds", manifest.optDouble("seconds", 0.0)).toFloat(),
        palette = palette(manifest),
        scene = manifest.optJSONObject("visual")?.optString("scene") ?: "",
        tracks = tracks,
        bars = bars,
        beatsPerBar = (timeline.optJSONArray("meter")?.optInt(0, 4) ?: 4).coerceAtLeast(1),
        beatUnit = (timeline.optJSONArray("meter")?.optInt(1, 4) ?: 4).coerceAtLeast(1),
        sections = sections,
        lyrics = lyrics,
        songJson = songJson,
        samples = samples,
    )
}

/** What a song list needs to know of an .agbw, without keeping its samples. */
class SongCard(val source: String, val title: String, val artist: String, val seconds: Float, val bpm: Double, val key: String, val accent: Int)

fun cardOf(source: String, input: InputStream): SongCard {
    ZipInputStream(input.buffered()).use { zip ->
        while (true) {
            val e = zip.nextEntry ?: break
            if (e.name != "web.json") continue
            val m = JSONObject(String(zip.readBytes()))
            val artists = m.optJSONArray("artists")
            val artist = (0 until (artists?.length() ?: 0)).joinToString(" · ") { artists!!.getJSONObject(it).optString("name") }
            val key = listOf(m.optString("key"), m.optString("scale")).filter { it.isNotEmpty() }.joinToString(" ")
            return SongCard(source, m.optString("title", "无题"), artist, m.optDouble("seconds", 0.0).toFloat(), m.optDouble("bpm", 0.0), key, palette(m)[1])
        }
    }
    throw AgbwException(".agbw 里缺少 web.json")
}
