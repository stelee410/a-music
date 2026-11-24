package dev.agb.duo

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.InputStream
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.abs
import kotlin.math.pow
import kotlin.math.roundToInt

/** A pad: a sound the engine renders once (`agb oneshot` with these arguments) and the listener plays. */
class Pad(val file: String, val name: String, val args: List<String>) {
    @Volatile var sound: FloatArray? = null
    /** The listener's own sound is in place of the engine's. */
    @Volatile var own = false
    // What the listener has set for it: pitch in semitones (−12…12), and 0…1 for the others.
    @Volatile var pitch = 0f
    @Volatile var decay = 1f
    @Volatile var tone = 1f
    @Volatile var volume = 0.9f
}

class PadBank(val title: String, space: Float, val pads: List<Pad>) {
    /** How much of the bank goes to the reverb. */
    @Volatile var space = space
}

/** The pages of the console. */
enum class Page(val title: String) { SCORE("乐谱"), JAM("jam"), CHAT("对话"), MIXER("调音台"), PADS("打击垫"), HEALING("疗愈") }

/**
 * Everything the two screens share: the player of finished songs, the live session (the AI
 * performing, or a song played with the AI listening), the mixer, the pads, and what the pad's
 * keys do. One for the app; made by the first activity.
 */
object Studio {
    private const val TAG = "agb"
    lateinit var app: Context
        private set
    val player by lazy { Player() }
    val mixer by lazy { LiveMixer() }
    val live by lazy { LiveSession(app, mixer) }
    val prefs by lazy { Prefs(app) }
    private val main = Handler(Looper.getMainLooper())
    private var ready = false

    @Volatile var cards: List<SongCard> = emptyList()
        private set
    @Volatile var current = -1
        private set
    @Volatile var sceneKind = 0
        private set
    @Volatile var shownTranspose = 0
        private set
    @Volatile var shownBpm = 0.0
        private set
    /** Live: the session is what is heard and shown; otherwise the player. */
    @Volatile var liveMode = false
        private set
    @Volatile var page = Page.SCORE
    @Volatile var lyricsOn = true
    /** The left stick: the viewer leaning to look at the stage. */
    @Volatile var leanX = 0f
    @Volatile var leanY = 0f
    /** A line for the listener that goes away by itself. */
    @Volatile var notice = ""
        private set
    private var noticeUntil = 0L
    @Volatile var padBank = 0
        set(v) {
            field = v
            if (ready) loadBank()
        }
    /** The console has its list of songs, its launcher or its settings open: the pad moves in them. */
    @Volatile var overlay: Overlay? = null

    /** What lies over the console, and how the pad's keys reach it. */
    interface Overlay {
        fun move(by: Int)
        fun pick()
        fun close()
    }

    val playing: Playing get() = if (liveMode) live else player
    val sceneName get() = SCENES[sceneKind].name

    fun start(context: Context) {
        if (ready) return
        ready = true
        app = context.applicationContext
        prefs.importKeyFile()
        player.onEnded = { main.post { if (!liveMode && auto) advance(atEnd = false) } }
        // a few seconds before the end the next song is made ready: it comes in where the last bar ends
        player.onNearEnd = { main.post { if (!liveMode && auto && aligned) advance(atEnd = true) } }
        player.onSwitched = { song ->
            main.post {
                if (cueIndex >= 0) current = cueIndex
                cueIndex = -1
                shownTranspose = 0
                shownBpm = song.bpm
                sceneKind = sceneIndex(song.scene)
            }
        }
        Thread({ findSongs() }, "agb-songs").start()
        Thread({ renderPads() }, "agb-pads").start()
    }

    fun say(text: String, seconds: Int = 4) {
        notice = text
        noticeUntil = SystemClock.uptimeMillis() + seconds * 1000L
        main.postDelayed({ if (SystemClock.uptimeMillis() >= noticeUntil) notice = "" }, seconds * 1000L + 50)
    }

    /** The listener is at the console: the mixer's output is kept running, so that a pad sounds the moment it is hit. */
    fun wake() {
        if (ready) mixer.awake = true
    }

    /** The listener has left: nothing goes on sounding. */
    fun rest() {
        if (!ready) return
        mixer.awake = false
        Jam.rest()
        player.pause()
        if (live.active && !live.paused) live.pause()
        mixer.toneOn = false
        mixer.binauralOn = false
        mixer.noiseOn = false
        if (mixer.isRecording) mixer.stopRecording()
    }

    // ------------------------------------------------------------------ the songs

    /** Where a listener puts songs of their own: Android/data/dev.agb.duo/files/songs on the device. */
    private fun ownSongs(): File? = app.getExternalFilesDir("songs")

    fun open(source: String): InputStream = if (source.startsWith("/")) File(source).inputStream() else app.assets.open(source)

    private fun findSongs() {
        val sources = ArrayList<String>()
        app.assets.list("songs")?.sorted()?.filter { it.endsWith(".agbw") }?.forEach { sources.add("songs/$it") }
        ownSongs()?.listFiles()?.sortedBy { it.name }?.filter { it.name.endsWith(".agbw") }?.forEach { sources.add(it.absolutePath) }
        val found = ArrayList<SongCard>()
        for (s in sources) {
            try {
                open(s).use { found.add(cardOf(s, it)) }
            } catch (e: Exception) {
                Log.w(TAG, "$s: ${e.message}")
            }
        }
        main.post {
            cards = found
            if (found.isNotEmpty() && current < 0) choose(0, play = false)
        }
    }

    // The playlist: all the songs, in their order.
    private val listStore by lazy { app.getSharedPreferences("playlist", Context.MODE_PRIVATE) }
    /** At the end of a song the next one plays. */
    var auto: Boolean
        get() = listStore.getBoolean("auto", true)
        set(v) = listStore.edit().putBoolean("auto", v).apply()
    /** Another song comes in on a bar line of the one that is playing, without a gap. */
    var aligned: Boolean
        get() = listStore.getBoolean("aligned", true)
        set(v) = listStore.edit().putBoolean("aligned", v).apply()
    /** After the last song the first plays again. */
    var loop: Boolean
        get() = listStore.getBoolean("loop", true)
        set(v) = listStore.edit().putBoolean("loop", v).apply()
    /** The song that is waiting to come in, or −1. */
    @Volatile var cueIndex = -1
        private set

    /** The next song of the list, at the end of this one. */
    private fun advance(atEnd: Boolean) {
        if (cards.isEmpty() || cueIndex >= 0) return
        val next = current + 1
        if (next >= cards.size && !loop) return
        if (atEnd) cueSong(next.mod(cards.size), atEnd = true) else choose(next.mod(cards.size), play = true)
    }

    /** Make a song ready to come in on a bar line of the one that is playing. */
    private fun cueSong(index: Int, atEnd: Boolean) {
        val card = cards.getOrNull(index) ?: return
        cueIndex = index
        main.removeCallbacks(commit)
        if (!atEnd) say("下一首《${card.title}》在小节线切入", 5)
        Thread({
            try {
                val song = open(card.source).use { openAgbw(it) }
                main.post {
                    if (cueIndex != index || liveMode) return@post
                    if (player.state == Player.State.PLAYING) player.cue(song, atEnd)
                    else {
                        // nothing is sounding any more: simply play it
                        cueIndex = -1
                        current = index
                        shownTranspose = 0
                        shownBpm = song.bpm
                        sceneKind = sceneIndex(song.scene)
                        player.load(song, true)
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "${card.source}: ${e.message}", e)
                main.post {
                    cueIndex = -1
                    say("打不开这首曲子：${e.message}")
                }
            }
        }, "agb-open").start()
    }

    fun choose(index: Int) {
        if (!liveMode && aligned && index != current && player.state == Player.State.PLAYING) cueSong(index, atEnd = false)
        else choose(index, play = true)
    }

    private fun choose(index: Int, play: Boolean) {
        val card = cards.getOrNull(index) ?: return
        leaveLive()
        cueIndex = -1
        current = index
        main.removeCallbacks(commit)
        shownTranspose = 0
        shownBpm = card.bpm
        player.pause()
        Thread({
            try {
                val song = open(card.source).use { openAgbw(it) }
                main.post {
                    if (cards.getOrNull(current) !== card || liveMode) return@post // another was chosen meanwhile
                    shownBpm = song.bpm
                    sceneKind = sceneIndex(song.scene)
                    player.load(song, play)
                }
            } catch (e: Exception) {
                Log.e(TAG, "${card.source}: ${e.message}", e)
                main.post { say("打不开这首曲子：${e.message}") }
            }
        }, "agb-open").start()
    }

    fun step(by: Int) {
        if (cards.isEmpty()) return
        // from the song that is waiting, if one is: two presses go two songs on
        val from = if (cueIndex >= 0) cueIndex else current
        choose((from + by).mod(cards.size))
    }

    fun nextScene() {
        sceneKind = (sceneKind + 1) % SCENES.size
    }

    // ------------------------------------------------------------------ tuning

    /** The reference pitch, A4 in Hz: 440 as written, 432 a little lower… The whole band moves; pads and healing sounds do not. */
    @Volatile var a4 = 440f
        private set

    private val retune = Runnable {
        val ratio = a4 / 440.0
        player.setTuning(ratio)
        mixer.rate = ratio.toFloat()
        // the engine of a performance is asked for the tempo that makes up for it
        if (liveMode && live.active) live.setTempo(shownBpm / ratio)
        touched()
    }

    fun tune(hz: Float) {
        a4 = ((hz.coerceIn(426f, 454f)) * 10).roundToInt() / 10f
        main.removeCallbacks(retune)
        main.postDelayed(retune, 500)
    }

    // ------------------------------------------------------------------ key and tempo

    // Key and tempo are shown at once and played a moment later: several presses, one change.
    private val commit = Runnable {
        if (liveMode) {
            if (shownBpm.roundToInt() != (live.bpm * a4 / 440.0).roundToInt()) live.setTempo(shownBpm * 440.0 / a4)
            if (shownTranspose != live.transpose) live.setTranspose(shownTranspose)
        } else {
            player.setTranspose(shownTranspose)
            player.setBpm(shownBpm)
        }
    }

    fun transposeBy(semitones: Int) {
        if (liveMode) {
            if (!live.ofSong || !live.active) return
            shownTranspose = (shownTranspose + semitones).coerceIn(live.transposeRange.first, live.transposeRange.last)
        } else {
            if (player.song == null) return
            shownTranspose = (shownTranspose + semitones).coerceIn(-12, 12)
        }
        main.removeCallbacks(commit)
        main.postDelayed(commit, if (liveMode) 600 else 450)
    }

    fun tempoBy(bpm: Int) {
        shownBpm = if (liveMode) {
            if (!live.active) return
            (shownBpm.roundToInt() + bpm).toDouble().coerceIn(40.0, 200.0)
        } else {
            val own = player.song?.bpm ?: return
            (shownBpm.roundToInt() + bpm).toDouble().coerceIn(own * 0.5, own * 1.5)
        }
        main.removeCallbacks(commit)
        main.postDelayed(commit, if (liveMode) 600 else 450)
    }

    fun resetKeyAndTempo() {
        if (liveMode) return
        val own = player.song?.bpm ?: return
        shownTranspose = 0
        shownBpm = own
        main.removeCallbacks(commit)
        main.post(commit)
    }

    fun seekBars(by: Int) {
        if (liveMode) return
        val song = player.song ?: return
        val bar = song.barAt(player.time() + 0.05f)
        player.seek(song.barStart((bar + by).coerceIn(0, song.bars.size - 1)))
    }

    fun seekTo(time: Float) {
        if (liveMode) return
        val song = player.song ?: return
        // to the nearest bar line: a jump that lands on the beat
        val bar = song.barAt(time)
        val nearest = if (bar + 1 < song.bars.size && time - song.barStart(bar) > song.barEnd(bar) - time) bar + 1 else bar
        player.seek(song.barStart(nearest))
    }

    fun togglePlay() {
        if (!liveMode) player.toggle()
        else if (live.active) {
            if (live.paused) live.resume() else live.pause()
        }
    }

    // ------------------------------------------------------------------ live: the AI performs, or listens

    private fun enterLive() {
        player.pause()
        main.removeCallbacks(commit)
        liveMode = true
        page = Page.CHAT
        shownTranspose = 0
    }

    /** The AI plays from what the listener wrote. */
    fun perform(brief: String, phrases: Int, bars: Int, mock: Boolean) {
        enterLive()
        sceneKind = 0
        shownBpm = 120.0
        live.perform(brief, phrases, bars, mock)
        followTempo()
    }

    /** The song that is loaded, played with the AI listening. */
    fun jam(mock: Boolean) {
        val song = player.song ?: return say("先选一首曲子")
        val card = cards.getOrNull(current) ?: return
        enterLive()
        shownBpm = song.bpm
        live.jam(song, { open(card.source) }, mock)
        followTempo()
        if (!mock && prefs.apiKey.isEmpty()) say("没有 API Key：曲子照常播放，乐手不会回应", 6)
    }

    /** More phrases for the performance that has ended. */
    fun again() {
        if (!live.canContinue) return
        live.perform(live.title, 4, 8, mock = false, again = true)
        followTempo()
    }

    // The tempo shown follows what the session plays until the listener touches it.
    private val follow = object : Runnable {
        override fun run() {
            if (!liveMode) return
            if (!main.hasCallbacks(commit)) {
                shownBpm = live.bpm * a4 / 440.0 // what is heard: the band is played that much slower or faster
                shownTranspose = live.transpose
            }
            main.postDelayed(this, 500)
        }
    }

    private fun followTempo() {
        main.removeCallbacks(follow)
        main.postDelayed(follow, 500)
    }

    fun save() {
        say("正在保存…", 30)
        Thread({ live.save().let { main.post { say(it, 8) } } }, "agb-save").start()
    }

    /** Back to the songs. */
    fun leaveLive() {
        if (!liveMode) return
        live.stop()
        mixer.reset()
        liveMode = false
        if (page == Page.CHAT || page == Page.MIXER) page = Page.SCORE
        shownTranspose = player.transpose
        shownBpm = player.tempo
    }

    fun chat(text: String) {
        if (text.isBlank()) return
        if (!live.active) return say("演奏开始后才能对乐手说话")
        live.chat(text)
    }

    // What the listener has done is told to the AI a moment after the last touch.
    private val tell = Runnable { if (liveMode && live.active) live.humanState(humanState()) }

    fun touched() {
        main.removeCallbacks(tell)
        main.postDelayed(tell, 1500)
    }

    private val padHits = ArrayList<Pair<Long, String>>()

    private fun humanState(): JSONObject {
        val now = SystemClock.uptimeMillis()
        val recent = synchronized(padHits) {
            padHits.removeAll { it.first < now - 60_000 }
            padHits.map { it.second }
        }
        val s = JSONObject()
        s.put("masterFilter", if (mixer.filter >= 0.995f) "open" else "low-pass at ${(200 * 100.0.pow(mixer.filter.toDouble())).toInt()} Hz")
        s.put("reverb", (mixer.reverb * 100).roundToInt() / 100.0)
        s.put("delay", (mixer.delay * 100).roundToInt() / 100.0)
        s.put("tuningA4Hz", a4.toDouble())
        val band = JSONObject()
        live.song?.tracks?.forEachIndexed { i, t ->
            if (i < LiveMixer.SLOTS && (mixer.muted[i] || abs(mixer.volume[i] - LiveMixer.DEFAULT_VOLUME) > 0.05f)) {
                band.put(t.id, JSONObject().put("volume", (mixer.volume[i] * 100).roundToInt() / 100.0).put("muted", mixer.muted[i]))
            }
        }
        if (band.length() > 0) s.put("bandMixer", band)
        if (recent.isNotEmpty()) s.put("padsLastMinute", JSONObject().put("hits", recent.size).put("sounds", JSONArray(recent.distinct())))
        if (mixer.toneOn) s.put("toneGenerator", JSONObject().put("hz", mixer.toneHz.toDouble()))
        if (mixer.binauralOn) s.put("binauralBeat", JSONObject().put("carrierHz", mixer.carrierHz.toDouble()).put("beatHz", mixer.beatHz.toDouble()).put("band", bandName(mixer.beatHz)))
        if (mixer.noiseOn) s.put("noise", arrayOf("white", "pink", "brown")[mixer.noiseColor.coerceIn(0, 2)])
        return s
    }

    fun bandName(hz: Float) = when {
        hz < 4 -> "delta"
        hz < 8 -> "theta"
        hz < 13 -> "alpha"
        hz < 30 -> "beta"
        else -> "gamma"
    }

    // ------------------------------------------------------------------ recording

    fun toggleRecording() {
        if (mixer.isRecording) {
            mixer.stopRecording()
            say("录音已保存到 recordings/")
        } else {
            val dir = app.getExternalFilesDir("recordings") ?: app.filesDir
            mixer.startRecording(File(dir, "live-${SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())}.wav"))
            say(if (liveMode) "开始录音" else "开始录音（录的是演奏、打击垫和疗愈声；曲目播放器的声音不在内）", 6)
        }
    }

    // ------------------------------------------------------------------ pads

    private fun drum(file: String, name: String, pitch: Int, preset: String = "classic_kit", tail: Int = 2500) =
        Pad(file, name, listOf("--engine", "drum.analog808style", "--preset", "preset:drum.analog808style:drums:$preset@v1.0", "--pitch", "$pitch", "--len-ms", "100", "--tail-ms", "$tail"))

    private fun modal(file: String, name: String, preset: String, pitch: String = "C4", hz: Double? = null, tail: Int = 15000) =
        Pad(file, name, listOf("--engine", "synth.modal", "--preset", "preset:synth.modal:$preset@v1.0", "--pitch", pitch, "--len-ms", "100", "--tail-ms", "$tail") + (hz?.let { listOf("--params", "{\"freqHz\":$it}") } ?: emptyList()))

    private fun synth(file: String, name: String, params: String, pitch: String, len: Int, tail: Int) =
        Pad(file, name, listOf("--engine", "synth.subtractive", "--params", params, "--pitch", pitch, "--len-ms", "$len", "--tail-ms", "$tail"))

    /** The same five banks of eight the Mac app has. */
    val banks: List<PadBank> by lazy {
        listOf(
            PadBank("鼓组", 0.06f, listOf(
                drum("kit-kick", "底鼓", 36), drum("kit-punch", "重底鼓", 35, preset = "punchy_kit"), drum("kit-snare", "军鼓", 38), drum("kit-clap", "拍手", 39),
                drum("kit-ch", "闭镲", 42), drum("kit-oh", "开镲", 46), drum("kit-rim", "边击", 37), drum("kit-crash", "吊镲", 49, tail = 5000),
            )),
            PadBank("打击乐", 0.1f, listOf(
                drum("perc-tomlo", "低嗵", 41), drum("perc-tomhi", "高嗵", 48), drum("perc-bongo", "手鼓", 60), drum("perc-conga", "康加", 63),
                drum("perc-shaker", "沙锤", 70), drum("perc-tamb", "铃鼓", 54), drum("perc-claves", "响棒", 75), drum("perc-ride", "叮叮镲", 51, tail = 5000),
            )),
            PadBank("颂钵", 0.22f, listOf(
                modal("bowl-c", "C 根", "bowl:singing_bowl", "C3"), modal("bowl-d", "D 腹", "bowl:singing_bowl", "D3"), modal("bowl-e", "E 脐", "bowl:singing_bowl", "E3"),
                modal("bowl-f", "F 心", "bowl:singing_bowl", "F3"), modal("bowl-g", "G 喉", "bowl:singing_bowl", "G3"), modal("bowl-a", "A 眉", "bowl:singing_bowl", "A3"),
                modal("bowl-b", "B 顶", "bowl:singing_bowl", "B3"), modal("bowl-crystal", "水晶钵", "bowl:crystal_bowl", "C4"),
            )),
            PadBank("音叉", 0.15f, listOf(
                modal("fork-om", "OM 136", "fork:tuning_fork", hz = 136.1, tail = 12000), modal("fork-174", "174", "fork:tuning_fork", hz = 174.0, tail = 12000),
                modal("fork-285", "285", "fork:tuning_fork", hz = 285.0, tail = 12000), modal("fork-396", "396", "fork:tuning_fork", hz = 396.0, tail = 12000),
                modal("fork-417", "417", "fork:tuning_fork", hz = 417.0, tail = 12000), modal("fork-528", "528", "fork:tuning_fork", hz = 528.0, tail = 12000),
                modal("fork-639", "639", "fork:tuning_fork", hz = 639.0, tail = 12000), modal("fork-741", "741", "fork:tuning_fork", hz = 741.0, tail = 12000),
            )),
            PadBank("钟与氛围", 0.3f, listOf(
                modal("amb-chime", "管钟", "chime:tubular_chime", "G5", tail = 7000), modal("amb-bell", "寺钟", "bell:temple_bell", "C4", tail = 10000),
                modal("amb-gong", "风锣", "gong:wind_gong", "A1", tail = 14000), modal("amb-deepbowl", "深钵", "bowl:deep_bowl", "F2", tail = 18000),
                synth("amb-drone", "低吟", """{"osc.wave":"saw","osc2.wave":"saw","osc2.level":0.5,"osc2.detuneCents":9,"filter.cutoffHz":700,"filter.envAmount":0.5,"amp.attackMs":900,"amp.sustain":0.85,"amp.releaseMs":2500,"gainDb":-10}""", "A1", 4000, 2600),
                synth("amb-wind", "风声", """{"osc.wave":"noise","filter.type":"bp","filter.cutoffHz":500,"filter.resonance":0.6,"filter.envAmount":2.5,"fenv.attackMs":2200,"fenv.sustain":0.6,"amp.attackMs":1500,"amp.sustain":0.8,"amp.releaseMs":2000,"gainDb":-12}""", "C3", 3500, 2200),
                synth("amb-waves", "海浪", """{"osc.wave":"noise","filter.cutoffHz":900,"filter.resonance":0.1,"filter.envAmount":1.5,"fenv.attackMs":2500,"fenv.decayMs":3000,"fenv.sustain":0.2,"amp.attackMs":2200,"amp.decayMs":2500,"amp.sustain":0.4,"amp.releaseMs":3000,"gainDb":-9}""", "C3", 4500, 3200),
                synth("amb-rain", "雨声", """{"osc.wave":"noise","filter.type":"hp","filter.cutoffHz":2500,"filter.resonance":0.2,"filter.envAmount":0,"amp.attackMs":600,"amp.sustain":0.7,"amp.releaseMs":1500,"gainDb":-16}""", "C3", 5000, 1600),
            )),
        )
    }

    /** Where the listener puts sounds of their own for the pads: `<name of the pad's file>.wav`, e.g. kit-kick.wav. */
    private fun ownPads(): File? = app.getExternalFilesDir("pads")

    private val padStore by lazy { app.getSharedPreferences("pads", Context.MODE_PRIVATE) }

    private val keepLater = Runnable { keepPads() }

    /** Keep what the listener has set for the pads, once they have stopped turning the knobs. */
    fun padsChanged() {
        main.removeCallbacks(keepLater)
        main.postDelayed(keepLater, 800)
    }

    private fun keepPads() {
        loadBank()
        val e = padStore.edit()
        for (bank in banks) {
            e.putFloat("space:${bank.title}", bank.space)
            for (p in bank.pads) e.putString(p.file, "${p.pitch},${p.decay},${p.tone},${p.volume}")
        }
        e.apply()
    }

    private fun recallPads() {
        for (bank in banks) {
            bank.space = padStore.getFloat("space:${bank.title}", bank.space)
            for (p in bank.pads) {
                val v = padStore.getString(p.file, null)?.split(',')?.mapNotNull { it.toFloatOrNull() } ?: continue
                if (v.size == 4) {
                    p.pitch = v[0]
                    p.decay = v[1]
                    p.tone = v[2]
                    p.volume = v[3]
                }
            }
        }
    }

    /** The pads' sounds: the listener's own where there is one, else rendered by the engine the first time and read from the app's files after that. */
    private fun renderPads() {
        recallPads()
        val mine = ownPads()
        val dir = File(app.filesDir, "pads-v1").apply { mkdirs() }
        val program = File(app.applicationInfo.nativeLibraryDir, "libagbcli.so").absolutePath
        for (bank in banks) for (pad in bank.pads) {
            val wav = File(dir, "${pad.file}.wav")
            try {
                val given = mine?.let { File(it, "${pad.file}.wav") }
                if (given != null && given.exists()) {
                    val sound = readWav(given)
                    if (sound.isNotEmpty()) {
                        pad.sound = sound
                        pad.own = true
                        continue
                    }
                }
                if (!wav.exists() || wav.length() < 100) {
                    val tmp = File(dir, "${pad.file}.tmp.wav")
                    val p = ProcessBuilder(listOf(program, "oneshot") + pad.args + listOf("--out", tmp.absolutePath)).redirectErrorStream(true).start()
                    p.inputStream.readBytes()
                    if (p.waitFor() == 0) tmp.renameTo(wav) else tmp.delete()
                }
                if (wav.exists()) pad.sound = readWav(wav)
            } catch (e: Exception) {
                Log.w(TAG, "pad ${pad.file}: ${e.message}")
            }
            // the bank on the screen is playable as soon as its last pad is there
            if (bank === banks.getOrNull(padBank) && pad === bank.pads.last()) loadBank()
        }
        loadBank()
    }

    fun hitPad(index: Int) {
        val bank = banks.getOrNull(padBank) ?: return
        val pad = bank.pads.getOrNull(index) ?: return
        val sound = pad.sound ?: return say("这个垫子的声音还在准备")
        // At once from the pad's own output when it is loaded there (the mixer then adds only the
        // reverb); else through the mixer, a little late.
        val direct = padLoadedBank == padBank && padPlayer.hit(index, pad.volume, pad.pitch)
        mixer.hit(sound, pad.volume, bank.space, pad.pitch, pad.decay, pad.tone, echoOnly = direct)
        synchronized(padHits) { padHits.add(SystemClock.uptimeMillis() to pad.name) }
        padFlash[index] = SystemClock.uptimeMillis()
        padChosen = index
        touched()
    }

    private val padPlayer = PadPlayer()
    private val padLoader = java.util.concurrent.Executors.newSingleThreadExecutor()
    @Volatile private var padLoadedBank = -1

    /** Load the bank that is shown into the pads' own outputs (again when a pad's decay or tone has changed). */
    fun loadBank() {
        val index = padBank
        padLoader.execute {
            val bank = banks.getOrNull(index) ?: return@execute
            if (padLoadedBank != index) {
                padLoadedBank = -1
                padPlayer.clear()
            }
            for ((i, p) in bank.pads.withIndex()) p.sound?.let { padPlayer.load(i, it, p.decay, p.tone) }
            if (padBank == index) padLoadedBank = index
        }
    }

    /** The pad whose knobs are shown: the one hit last. */
    @Volatile var padChosen = 0

    /** When each pad of the bank shown was last hit, for its light. */
    val padFlash = LongArray(8)

    // ------------------------------------------------------------------ the pad's keys

    private var lastAct = ""
    private var lastActAt = 0L

    /** Do what a key of the pad means. The same thing asked twice within a moment (a key that is also an axis) is done once. */
    private fun act(what: String) {
        val now = SystemClock.uptimeMillis()
        if (what == lastAct && now - lastActAt < 40) return
        lastAct = what
        lastActAt = now
        val over = overlay
        if (over != null) {
            when (what) {
                "a" -> over.pick()
                "b", "y" -> over.close()
                "up" -> over.move(-1)
                "down" -> over.move(1)
            }
            return
        }
        if (page == Page.JAM) {
            // the pad is played with the left stick while A is held (see `key` and `stick`); the rest sets it up
            when (what) {
                "l1" -> turnPage(-1)
                "r1" -> turnPage(1)
                "l2" -> Jam.track = Jam.track - 1
                "r2" -> Jam.track = Jam.track + 1
                "left" -> Jam.setProgram(Jam.track, Jam.program[Jam.track] - 1)
                "right" -> Jam.setProgram(Jam.track, Jam.program[Jam.track] + 1)
                "up" -> Jam.rhythm = Jam.rhythm + 1
                "down" -> Jam.rhythm = Jam.rhythm - 1
                "x" -> Jam.record(!Jam.recording)
                "y" -> Jam.play(!Jam.playing)
                "b" -> Jam.clear(Jam.track)
                "start" -> togglePlay()
            }
            return
        }
        if (page == Page.PADS) {
            // the cross and the four keys are the eight pads; the shoulders turn the page, as
            // everywhere, and the triggers change the bank
            val pad = listOf("left", "up", "right", "down", "x", "y", "b", "a").indexOf(what)
            when {
                pad >= 0 -> hitPad(pad)
                what == "l1" -> turnPage(-1)
                what == "r1" -> turnPage(1)
                what == "l2" -> padBank = (padBank - 1).mod(banks.size)
                what == "r2" -> padBank = (padBank + 1).mod(banks.size)
                what == "start" -> togglePlay()
            }
            return
        }
        when (what) {
            "a", "start" -> togglePlay()
            "x" -> nextScene()
            "y" -> openList?.invoke()
            // the shoulders turn the page: that is done more often than going to another song
            // (which is on the screen, and in the list)
            "l1" -> turnPage(-1)
            "r1" -> turnPage(1)
            "l2" -> tempoBy(-2)
            "r2" -> tempoBy(2)
            "up" -> transposeBy(1)
            "down" -> transposeBy(-1)
            "left" -> seekBars(-1)
            "right" -> seekBars(1)
            "select" -> resetKeyAndTempo()
            "lyrics" -> lyricsOn = !lyricsOn
        }
    }

    /** Set by the console: open its list of songs. */
    @Volatile var openList: (() -> Unit)? = null

    fun pages(): List<Page> = if (liveMode) Page.values().toList() else listOf(Page.SCORE, Page.JAM, Page.PADS, Page.HEALING)

    private var jamSet = false

    /** The jam page is on the screen: its sound runs, and the first time it takes the key, scale and tempo of the song that is loaded. */
    fun jamShown() {
        Jam.wake()
        if (jamSet) return
        jamSet = true
        val song = playing.song ?: return
        val names = listOf("C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B")
        val flats = mapOf("Db" to 1, "D#" to 3, "Gb" to 6, "G#" to 8, "A#" to 10)
        (names.indexOf(song.key).takeIf { it >= 0 } ?: flats[song.key])?.let { Jam.key = it }
        Jam.scale = when (song.scale.lowercase()) {
            "major", "ionian" -> 0
            "minor", "aeolian" -> 1
            "dorian" -> 5
            "phrygian" -> 6
            "mixolydian" -> 7
            else -> 3
        }
        if (song.bpm in 40.0..220.0) Jam.bpm = song.bpm.toFloat()
    }

    fun turnPage(by: Int) {
        val all = pages()
        page = all[(all.indexOf(page).coerceAtLeast(0) + by).mod(all.size)]
    }

    fun key(event: KeyEvent): Boolean {
        val what = when (event.keyCode) {
            KeyEvent.KEYCODE_BUTTON_A, KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE -> "a"
            KeyEvent.KEYCODE_BUTTON_B -> "b"
            KeyEvent.KEYCODE_BUTTON_X -> "x"
            KeyEvent.KEYCODE_BUTTON_Y -> "y"
            KeyEvent.KEYCODE_BUTTON_L1, KeyEvent.KEYCODE_MEDIA_PREVIOUS -> "l1"
            KeyEvent.KEYCODE_BUTTON_R1, KeyEvent.KEYCODE_MEDIA_NEXT -> "r1"
            KeyEvent.KEYCODE_BUTTON_L2 -> "l2"
            KeyEvent.KEYCODE_BUTTON_R2 -> "r2"
            KeyEvent.KEYCODE_DPAD_UP -> "up"
            KeyEvent.KEYCODE_DPAD_DOWN -> "down"
            KeyEvent.KEYCODE_DPAD_LEFT -> "left"
            KeyEvent.KEYCODE_DPAD_RIGHT -> "right"
            KeyEvent.KEYCODE_BUTTON_START -> "start"
            KeyEvent.KEYCODE_BUTTON_SELECT -> "select"
            KeyEvent.KEYCODE_BUTTON_THUMBL, KeyEvent.KEYCODE_BUTTON_THUMBR -> "lyrics"
            KeyEvent.KEYCODE_BACK -> if (overlay != null) "b" else return false
            else -> return false
        }
        if (page == Page.JAM && overlay == null && what == "a") {
            // A is the finger on the pad, at the place the left stick points to
            stickDown = event.action == KeyEvent.ACTION_DOWN
            Jam.touch(0.5f + 0.5f * leanX, 0.5f - 0.5f * leanY, stickDown)
            return true
        }
        // held: the bar and the cursor go on moving, nothing else repeats
        val repeats = page != Page.PADS && (what == "left" || what == "right" || ((what == "up" || what == "down") && overlay != null))
        if (event.action == KeyEvent.ACTION_DOWN && (event.repeatCount == 0 || repeats)) act(what)
        return true
    }

    /** The right stick, sideways: how fast the control that has the focus is being turned (−1…1). */
    @Volatile var nudge = 0f
    /** Steps of the right stick up or down that the console has not taken yet: to the control before, or after. */
    @Volatile var focusStep = 0
    private var rightY = 0
    /** On the jam page: A is held, the left stick is the finger. */
    @Volatile var stickDown = false
        private set
    private var hatX = 0
    private var hatY = 0
    private var leftTrigger = false
    private var rightTrigger = false

    /** The left stick leans the picture; the cross and the triggers, where they come as axes, are keys. */
    fun stick(event: MotionEvent): Boolean {
        if (event.source and InputDevice.SOURCE_JOYSTICK != InputDevice.SOURCE_JOYSTICK || event.action != MotionEvent.ACTION_MOVE) return false
        fun axis(a: Int) = event.getAxisValue(a).let { if (abs(it) < 0.12f) 0f else it }
        leanX = axis(MotionEvent.AXIS_X)
        leanY = axis(MotionEvent.AXIS_Y)
        if (stickDown && page == Page.JAM) Jam.touch(0.5f + 0.5f * leanX, 0.5f - 0.5f * leanY, true)
        // the right stick: sideways turns the control that has the focus, up and down goes to the next one
        nudge = event.getAxisValue(MotionEvent.AXIS_Z).let { if (abs(it) < 0.15f) 0f else it }
        val ry = event.getAxisValue(MotionEvent.AXIS_RZ).let { if (it < -0.6f) -1 else if (it > 0.6f) 1 else 0 }
        if (ry != rightY) {
            rightY = ry
            if (ry != 0) focusStep += ry
        }
        val hx = event.getAxisValue(MotionEvent.AXIS_HAT_X).roundToInt()
        val hy = event.getAxisValue(MotionEvent.AXIS_HAT_Y).roundToInt()
        if (hx != hatX) {
            hatX = hx
            if (hx < 0) act("left") else if (hx > 0) act("right")
        }
        if (hy != hatY) {
            hatY = hy
            if (hy < 0) act("up") else if (hy > 0) act("down")
        }
        val lt = maxOf(event.getAxisValue(MotionEvent.AXIS_LTRIGGER), event.getAxisValue(MotionEvent.AXIS_BRAKE)) > 0.5f
        val rt = maxOf(event.getAxisValue(MotionEvent.AXIS_RTRIGGER), event.getAxisValue(MotionEvent.AXIS_GAS)) > 0.5f
        if (lt && !leftTrigger) act("l2")
        if (rt && !rightTrigger) act("r2")
        leftTrigger = lt
        rightTrigger = rt
        return true
    }
}
