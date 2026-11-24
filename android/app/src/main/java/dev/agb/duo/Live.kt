package dev.agb.duo

import android.content.Context
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedWriter
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.CopyOnWriteArrayList
import java.util.zip.ZipInputStream

/** What the listener has set up for talking to the language model. Kept in the app's own storage. */
class Prefs(context: Context) {
    private val store = context.getSharedPreferences("settings", Context.MODE_PRIVATE)
    private val files = context.getExternalFilesDir(null)

    var apiKey: String
        get() = store.getString("apiKey", "") ?: ""
        set(v) = store.edit().putString("apiKey", v.trim()).apply()
    var baseUrl: String
        get() = store.getString("baseUrl", DEFAULT_BASE) ?: DEFAULT_BASE
        set(v) = store.edit().putString("baseUrl", v.trim().ifEmpty { DEFAULT_BASE }).apply()
    var models: String
        get() = store.getString("models", "") ?: ""
        set(v) = store.edit().putString("models", v.trim()).apply()

    /** A key file put into the app's folder on the device (`…/files/agentllm.key`) is taken in, and removed. True when one was. */
    fun importKeyFile(): Boolean {
        val f = File(files ?: return false, KEY_FILE)
        val key = try {
            f.readText().trim()
        } catch (e: Exception) {
            return false
        }
        if (key.isEmpty()) return false
        apiKey = key
        // it is kept in the app's own storage now; the file lay where other apps with access to files could read it
        f.delete()
        return true
    }

    companion object {
        const val DEFAULT_BASE = "https://agentllm.linkyun.co"
        const val KEY_FILE = "agentllm.key"
    }
}

/**
 * A performance that is made while it is heard: the AI playing from a brief (`agb perform`), or a
 * song played with the AI listening to what the listener says (`agb song play`). Both are the
 * engine's command-line program, packed into the app, run the way the Mac app runs it: it writes
 * one event a line, and a bar of sound for every track as files; it reads the listener's commands.
 *
 * Everything public may be called from any thread.
 */
class LiveSession(private val context: Context, val mixer: LiveMixer) : Playing {
    enum class Status { IDLE, PREPARING, PLAYING, FINALE, FINISHED, STOPPED, ERROR }

    /** A line of what was said and done: 0 the listener, 1 the AI, 2 a change to the music, 3 something that happened. */
    class Entry(val kind: Int, val text: String, val detail: String = "")

    @Volatile var status = Status.IDLE
        private set
    @Volatile override var song: Song? = null
        private set
    val feed = CopyOnWriteArrayList<Entry>()
    /** What the AI is doing now, or "". */
    @Volatile var activity = ""
        private set
    @Volatile var error = ""
        private set
    @Volatile var title = ""
        private set
    /** Playing a song (jam), not a performance from a brief. */
    @Volatile var ofSong = false
        private set
    @Volatile var phrase = 0
        private set
    @Volatile var phrases = 0
        private set
    @Volatile var bpm = 120.0
        private set
    @Volatile var transpose = 0
        private set
    @Volatile var transposeRange = 0..0
        private set
    /** The AI answers what the listener says. */
    @Volatile var interactive = false
        private set
    /** The listener has said something and the AI has not answered yet. */
    @Volatile var thinking = false
        private set
    /** The AI is writing the next phrase. */
    @Volatile var composing = false
        private set
    @Volatile var paused = false
        private set
    /** The folder of the performance: what "continue" and "save" work from. */
    @Volatile var project: File? = null
        private set

    private var process: java.lang.Process? = null
    private var commands: BufferedWriter? = null
    private val slots = ArrayList<String>()
    private var names: Map<String, Track> = emptyMap()
    private var pendingTitles = ArrayList<Pair<Long, String>>()
    private var ppqn = 960
    private var run = 0
    private var first = true
    private var done = false

    init {
        mixer.onEnded = {
            if (done && (status == Status.PLAYING || status == Status.FINALE)) {
                status = Status.FINISHED
                activity = "演奏完成"
            }
        }
    }

    private val program get() = File(context.applicationInfo.nativeLibraryDir, "libagbcli.so")
    private val workRoot get() = File(context.cacheDir, "live").apply { mkdirs() }
    val performances: File get() = File(context.getExternalFilesDir(null) ?: context.filesDir, "performances").apply { mkdirs() }

    val active get() = status == Status.PREPARING || status == Status.PLAYING || status == Status.FINALE
    val canContinue get() = !active && !ofSong && project?.let { File(it, "perform.json").exists() } == true
    val canSave get() = !active && project?.let { File(it, "project.json").exists() } == true

    // ------------------------------------------------------------------ Playing

    override fun time() = mixer.heard() / LiveMixer.RATE.toFloat()

    override fun loudness(out: FloatArray) {
        out[0] = if (mixer.playing) mixer.level() else 0f
        out[1] = if (mixer.playing) mixer.low() else 0f
    }

    override val waiting get() = status == Status.PREPARING || (active && mixer.dry)
    override val sounding get() = mixer.playing && !mixer.dry

    // ------------------------------------------------------------------ starting

    /**
     * The AI plays from `brief`. `again`: go on with the performance in [project] for more phrases.
     */
    fun perform(brief: String, phraseCount: Int, bars: Int, mock: Boolean, again: Boolean = false) {
        val prefs = Prefs(context)
        if (!mock && prefs.apiKey.isEmpty()) {
            fail("没有 API Key：请在“设置”里填写，或把密钥文件放进应用文件夹")
            return
        }
        val dir = if (again) project ?: return else File(performances, SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date()))
        val args = arrayListOf("perform", "--project", dir.absolutePath, "--phrases", "$phraseCount", "--bars", "$bars", "--realtime")
        if (brief.isNotBlank()) args += listOf("--brief", brief.trim())
        if (mock) args += "--mock"
        if (again) args += "--continue"
        if (prefs.models.isNotEmpty()) args += listOf("--model", prefs.models)
        begin(args, prefs, brief.trim().ifEmpty { "即兴" }, null, dir, keep = again)
    }

    /** Play a song with the AI listening: what the listener says changes the music. */
    fun jam(source: Song, open: () -> java.io.InputStream, mock: Boolean) {
        val prefs = Prefs(context)
        stop()
        Thread({
            try {
                workRoot.listFiles()?.forEach { it.deleteRecursively() }
                val stamp = System.currentTimeMillis()
                val folder = File(workRoot, "song-$stamp").apply { mkdirs() }
                // The .agbw holds the song the engine plays (its song.json and samples) as it is.
                ZipInputStream(open().buffered()).use { zip ->
                    while (true) {
                        val e = zip.nextEntry ?: break
                        if (e.isDirectory || e.name.contains("..") || e.name.startsWith("/")) continue
                        val f = File(folder, e.name)
                        f.parentFile?.mkdirs()
                        f.outputStream().use { zip.copyTo(it) }
                    }
                }
                val work = File(workRoot, "play-$stamp")
                val args = arrayListOf("song", "play", folder.absolutePath, "--work-dir", work.absolutePath, "--realtime")
                // With no key the song still plays; nobody answers. The built-in performer answers when asked to.
                if (mock) args += "--mock"
                if (prefs.models.isNotEmpty()) args += listOf("--model", prefs.models)
                begin(args, prefs, source.title, source, work, keep = false)
            } catch (e: Exception) {
                Log.e(TAG, "jam", e)
                fail(e.message ?: e.toString())
            }
        }, "agb-jam").start()
    }

    @Synchronized
    private fun begin(args: List<String>, prefs: Prefs, name: String, source: Song?, dir: File, keep: Boolean) {
        stopLocked()
        val mine = ++run
        if (!keep) {
            feed.clear()
            song = null
        }
        mixer.reset()
        slots.clear()
        pendingTitles = ArrayList()
        names = source?.tracks?.associateBy { it.id } ?: emptyMap()
        title = name
        ofSong = source != null
        project = dir
        error = ""
        phrase = 0
        phrases = 0
        first = true
        done = false
        paused = false
        thinking = false
        composing = false
        interactive = false
        transpose = 0
        transposeRange = 0..0
        status = Status.PREPARING
        activity = if (source != null) "正在准备「$name」" else "AI 正在构思开场"
        song = Song(
            title = name, artist = if (source != null) source.artist else "AI 乐手", key = source?.key ?: "", scale = source?.scale ?: "",
            bpm = source?.bpm ?: 120.0, seconds = 0f, palette = source?.palette ?: PALETTES[(name.hashCode() and 0x7fffffff) % PALETTES.size],
            scene = source?.scene ?: "orbits", tracks = emptyList(), bars = floatArrayOf(0f), beatsPerBar = source?.beatsPerBar ?: 4,
            beatUnit = source?.beatUnit ?: 4, sections = emptyList(), lyrics = emptyList(), songJson = ByteArray(0), samples = emptyMap(),
        )
        try {
            val builder = ProcessBuilder(listOf(program.absolutePath) + args).directory(workRoot)
            val env = builder.environment()
            if (prefs.apiKey.isNotEmpty()) env["AGENTLLM_API_KEY"] = prefs.apiKey
            env["AGENTLLM_API_BASE"] = prefs.baseUrl
            env["AGB_LIGHT_SINGERS"] = "1"
            env["AGB_FAST_CORES"] = "1"
            env["HOME"] = context.filesDir.absolutePath
            env["TMPDIR"] = context.cacheDir.absolutePath
            val p = builder.start()
            process = p
            commands = p.outputStream.bufferedWriter()
            Thread({ read(p, mine) }, "agb-live").start()
            Thread({
                // what the engine says beside its events; the stream is closed under us when it is stopped
                try {
                    p.errorStream.bufferedReader().forEachLine { Log.w(TAG, "agb: $it") }
                } catch (_: java.io.IOException) {
                }
            }, "agb-live-err").start()
        } catch (e: Exception) {
            Log.e(TAG, "start", e)
            fail("引擎没有启动：${e.message}")
        }
    }

    private fun fail(message: String) {
        error = message
        status = Status.ERROR
        activity = ""
        thinking = false
    }

    // ------------------------------------------------------------------ what the listener does

    private fun send(o: JSONObject) {
        try {
            synchronized(this) {
                val w = commands ?: return
                w.write(o.toString())
                w.newLine()
                w.flush()
            }
        } catch (e: Exception) {
            Log.w(TAG, "command: ${e.message}")
        }
    }

    fun chat(text: String) {
        val t = text.trim()
        if (t.isEmpty() || !active) return
        thinking = true
        send(JSONObject().put("type", "chat").put("text", t))
    }

    fun setTempo(value: Double) = send(JSONObject().put("type", "set_tempo").put("bpm", value))
    fun setTranspose(semitones: Int) = send(JSONObject().put("type", "set_transpose").put("semitones", semitones))
    fun humanState(state: JSONObject) {
        if (active) send(JSONObject().put("type", "human_state").put("state", state))
    }

    fun finish() {
        if (!active) return
        feed.add(Entry(3, "你请乐队收尾"))
        send(JSONObject().put("type", "finish"))
    }

    fun pause() {
        if (!active || paused) return
        paused = true
        mixer.pause()
        send(JSONObject().put("type", "pause"))
    }

    fun resume() {
        if (!active || !paused) return
        paused = false
        if (!first) mixer.play()
        send(JSONObject().put("type", "resume"))
    }

    @Synchronized
    fun stop() {
        val was = active
        stopLocked()
        if (was) {
            status = Status.STOPPED
            activity = "已停止"
        }
    }

    private fun stopLocked() {
        run++
        mixer.pause()
        try {
            commands?.close()
        } catch (_: Exception) {
        }
        commands = null
        process?.destroy()
        process = null
        thinking = false
        composing = false
    }

    /** Save the performance as an .agbsong file. Returns what to tell the listener. */
    fun save(): String {
        val dir = project ?: return "没有可以保存的演奏"
        val out = File(context.getExternalFilesDir("exports") ?: context.filesDir, "${dir.name.take(40)}.agbsong")
        return try {
            val p = ProcessBuilder(program.absolutePath, "song", "export", "--project", dir.absolutePath, "--out", out.absolutePath, "--title", title).redirectErrorStream(true).start()
            val text = p.inputStream.bufferedReader().readText()
            p.waitFor()
            val o = JSONObject(text.substring(text.indexOf('{').coerceAtLeast(0)))
            if (o.optBoolean("ok")) "已保存：exports/${out.name}（${o.optInt("bars")} 小节，${o.optInt("notes")} 个音）"
            else "没有保存：" + (o.optJSONObject("error")?.optString("message") ?: text.take(120))
        } catch (e: Exception) {
            "没有保存：${e.message}"
        }
    }

    // ------------------------------------------------------------------ what the engine says

    private fun read(p: java.lang.Process, mine: Int) {
        try {
            p.inputStream.bufferedReader().forEachLine { line ->
                if (mine != run || line.isBlank()) return@forEachLine
                try {
                    event(JSONObject(line))
                } catch (e: Exception) {
                    Log.w(TAG, "event: ${e.message}: ${line.take(200)}")
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "reading: ${e.message}")
        }
        if (mine != run) return
        val code = try {
            p.waitFor()
        } catch (e: Exception) {
            -1
        }
        if (!done && status != Status.ERROR) fail("引擎提前结束了（$code）")
    }

    private fun event(o: JSONObject) {
        when (o.optString("type")) {
            "session" -> {
                phrases = o.optInt("phrases")
                phrase = o.optInt("firstPhrase")
                ppqn = o.optInt("ppqn", 960)
                interactive = o.optBoolean("interactive", !ofSong)
                if (o.has("bpm")) bpm = o.optDouble("bpm", bpm)
                transpose = o.optInt("transpose")
                o.optJSONArray("transposeRange")?.let { transposeRange = it.optInt(0)..it.optInt(1) }
                mixer.bpm = bpm
                val skipped = o.optJSONArray("skipped")
                if (skipped != null && skipped.length() > 0) feed.add(Entry(3, "有 ${skipped.length()} 处换了演奏方式", skipped.optJSONObject(0)?.optString("message") ?: ""))
            }
            "phrase_request" -> {
                composing = true
                if (o.optBoolean("answer")) thinking = true
                activity = when {
                    o.optBoolean("answer") -> "乐手正在回应你"
                    o.optBoolean("final") -> "AI 正在写终章"
                    else -> "AI 正在构思第 ${o.optInt("phrase") + 1} 句"
                }
            }
            "llm_reply" -> {
                composing = false
                activity = "${o.optString("model")} 写好了（${o.optInt("elapsedMs") / 1000} 秒）"
            }
            "llm_error" -> {
                activity = "模型暂时没有回应，正在重试"
                Log.w(TAG, "llm: ${o.optString("message").take(300)}")
            }
            "txn_rejected" -> activity = "引擎退回了这一句（${o.optString("code")}），AI 正在修正"
            "phrase_skipped" -> {
                thinking = false
                composing = false
                feed.add(Entry(3, "这一句没写成，音乐照原样继续", o.optString("message").take(120)))
            }
            "chat" -> {
                val ai = o.optString("who") == "ai"
                if (ai) thinking = false
                feed.add(Entry(if (ai) 1 else 0, o.optString("text")))
            }
            "change" -> {
                val answer = o.optBoolean("answer")
                if (!answer || !ofSong) phrase = o.optInt("phrase") + 1
                val name = o.optString("title").ifEmpty { if (answer) "改动" else "第 $phrase 句" }
                val late = o.optInt("lateBars")
                val detail = listOfNotNull(o.optString("commentary").takeIf { it.isNotEmpty() }, if (late > 0) "AI 晚到 $late 小节" else null).joinToString(" · ")
                feed.add(Entry(2, name, detail))
                if (!answer || !ofSong) synchronized(pendingTitles) { pendingTitles.add(o.optLong("tick") to name) }
                if (answer) thinking = false
                composing = false
                activity = when {
                    o.optBoolean("final") -> "终章已就位"
                    answer -> "乐手改了：$name"
                    ofSong -> "正在播放「$title」"
                    else -> "第 $phrase 句：$name"
                }
                if (o.optBoolean("final")) status = Status.FINALE
            }
            "human_change" -> when (o.optString("kind")) {
                "tempo" -> {
                    bpm = o.optDouble("bpm", bpm)
                    feed.add(Entry(3, "BPM → ${Math.round(bpm)}"))
                }
                "transpose" -> {
                    transpose = o.optInt("semitones")
                    feed.add(Entry(3, "移调 ${"%+d".format(transpose)}"))
                }
            }
            "human_rejected" -> feed.add(Entry(3, "没有改成", o.optString("error").take(120)))
            "finishing" -> {
                status = Status.FINALE
                activity = "正在收尾"
            }
            "performer" -> if (o.optBoolean("connected")) feed.add(Entry(3, "乐手已就位"))
            "chunk" -> chunk(o)
            "done" -> {
                done = true
                // the bars as files have all been read; the whole performance stays (renders/full)
                project?.let { File(it, "renders/perform").deleteRecursively() }
                thinking = false
                composing = false
                if (mixer.ended) {
                    status = Status.FINISHED
                    activity = "演奏完成"
                } else activity = "已全部送出，播放到结尾"
            }
            "fatal" -> fail(o.optString("message").ifEmpty { "引擎出错" })
        }
    }

    private fun slotOf(id: String, engine: String): Int {
        val at = slots.indexOf(id)
        if (at >= 0) return at
        if (slots.size >= LiveMixer.SLOTS) return -1
        slots.add(id)
        val known = names[id]
        val s = song ?: return slots.size - 1
        val none = FloatArray(0)
        s.tracks = s.tracks + Track(id, known?.name ?: id, known?.role ?: "", engine, known?.sung ?: (engine == "voice.chimera"), none, none, IntArray(0), none)
        s.revision++
        return slots.size - 1
    }

    private fun chunk(o: JSONObject) {
        val s = song ?: return
        val start = o.optLong("startSample")
        val samples = o.optInt("samples")
        val startTick = o.optLong("startTick")
        val tempo = o.optDouble("tempoBpm", bpm)
        bpm = tempo
        mixer.bpm = tempo
        val at = start / LiveMixer.RATE.toFloat()

        // the tracks, in the order the engine has them, and the sound of each for this bar
        val tracks = o.optJSONArray("tracks") ?: JSONArray()
        for (i in 0 until tracks.length()) tracks.getJSONObject(i).let { slotOf(it.optString("trackId"), it.optString("engineId")) }
        val stems = arrayOfNulls<FloatArray>(LiveMixer.SLOTS)
        val files = o.optJSONArray("stems") ?: JSONArray()
        for (i in 0 until files.length()) {
            val f = files.getJSONObject(i)
            val slot = slotOf(f.optString("trackId"), "")
            if (slot < 0) continue
            val file = File(f.optString("wav")).let { if (it.isAbsolute) it else File(workRoot, it.path) }
            val sound = readWav(file)
            // the sound is in memory now; a performance fills the disk quickly otherwise
            file.delete()
            if (sound.any { it != 0f }) stems[slot] = if (sound.size >= samples) sound else sound.copyOf(samples)
        }
        mixer.add(LiveMixer.Chunk(start, samples, stems))

        // the notes of the bar, for the pictures
        val perTick = 60.0 / (tempo * ppqn)
        val events = o.optJSONArray("events") ?: JSONArray()
        val byTrack = HashMap<String, ArrayList<FloatArray>>()
        for (i in 0 until events.length()) {
            val e = events.getJSONArray(i)
            val time = at + ((e.getLong(0) - startTick) * perTick).toFloat()
            byTrack.getOrPut(e.getString(1)) { ArrayList() }.add(floatArrayOf(time, (e.getLong(5) * perTick).toFloat(), e.getDouble(3).toFloat(), e.getDouble(4).toFloat()))
        }
        for ((id, notes) in byTrack) {
            val slot = slots.indexOf(id)
            val track = s.tracks.getOrNull(slot) ?: continue
            notes.sortBy { it[0] }
            // never earlier than what is there: the arrays stay sorted
            val floor = track.start.lastOrNull() ?: 0f
            if (track.count == 0) s.revision++ // its first notes: the scenes draw it from now on
            track.append(FloatArray(notes.size) { maxOf(floor, notes[it][0]) }, FloatArray(notes.size) { notes[it][1] }, IntArray(notes.size) { notes[it][2].toInt() }, FloatArray(notes.size) { notes[it][3] })
        }

        // bars and sections
        if (at > 0f || s.bars.size > 1) s.bars = s.bars + at
        s.seconds = (start + samples) / LiveMixer.RATE.toFloat()
        val due = synchronized(pendingTitles) {
            val d = pendingTitles.filter { it.first <= startTick }
            pendingTitles.removeAll(d.toSet())
            d
        }
        for ((_, name) in due) {
            val closed = s.sections.dropLast(1) + (s.sections.lastOrNull()?.let { listOf(Section(it.id, it.name, it.from, at)) } ?: emptyList())
            s.sections = closed + Section("p${closed.size}", name, at, Float.MAX_VALUE)
        }

        if (o.optBoolean("last")) mixer.endsAt(start + samples)
        if (first) {
            first = false
            status = if (status == Status.FINALE) Status.FINALE else Status.PLAYING
            if (!paused) mixer.play()
        }
    }

    companion object {
        private const val TAG = "agb"
        /** For a performance from a brief (a song brings its own colours): background, then three accents. */
        private val PALETTES = arrayOf(
            intArrayOf(0xff0b0d14.toInt(), 0xffe8825a.toInt(), 0xfff4ead8.toInt(), 0xff5fb8b0.toInt()),
            intArrayOf(0xff0e0c1c.toInt(), 0xffffb048.toInt(), 0xffff6b81.toInt(), 0xff56d6c4.toInt()),
            intArrayOf(0xff05060c.toInt(), 0xff00e5ff.toInt(), 0xffff2bd6.toInt(), 0xffff7a1a.toInt()),
            intArrayOf(0xff071013.toInt(), 0xff7be0a4.toInt(), 0xffe9f2d8.toInt(), 0xffd9a441.toInt()),
        )
    }
}
