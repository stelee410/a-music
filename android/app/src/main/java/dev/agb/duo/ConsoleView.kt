package dev.agb.duo

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Typeface
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sin

/**
 * The lower screen: everything that is read and touched.
 *
 * Its pages: the score (the tracks and the notes that are coming; touch a track to silence it,
 * drag the notes to move through a song, drag a track sideways to set its level in a performance),
 * the talk with the AI, the mixer, the pads and the healing sounds. Over them lie the list of
 * songs, the start of an AI performance, and the settings.
 *
 * It is drawn in units of 1/480 of its height (the 4:3 screen of the device is 640 by 480 of
 * them), anew for every frame, and what can be touched is noted while it is drawn.
 */
class ConsoleView(context: Context) : View(context), Studio.Overlay {
    /** The text fields are real ones, laid over this view by whoever holds it. */
    interface Host {
        fun text(field: String): String
        fun setText(field: String, value: String)
        fun done()
    }

    var host: Host? = null
    /** Where each text field belongs now (in pixels); a field that is not here is hidden. */
    val fields = HashMap<String, RectF>()

    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val bold = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
    private val plain = Typeface.DEFAULT
    private val rect = RectF()
    private val path = Path()
    private var u = 1f
    private var w = 640f
    private var ink = 0
    private var accent = 0
    private var done = BooleanArray(0)
    private var trackColors: List<Rgb> = emptyList()
    private var coloursOf: Song? = null
    private var coloursAt = -1

    private enum class Over { NONE, LIST, LAUNCH, SETTINGS }

    private var over = Over.NONE
    private var cursor = 0
    private var phrases = 8
    private var bars = 8
    private var mock = false

    // ---- what can be touched: noted while drawing
    private class Zone(val rect: RectF, val tap: (() -> Unit)?, val down: (() -> Unit)?, val drag: ((Float, Float, Int) -> Unit)?)
    private class Touch(val zone: Zone, val x: Float, val y: Float, var moved: Boolean)

    private val zones = ArrayList<Zone>()
    private val touches = HashMap<Int, Touch>()
    private var pressed: RectF? = null
    private var scrubFrom = 0f
    private var scrubX = 0f
    /** While the notes or the overview are dragged: the time shown, not yet jumped to. */
    private var scrub = -1f

    init {
        Studio.openList = { post { if (over == Over.LIST) close() else open(Over.LIST) } }
    }

    override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
        super.onLayout(changed, left, top, right, bottom)
        // The faders are dragged from near the left edge: that is not the system's "back" swipe.
        systemGestureExclusionRects = listOf(android.graphics.Rect(0, 0, width / 3, height), android.graphics.Rect(width * 2 / 3, 0, width, height))
    }

    // ------------------------------------------------------------------ what lies over the pages

    private fun open(what: Over) {
        over = what
        cursor = Studio.current.coerceAtLeast(0)
        Studio.overlay = this
        if (what == Over.SETTINGS) {
            host?.setText("key", "")
            host?.setText("base", Studio.prefs.baseUrl)
            host?.setText("models", Studio.prefs.models)
        }
    }

    override fun close() {
        over = Over.NONE
        Studio.overlay = null
        host?.done()
    }

    override fun move(by: Int) {
        val n = Studio.cards.size
        if (over == Over.LIST && n > 0) cursor = (cursor + by).mod(n)
    }

    override fun pick() {
        when (over) {
            Over.LIST -> {
                if (cursor in Studio.cards.indices) Studio.choose(cursor)
                close()
            }
            Over.LAUNCH -> launch()
            else -> {}
        }
    }

    private fun launch() {
        val brief = host?.text("brief") ?: ""
        close()
        Studio.perform(brief, phrases, bars, mock)
    }

    // ------------------------------------------------------------------ small things

    private val headerH = 54f
    private val bodyTop = 54f
    private val bodyBottom = 352f
    private val tracksW = 138f
    private val stripTop = 358f
    private val stripBottom = 396f
    private val keysTop = 404f
    private val keysBottom = 470f

    private fun text(size: Float, color: Int, face: Typeface = plain, align: Paint.Align = Paint.Align.LEFT) {
        paint.style = Paint.Style.FILL
        paint.textSize = size * u
        paint.color = color
        paint.typeface = face
        paint.textAlign = align
    }

    private fun fitted(s: String, width: Float): String {
        if (paint.measureText(s) <= width) return s
        var n = s.length
        while (n > 1 && paint.measureText(s, 0, n) + paint.measureText("…") > width) n--
        return s.substring(0, n) + "…"
    }

    private fun clock(seconds: Float): String {
        val s = max(0f, seconds).toInt()
        return "%d:%02d".format(s / 60, s % 60)
    }

    private fun shade(color: Int, alpha: Int) = Color.argb(alpha, Color.red(color), Color.green(color), Color.blue(color))
    private fun colorOf(c: Rgb, alpha: Int = 255) = Color.argb(alpha, (c.r * 255).toInt(), (c.g * 255).toInt(), (c.b * 255).toInt())

    /** Note that the rectangle (in units) can be touched. */
    private fun zone(l: Float, t: Float, r: Float, b: Float, down: (() -> Unit)? = null, drag: ((Float, Float, Int) -> Unit)? = null, tap: (() -> Unit)? = null) {
        zones.add(Zone(RectF(l * u, t * u, r * u, b * u), tap, down, drag))
    }

    /** A key: a rounded box with a word in it, and under the word the key of the pad that does the same. */
    private fun key(canvas: Canvas, l: Float, t: Float, r: Float, b: Float, label: String, hint: String = "", lit: Boolean = false, size: Float = 13f, tap: () -> Unit) {
        rect.set(l * u, t * u, r * u, b * u)
        val held = pressed?.let { it.left == rect.left && it.top == rect.top } == true
        paint.style = Paint.Style.FILL
        paint.color = when {
            held -> shade(accent, 150)
            lit -> shade(accent, 80)
            else -> shade(ink, 24)
        }
        val round = min(9f, (b - t) / 3) * u
        canvas.drawRoundRect(rect, round, round, paint)
        text(size, ink, bold, Paint.Align.CENTER)
        val cy = if (hint.isEmpty()) (t + b) / 2 + size * 0.36f else t + (b - t) * 0.42f + size * 0.36f
        canvas.drawText(fitted(label, (r - l - 6) * u), (l + r) / 2 * u, cy * u, paint)
        if (hint.isNotEmpty()) {
            text(9.5f, shade(ink, 120), bold, Paint.Align.CENTER)
            canvas.drawText(hint, (l + r) / 2 * u, (b - 8) * u, paint)
        }
        zone(l, t, r, b, tap = tap)
    }

    // ---- the controls that can be turned with the pad: the right stick, sideways, turns the one
    // that has the focus (the further the stick, the faster; a little is a fine adjustment); up and
    // down goes to the one before or after. Touching a control gives it the focus.
    private var focus = ""
    private val order = ArrayList<String>()
    private var lastFrame = 0L
    private var frameSeconds = 0f

    /**
     * A slider: a name, a value (0…1), and a bar that is dragged or turned with the stick.
     * `compact` leaves out the line with the name (it is written beside the bar by the caller).
     */
    private fun slider(canvas: Canvas, l: Float, t: Float, r: Float, label: String, value: Float, shown: String, compact: Boolean = false, set: (Float) -> Unit) {
        val id = "${Studio.page}/$label/${l.toInt()}/${t.toInt()}"
        order.add(id)
        val focused = id == focus
        if (focused && Studio.nudge != 0f) {
            val n = Studio.nudge
            // a third of the way a second at full tilt, a hundredth at a light touch
            set((value + n * abs(n) * 0.33f * frameSeconds).coerceIn(0f, 1f))
        }
        if (!compact) {
            text(12f, if (focused) ink else shade(ink, 200), if (focused) bold else plain)
            canvas.drawText(label, l * u, (t + 4) * u, paint)
            text(12f, ink, bold, Paint.Align.RIGHT)
            canvas.drawText(shown, r * u, (t + 4) * u, paint)
        }
        val y = if (compact) t + 8 else t + 20
        if (focused) {
            // the one the stick turns
            paint.style = Paint.Style.STROKE
            paint.strokeWidth = 1.5f * u
            paint.color = shade(accent, 230)
            rect.set((l - 12) * u, (y - 13) * u, (r + 12) * u, (y + 13) * u)
            canvas.drawRoundRect(rect, 13 * u, 13 * u, paint)
            paint.style = Paint.Style.FILL
        }
        paint.color = shade(ink, 30)
        rect.set(l * u, (y - 4) * u, r * u, (y + 4) * u)
        canvas.drawRoundRect(rect, 4 * u, 4 * u, paint)
        paint.color = shade(accent, 220)
        val x = l + (r - l) * value.coerceIn(0f, 1f)
        rect.set(l * u, (y - 4) * u, x * u, (y + 4) * u)
        canvas.drawRoundRect(rect, 4 * u, 4 * u, paint)
        paint.color = ink
        canvas.drawCircle(x * u, y * u, (if (focused) 9.5f else 8f) * u, paint)
        zone(l - 10, y - 16, r + 10, y + 16, drag = { px, _, _ ->
            focus = id
            set(((px / u - l) / (r - l)).coerceIn(0f, 1f))
        })
    }

    /** The right stick was pushed up or down: the focus goes to the control before or after. */
    private fun moveFocus() {
        val step = Studio.focusStep
        if (step == 0) return
        Studio.focusStep = 0
        if (order.isEmpty()) return
        val at = order.indexOf(focus)
        focus = if (at < 0) order[if (step > 0) 0 else order.size - 1] else order[(at + step).mod(order.size)]
    }

    /** A triangle pointing right (direction 1) or left (−1), `size` high, its flat side at x. */
    private fun triangle(canvas: Canvas, x: Float, cy: Float, size: Float, direction: Float) {
        path.rewind()
        path.moveTo(x, cy - size / 2)
        path.lineTo(x + direction * size * 0.86f, cy)
        path.lineTo(x, cy + size / 2)
        path.close()
        canvas.drawPath(path, paint)
    }

    private fun dots(canvas: Canvas, x: Float, y: Float) {
        val now = SystemClock.uptimeMillis() / 1000f
        paint.color = accent
        for (i in 0..2) {
            paint.alpha = (70 + 150 * (0.5f + 0.5f * sin(now * 5f - i * 0.9f))).toInt()
            canvas.drawCircle((x + i * 9) * u, y * u, 2.6f * u, paint)
        }
        paint.alpha = 255
    }

    // ------------------------------------------------------------------ drawing

    override fun onDraw(canvas: Canvas) {
        postInvalidateOnAnimation()
        u = height / 480f
        w = width / u
        zones.clear()
        fields.clear()
        val now = SystemClock.uptimeMillis()
        frameSeconds = ((now - lastFrame) / 1000f).coerceIn(0f, 0.1f)
        lastFrame = now
        moveFocus() // among the controls of the frame before, which are the ones on the screen
        order.clear()
        val live = Studio.liveMode
        val song = Studio.playing.song
        val bg = song?.palette?.get(0) ?: 0xff0b0d14.toInt()
        accent = song?.palette?.get(1) ?: 0xffe8825a.toInt()
        ink = 0xfff1ede4.toInt()
        // A shade darker than the stage: the console is the instrument, the picture is above.
        canvas.drawColor(Color.rgb(Color.red(bg) * 2 / 3, Color.green(bg) * 2 / 3, Color.blue(bg) * 2 / 3))
        if (song !== coloursOf || song?.revision != coloursAt) {
            coloursOf = song
            coloursAt = song?.revision ?: -1
            trackColors = if (song == null) emptyList() else spread(song.palette, max(1, song.tracks.size))
        }
        if (Studio.page !in Studio.pages()) Studio.page = Page.SCORE
        val t = if (scrub >= 0) scrub else Studio.playing.time()

        header(canvas, song, t)
        when (Studio.page) {
            Page.SCORE -> if (song != null && song.tracks.isNotEmpty()) {
                tracks(canvas, song, t, live)
                roll(canvas, song, t, live)
            } else empty(canvas, live)
            Page.CHAT -> chat(canvas)
            Page.MIXER -> mixer(canvas)
            Page.PADS -> pads(canvas)
            Page.HEALING -> healing(canvas)
        }
        paint.color = shade(ink, 26)
        canvas.drawRect(0f, (bodyBottom - 0.5f) * u, w * u, bodyBottom * u, paint)
        if (live) liveStrip(canvas) else if (song != null) overview(canvas, song, t)
        if (live) liveKeys(canvas) else playerKeys(canvas)
        when (over) {
            Over.LIST -> list(canvas)
            Over.LAUNCH -> launcher(canvas)
            Over.SETTINGS -> settings(canvas)
            Over.NONE -> {}
        }
        val notice = Studio.notice
        if (notice.isNotEmpty()) {
            paint.color = 0xf0101218.toInt()
            rect.set(40 * u, 362 * u, (w - 40) * u, 394 * u)
            canvas.drawRoundRect(rect, 8 * u, 8 * u, paint)
            text(12.5f, ink, plain, Paint.Align.CENTER)
            canvas.drawText(fitted(notice, (w - 100) * u), w / 2 * u, 383 * u, paint)
        }
    }

    private fun empty(canvas: Canvas, live: Boolean) {
        text(15f, shade(ink, 150), plain, Paint.Align.CENTER)
        val message = when {
            live && Studio.live.status == LiveSession.Status.ERROR -> Studio.live.error
            live -> Studio.live.activity.ifEmpty { "乐手正在调音" }
            Studio.player.state == Player.State.ERROR -> Studio.player.error ?: "出错了"
            Studio.cards.isEmpty() -> "没有曲子：把 .agbw 放进应用的 songs 文件夹，或者让 AI 演奏"
            else -> "载入中…"
        }
        canvas.drawText(fitted(message, (w - 60) * u), w / 2 * u, 200 * u, paint)
        if (live && (Studio.live.composing || Studio.live.thinking)) dots(canvas, w / 2 - 9, 222f)
    }

    private fun header(canvas: Canvas, song: Song?, t: Float) {
        val live = Studio.liveMode
        val pages = Studio.pages()
        val tabW = 50f
        val right = w - 8
        val tabsLeft = right - 40 - 6 - 46 - 8 - pages.size * (tabW + 4)
        val card = Studio.cards.getOrNull(Studio.current)
        text(19f, ink, bold)
        val name = if (live) Studio.live.title else song?.title ?: card?.title ?: "Agent Groovebox"
        canvas.drawText(fitted(name, (tabsLeft - 24) * u), 14 * u, 25 * u, paint)
        text(11f, shade(ink, 150))
        val under = if (live) {
            val s = Studio.live
            val state = when (s.status) {
                LiveSession.Status.IDLE -> ""
                LiveSession.Status.PREPARING -> "准备中"
                LiveSession.Status.PLAYING -> if (s.paused) "暂停" else "演奏中"
                LiveSession.Status.FINALE -> "终章"
                LiveSession.Status.FINISHED -> "演奏完成"
                LiveSession.Status.STOPPED -> "已停止"
                LiveSession.Status.ERROR -> "出错"
            }
            listOfNotNull(if (s.ofSong) "与乐手合奏" else "AI 演奏", state, song?.sectionAt(t)?.name).joinToString("  ·  ")
        } else {
            val state = when (Studio.player.state) {
                Player.State.LOADING -> "载入中"
                Player.State.BUFFERING -> "合成中"
                Player.State.PAUSED -> "暂停"
                Player.State.ENDED -> "结束"
                Player.State.ERROR -> "出错"
                else -> null
            }
            listOfNotNull(song?.artist?.takeIf { it.isNotEmpty() }, song?.sectionAt(t)?.name, state).joinToString("  ·  ")
        }
        canvas.drawText(fitted(under, (tabsLeft - 24) * u), 14 * u, 43 * u, paint)

        var x = tabsLeft
        for (p in pages) {
            key(canvas, x, 12f, x + tabW, 42f, p.title, lit = Studio.page == p && over == Over.NONE, size = 11.5f) {
                if (over != Over.NONE) close()
                Studio.page = p
            }
            x += tabW + 4
        }
        x += 4
        key(canvas, x, 12f, x + 46, 42f, "AI", lit = over == Over.LAUNCH, size = 13f) { if (over == Over.LAUNCH) close() else open(Over.LAUNCH) }
        x += 52
        key(canvas, x, 12f, x + 40, 42f, "设置", lit = over == Over.SETTINGS, size = 11f) { if (over == Over.SETTINGS) close() else open(Over.SETTINGS) }
        paint.color = shade(ink, 26)
        canvas.drawRect(0f, (headerH - 0.5f) * u, w * u, headerH * u, paint)
    }

    private fun rowHeight(song: Song) = (bodyBottom - bodyTop) / max(1, song.tracks.size)

    private fun tracks(canvas: Canvas, song: Song, t: Float, live: Boolean) {
        val h = rowHeight(song)
        val muted = Studio.player.muted
        val mixer = Studio.mixer
        val sounding = Studio.playing.sounding
        for ((i, track) in song.tracks.withIndex()) {
            val top = bodyTop + i * h
            val off = if (live) i < LiveMixer.SLOTS && mixer.muted[i] else track.id in muted
            val c = trackColors.getOrNull(i) ?: Rgb(accent)
            if (live && i < LiveMixer.SLOTS) {
                // the fader: how far the row is filled
                paint.color = colorOf(c, if (off) 14 else 44)
                canvas.drawRect(0f, (top + 1) * u, tracksW * mixer.volume[i] * u, (top + h - 1) * u, paint)
            }
            val hit = if (off || !sounding) 0f else if (live && i < LiveMixer.SLOTS) min(1f, mixer.slotLevel[i] * 4) else track.pulse(t, 0.2f)
            paint.style = Paint.Style.FILL
            paint.color = if (off) shade(ink, 40) else colorOf(c, (90 + 165 * min(1f, hit * 1.4f)).toInt())
            canvas.drawCircle(16 * u, (top + h / 2) * u, (4f + 2.5f * min(1f, hit)) * u, paint)
            text(min(14f, h * 0.36f), if (off) shade(ink, 80) else ink, bold)
            canvas.drawText(fitted(track.name, (tracksW - 34) * u), 30 * u, (top + h / 2 - 1) * u, paint)
            text(min(9.5f, h * 0.25f), shade(ink, if (off) 60 else 120))
            val small = if (off) "静音" else if (live && i < LiveMixer.SLOTS) "${(mixer.volume[i] * 100).roundToInt()}%  ${track.engine.substringAfter('.')}" else track.role.ifEmpty { track.engine }
            canvas.drawText(fitted(small, (tracksW - 34) * u), 30 * u, (top + h / 2 + 12) * u, paint)
            paint.color = shade(ink, 18)
            canvas.drawRect(0f, (top + h - 0.5f) * u, width.toFloat(), (top + h) * u, paint)
            if (live && i < LiveMixer.SLOTS) {
                zone(0f, top, tracksW, top + h,
                    drag = { px, _, phase -> if (phase == 1) { mixer.volume[i] = (px / u / tracksW).coerceIn(0f, 1f); Studio.touched() } },
                    tap = { mixer.muted[i] = !mixer.muted[i]; Studio.touched() })
            } else if (!live) zone(0f, top, tracksW, top + h, tap = { Studio.player.toggleMute(track.id) })
        }
        paint.color = shade(ink, 26)
        canvas.drawRect((tracksW - 0.5f) * u, bodyTop * u, tracksW * u, bodyBottom * u, paint)
    }

    /** Seconds of song across the notes, and where in them "now" is. */
    private fun secondsShown(song: Song, t: Float) = song.barLength(t).coerceIn(0.5f, 8f) * 4f / 0.72f

    private fun roll(canvas: Canvas, song: Song, t: Float, live: Boolean) {
        val left = tracksW
        val span = w - left
        val shown = secondsShown(song, t)
        val perSecond = span / shown
        // a performance is known only a bar or two ahead: "now" is further to the right there
        val nowAt = if (live) 0.6f else 0.28f
        val t0 = t - shown * nowAt
        val t1 = t0 + shown
        fun x(time: Float) = (left + (time - t0) * perSecond) * u
        canvas.save()
        canvas.clipRect(left * u, bodyTop * u, w * u, bodyBottom * u)

        // bar lines, with the number of the bar
        val bars = song.bars
        var bar = song.barAt(max(0f, t0))
        while (bar < bars.size && bars[bar] <= t1) {
            val bx = x(bars[bar])
            paint.color = shade(ink, 34)
            canvas.drawRect(bx, bodyTop * u, bx + max(1f, 0.6f * u), bodyBottom * u, paint)
            text(8.5f, shade(ink, 90))
            canvas.drawText("${bar + 1}", bx + 3 * u, (bodyTop + 10) * u, paint)
            bar++
        }

        val h = rowHeight(song)
        val muted = Studio.player.muted
        val mixer = Studio.mixer
        for ((i, track) in song.tracks.withIndex()) {
            val start = track.start
            if (start.isEmpty()) continue
            val length = track.length
            val pitch = track.pitch
            val top = bodyTop + i * h + h * 0.2f
            val lane = h * 0.66f
            val off = if (live) i < LiveMixer.SLOTS && mixer.muted[i] else track.id in muted
            val c = trackColors.getOrNull(i) ?: Rgb(accent)
            val range = max(1, track.highest - track.lowest)
            val thick = (lane / (range + 1)).coerceIn(1.6f, 5f)
            val to = track.upper(t1).coerceAtMost(start.size)
            var from = to
            // back to the first note that could still be in the picture (the longest notes are a few bars)
            while (from > 0 && start[from - 1] > t0 - 12f) from--
            val bright = colorOf(Rgb().set(c).lerp(Rgb(ink), 0.55f))
            for (k in from until to) {
                val s = start[k]
                val e = s + length[k]
                if (e < t0) continue
                val y = top + (1f - (pitch[k] - track.lowest).toFloat() / range).coerceIn(0f, 1f) * (lane - thick)
                paint.color = when {
                    off -> shade(ink, 38)
                    t >= s && t < e -> bright
                    e <= t -> colorOf(c, 80)
                    else -> colorOf(c, 215)
                }
                rect.set(x(s), y * u, max(x(e) - 1.2f * u, x(s) + 2.2f * u), (y + thick) * u)
                canvas.drawRoundRect(rect, thick * u / 2, thick * u / 2, paint)
            }
            // a sung track: its words over its notes
            if (track.sung && !off) words(canvas, song, top, ::x, t0, t1, t)
        }

        // now
        paint.color = accent
        val nx = x(t)
        canvas.drawRect(nx - 0.8f * u, bodyTop * u, nx + 0.8f * u, bodyBottom * u, paint)
        canvas.restore()
        if (!live) zone(left, bodyTop, w, bodyBottom, drag = { px, _, phase ->
            when (phase) {
                0 -> {
                    scrubFrom = Studio.player.time()
                    scrubX = px
                }
                1 -> if (abs(px - scrubX) > 6 * u) scrub = (scrubFrom - (px - scrubX) / u / perSecond).coerceIn(0f, song.seconds)
                else -> {
                    if (scrub >= 0) Studio.seekTo(scrub)
                    scrub = -1f
                }
            }
        })
    }

    private fun words(canvas: Canvas, song: Song, top: Float, x: (Float) -> Float, t0: Float, t1: Float, t: Float) {
        for (line in song.lyrics) {
            if (line.to < t0 || line.from > t1) continue
            // each word over its note, but never on top of the word before it
            var free = -Float.MAX_VALUE
            for (word in line.words) {
                text(10.5f, if (t >= word.from) accent else shade(ink, 200), bold)
                val at = max(x(word.from), free)
                free = at + paint.measureText(word.text) + 3 * u
                if (word.to < t0 || word.from > t1) continue
                canvas.drawText(word.text, at, (top - 2) * u, paint)
            }
        }
    }

    private fun overview(canvas: Canvas, song: Song, t: Float) {
        val left = 50f
        val right = w - 50f
        val top = stripTop + 4
        val bottom = stripBottom - 4
        val total = max(0.001f, song.seconds)
        fun x(time: Float) = (left + (time / total).coerceIn(0f, 1f) * (right - left)) * u
        text(11f, shade(ink, 190), plain, Paint.Align.RIGHT)
        canvas.drawText(clock(t), (left - 8) * u, (top + 19) * u, paint)
        paint.textAlign = Paint.Align.LEFT
        canvas.drawText(clock(total), (right + 8) * u, (top + 19) * u, paint)

        // the sections, with their names
        if (song.sections.isEmpty()) {
            paint.color = shade(ink, 30)
            rect.set(left * u, top * u, right * u, (bottom - 6) * u)
            canvas.drawRoundRect(rect, 3 * u, 3 * u, paint)
        }
        for ((i, s) in song.sections.withIndex()) {
            val here = t >= s.from && t < s.to
            paint.color = if (here) shade(accent, 90) else shade(ink, if (i % 2 == 0) 34 else 22)
            rect.set(x(s.from) + 0.8f * u, top * u, x(s.to) - 0.8f * u, (bottom - 6) * u)
            canvas.drawRoundRect(rect, 3 * u, 3 * u, paint)
            text(9.5f, shade(ink, if (here) 255 else 150))
            val room = rect.width() - 8 * u
            if (room > 14 * u) canvas.drawText(fitted(s.name, room), rect.left + 4 * u, (top + 15.5f) * u, paint)
        }
        // what the engine has rendered
        if (done.size != song.bars.size) done = BooleanArray(song.bars.size)
        val n = Studio.player.barsDone(done)
        paint.color = shade(ink, 150)
        var i = 0
        while (i < n) {
            if (!done[i]) {
                i++
                continue
            }
            var j = i
            while (j + 1 < n && done[j + 1]) j++
            canvas.drawRect(x(song.barStart(i)), (bottom - 4) * u, x(song.barEnd(j)), (bottom - 2) * u, paint)
            i = j + 1
        }
        // now
        paint.color = accent
        val nx = x(t)
        canvas.drawRect(nx - 1 * u, (top - 3) * u, nx + 1 * u, bottom * u, paint)
        canvas.drawCircle(nx, (top - 3) * u, 3.2f * u, paint)
        zone(left - 10, stripTop - 6, right + 10, stripBottom + 4, drag = { px, _, phase ->
            scrub = ((px / u - left) / (right - left)).coerceIn(0f, 1f) * song.seconds
            if (phase == 2) {
                Studio.seekTo(scrub)
                scrub = -1f
            }
        })
    }

    /** Under the pages while a performance is on: which phrase, what the AI is doing, how long it has played. */
    private fun liveStrip(canvas: Canvas) {
        val s = Studio.live
        val y = stripTop + 24
        text(12f, shade(ink, 200), bold)
        // the phrase that is being heard, not the one the AI has got to
        val heard = (Studio.live.song?.let { song -> song.sections.indexOfLast { it.from <= s.time() } } ?: -1) + 1
        val count = if (s.phrases > 0 && heard > 0) (if (s.ofSong) "段落 " else "乐句 ") + "$heard / ${maxOf(s.phrases, heard)}" else ""
        canvas.drawText(count, 14 * u, y * u, paint)
        val from = 14 + paint.measureText(count) / u + 14
        text(12f, if (s.status == LiveSession.Status.ERROR) accent else shade(ink, 170))
        val line = if (s.status == LiveSession.Status.ERROR) s.error else s.activity
        canvas.drawText(fitted(line, (w - from - 110) * u), from * u, y * u, paint)
        if (s.composing || s.thinking) dots(canvas, from + min(paint.measureText(line) / u, w - from - 110) + 10, y - 4)
        text(11f, shade(ink, 150), plain, Paint.Align.RIGHT)
        canvas.drawText("${clock(s.time())}   ${s.bpm.roundToInt()} BPM", (w - 14) * u, y * u, paint)
    }

    private fun between(canvas: Canvas, l: Float, r: Float, label: String, value: String) {
        val cx = (l + r) / 2 * u
        text(9.5f, shade(ink, 120), plain, Paint.Align.CENTER)
        canvas.drawText(label, cx, (keysTop + 16) * u, paint)
        text(17f, ink, bold, Paint.Align.CENTER)
        canvas.drawText(value, cx, (keysTop + 40) * u, paint)
    }

    private fun playerKeys(canvas: Canvas) {
        val t = keysTop
        val b = keysBottom
        var x = (w - 622f) / 2
        val playing = Studio.player.playing
        key(canvas, x, t, x + 50, b, "", "L1") { Studio.step(-1) }
        icon(canvas, x + 25, t + 26, "prev")
        x += 56
        key(canvas, x, t, x + 66, b, "", "A", lit = true) { Studio.togglePlay() }
        icon(canvas, x + 33, t + 26, if (playing) "pause" else "play")
        x += 72
        key(canvas, x, t, x + 50, b, "", "R1") { Studio.step(1) }
        icon(canvas, x + 25, t + 26, "next")
        x += 64
        key(canvas, x, t, x + 40, b, "−", "↓", size = 20f) { Studio.transposeBy(-1) }
        between(canvas, x + 40, x + 98, "调", if (Studio.shownTranspose == 0) "原调" else "%+d".format(Studio.shownTranspose))
        key(canvas, x + 98, t, x + 138, b, "+", "↑", size = 20f) { Studio.transposeBy(1) }
        x += 150
        key(canvas, x, t, x + 40, b, "−", "L2", size = 20f) { Studio.tempoBy(-2) }
        between(canvas, x + 40, x + 98, "速", "${Studio.shownBpm.roundToInt()}")
        key(canvas, x + 98, t, x + 138, b, "+", "R2", size = 20f) { Studio.tempoBy(2) }
        x += 152
        key(canvas, x, t, x + 62, b, Studio.sceneName, "X") { Studio.nextScene() }
        x += 68
        key(canvas, x, t, x + 62, b, "曲目", "Y", lit = over == Over.LIST) { if (over == Over.LIST) close() else open(Over.LIST) }
    }

    private fun icon(canvas: Canvas, cxUnits: Float, cyUnits: Float, what: String) {
        val cx = cxUnits * u
        val cy = cyUnits * u
        paint.style = Paint.Style.FILL
        paint.color = ink
        when (what) {
            "pause" -> {
                canvas.drawRect(cx - 8 * u, cy - 10 * u, cx - 2.5f * u, cy + 10 * u, paint)
                canvas.drawRect(cx + 2.5f * u, cy - 10 * u, cx + 8 * u, cy + 10 * u, paint)
            }
            "play" -> triangle(canvas, cx - 7 * u, cy, 17 * u, 1f)
            "prev" -> {
                canvas.drawRect(cx - 10 * u, cy - 8 * u, cx - 7 * u, cy + 8 * u, paint)
                triangle(canvas, cx + 8 * u, cy, 13 * u, -1f)
            }
            "next" -> {
                canvas.drawRect(cx + 7 * u, cy - 8 * u, cx + 10 * u, cy + 8 * u, paint)
                triangle(canvas, cx - 8 * u, cy, 13 * u, 1f)
            }
        }
    }

    private fun liveKeys(canvas: Canvas) {
        val s = Studio.live
        val t = keysTop
        val b = keysBottom
        var x = 12f
        if (s.active) {
            key(canvas, x, t, x + 60, b, "", "A", lit = true) { Studio.togglePlay() }
            icon(canvas, x + 30, t + 26, if (s.paused) "play" else "pause")
            x += 66
            key(canvas, x, t, x + 54, b, "收尾") { s.finish() }
            x += 60
            key(canvas, x, t, x + 54, b, "停止") { s.stop() }
            x += 64
            if (s.ofSong && s.transposeRange.first < s.transposeRange.last) {
                key(canvas, x, t, x + 38, b, "−", "↓", size = 20f) { Studio.transposeBy(-1) }
                between(canvas, x + 38, x + 92, "调", if (Studio.shownTranspose == 0) "原调" else "%+d".format(Studio.shownTranspose))
                key(canvas, x + 92, t, x + 130, b, "+", "↑", size = 20f) { Studio.transposeBy(1) }
                x += 140
            }
            key(canvas, x, t, x + 38, b, "−", "L2", size = 20f) { Studio.tempoBy(-2) }
            between(canvas, x + 38, x + 92, "速", "${Studio.shownBpm.roundToInt()}")
            key(canvas, x + 92, t, x + 130, b, "+", "R2", size = 20f) { Studio.tempoBy(2) }
        } else {
            if (s.canContinue) {
                key(canvas, x, t, x + 110, b, "再演 4 句") { Studio.again() }
                x += 116
            }
            if (s.canSave) {
                key(canvas, x, t, x + 130, b, "另存为 agbsong") { Studio.save() }
                x += 136
            }
        }
        key(canvas, w - 142, t, w - 80, b, Studio.sceneName, "X") { Studio.nextScene() }
        key(canvas, w - 74, t, w - 12, b, "回曲目") { Studio.leaveLive() }
    }

    // ------------------------------------------------------------------ the pages of a performance

    private val quick = listOf("再安静一点", "更有能量", "来段旋律独奏", "加点雨声的感觉", "慢下来，我想冥想")

    private fun chat(canvas: Canvas) {
        val s = Studio.live
        // the talk, newest at the bottom
        val bottom = 296f
        var y = bottom
        val wide = (w - 36) * u
        val feed = s.feed
        var i = feed.size - 1
        if (feed.isEmpty()) {
            text(13f, shade(ink, 130), plain, Paint.Align.CENTER)
            canvas.drawText("演奏开始后，你可以随时对乐手说话。", w / 2 * u, 170 * u, paint)
        }
        if (s.thinking) {
            text(11.5f, shade(ink, 150))
            canvas.drawText("乐手在想", 18 * u, y * u, paint)
            dots(canvas, 18 + paint.measureText("乐手在想") / u + 8, y - 4)
            y -= 20
        }
        while (i >= 0 && y > bodyTop + 14) {
            val e = feed[i--]
            val (size, color, face, lead) = when (e.kind) {
                0 -> Quad(13f, ink, bold, "你  ")
                1 -> Quad(13f, accent, plain, "乐手  ")
                2 -> Quad(12.5f, shade(ink, 210), bold, "♪ ")
                else -> Quad(11.5f, shade(ink, 130), plain, "· ")
            }
            val lines = ArrayList<String>()
            if (e.detail.isNotEmpty()) {
                text(11f, shade(ink, 130))
                wrap(e.detail, wide - 16 * u, 2, lines)
                for (k in lines.indices.reversed()) {
                    canvas.drawText(lines[k], 30 * u, y * u, paint)
                    y -= 15
                }
                lines.clear()
            }
            text(size, color, face)
            wrap(lead + e.text, wide, 4, lines)
            for (k in lines.indices.reversed()) {
                if (y < bodyTop + 12) break
                canvas.drawText(lines[k], 18 * u, y * u, paint)
                y -= size + 5
            }
            y -= 5
        }
        // things to say with one touch
        var x = 14f
        for (q in quick) {
            text(11f, ink)
            val width = paint.measureText(q) / u + 18
            if (x + width > w - 10) break
            key(canvas, x, 304f, x + width, 324f, q, size = 11f) { Studio.chat(q) }
            x += width + 6
        }
        // the field, and "send"
        fields["chat"] = RectF(14 * u, 329 * u, (w - 84) * u, 350 * u)
        key(canvas, w - 76, 328f, w - 14, 350f, "发送", lit = true, size = 12f) {
            val words = host?.text("chat") ?: ""
            if (words.isNotBlank()) {
                Studio.chat(words)
                host?.setText("chat", "")
            }
        }
    }

    private data class Quad(val size: Float, val color: Int, val face: Typeface, val lead: String)

    /** Break `s` into at most `most` lines no wider than `width` pixels (the paint is set). */
    private fun wrap(s: String, width: Float, most: Int, into: ArrayList<String>) {
        var rest = s.replace('\n', ' ')
        while (rest.isNotEmpty() && into.size < most) {
            val n = paint.breakText(rest, true, width, null).coerceAtLeast(1)
            if (into.size == most - 1 && n < rest.length) {
                into.add(fitted(rest, width))
                return
            }
            into.add(rest.substring(0, n))
            rest = rest.substring(n)
        }
    }

    private fun mixer(canvas: Canvas) {
        val m = Studio.mixer
        val l = 26f
        val r = w * 0.42f
        slider(canvas, l, 74f, r, "滤波", m.filter, if (m.filter >= 0.995f) "全开" else "${(200 * Math.pow(100.0, m.filter.toDouble())).toInt()} Hz") { m.filter = it; Studio.touched() }
        slider(canvas, l, 126f, r, "混响", m.reverb, "${(m.reverb * 100).roundToInt()}%") { m.reverb = it; Studio.touched() }
        slider(canvas, l, 178f, r, "延迟", m.delay, "${(m.delay * 100).roundToInt()}%") { m.delay = it; Studio.touched() }
        slider(canvas, l, 230f, r, "总音量", m.master, "${(m.master * 100).roundToInt()}%") { m.master = it }
        key(canvas, l, 286f, l + 120, 322f, if (m.isRecording) "● 停止录音" else "录音", lit = m.isRecording) { Studio.toggleRecording() }
        text(10f, shade(ink, 110))
        canvas.drawText("右摇杆：左右微调，上下换控件", (l + 132) * u, 308 * u, paint)

        // the band: a fader and a mute for every track
        val tracks = Studio.live.song?.tracks ?: emptyList()
        val left = w * 0.42f + 44
        val count = min(tracks.size, LiveMixer.SLOTS)
        if (count == 0) return
        val row = min(36f, (bodyBottom - bodyTop - 12) / count)
        for (i in 0 until count) {
            val y = bodyTop + 10 + i * row
            val off = m.muted[i]
            text(min(12f, row * 0.36f), if (off) shade(ink, 90) else ink, bold)
            canvas.drawText(fitted(tracks[i].name, 70 * u), left * u, (y + row / 2 + 4) * u, paint)
            slider(canvas, left + 78, y + row / 2 - 8, w - 70, tracks[i].name, m.volume[i], "", compact = true) { m.volume[i] = it; Studio.touched() }
            key(canvas, w - 50, y + 3, w - 14, y + row - 3, if (off) "静" else "M", lit = off, size = 11f) { m.muted[i] = !m.muted[i]; Studio.touched() }
        }
    }

    private val padKeys = listOf("←", "↑", "→", "↓", "X", "Y", "B", "A")

    private fun pads(canvas: Canvas) {
        val banks = Studio.banks
        var x = 14f
        for ((i, bank) in banks.withIndex()) {
            val width = (w - 28 - 4 * 6) / 5
            key(canvas, x, 60f, x + width, 84f, bank.title, lit = i == Studio.padBank, size = 12f) { Studio.padBank = i }
            x += width + 6
        }
        val bank = banks[Studio.padBank.coerceIn(0, banks.size - 1)]
        val cellW = (w - 28 - 3 * 8) / 4
        val cellH = 82f
        val now = SystemClock.uptimeMillis()
        for ((i, pad) in bank.pads.withIndex()) {
            val l = 14 + (i % 4) * (cellW + 8)
            val t = 92 + (i / 4) * (cellH + 6)
            val glow = (1f - (now - Studio.padFlash[i]) / 220f).coerceIn(0f, 1f)
            rect.set(l * u, t * u, (l + cellW) * u, (t + cellH) * u)
            paint.color = if (pad.sound == null) shade(ink, 12) else shade(accent, (34 + 190 * glow).toInt())
            canvas.drawRoundRect(rect, 10 * u, 10 * u, paint)
            if (i == Studio.padChosen) {
                // the pad whose knobs are below
                paint.style = Paint.Style.STROKE
                paint.strokeWidth = 1.5f * u
                paint.color = shade(ink, 150)
                canvas.drawRoundRect(rect, 10 * u, 10 * u, paint)
                paint.style = Paint.Style.FILL
            }
            text(15f, if (pad.sound == null) shade(ink, 90) else ink, bold, Paint.Align.CENTER)
            canvas.drawText(fitted(pad.name, (cellW - 10) * u), (l + cellW / 2) * u, (t + cellH / 2 + 3) * u, paint)
            text(10f, shade(ink, 120), bold, Paint.Align.CENTER)
            canvas.drawText(padKeys[i] + if (pad.own) "  · 自己的采样" else "", (l + cellW / 2) * u, (t + cellH - 9) * u, paint)
            // a pad sounds when it is touched, not when it is let go
            zone(l, t, l + cellW, t + cellH, down = { Studio.hitPad(i) })
        }
        // the knobs of the pad that was hit last, and the room of the bank
        val pad = bank.pads[Studio.padChosen.coerceIn(0, bank.pads.size - 1)]
        val gap = 22f
        val each = (w - 28 - 4 * gap) / 5
        val t = 284f
        var l = 14f
        slider(canvas, l, t, l + each, "音高", (pad.pitch + 12f) / 24f, "%+d".format(pad.pitch.roundToInt())) { pad.pitch = (it * 24f - 12f).roundToInt().toFloat(); Studio.padsChanged() }
        l += each + gap
        slider(canvas, l, t, l + each, "衰减", pad.decay, if (pad.decay >= 0.995f) "自然" else "${(pad.decay * 100).roundToInt()}%") { pad.decay = it; Studio.padsChanged() }
        l += each + gap
        slider(canvas, l, t, l + each, "音色", pad.tone, if (pad.tone >= 0.995f) "原样" else "${(pad.tone * 100).roundToInt()}%") { pad.tone = it; Studio.padsChanged() }
        l += each + gap
        slider(canvas, l, t, l + each, "音量", pad.volume / 1.2f, "${(pad.volume * 100).roundToInt()}%") { pad.volume = it * 1.2f; Studio.padsChanged() }
        l += each + gap
        slider(canvas, l, t, l + each, "空间", bank.space, "${(bank.space * 100).roundToInt()}%") { bank.space = it; Studio.padsChanged() }
        text(10f, shade(ink, 110))
        canvas.drawText("「${pad.name}」的旋钮（空间是整组的）。右摇杆：左右微调，上下换旋钮。", 14 * u, (bodyBottom - 8) * u, paint)
        val m = Studio.mixer
        text(9.5f, shade(ink, 80), plain, Paint.Align.RIGHT)
        if (m.gaps > 0) canvas.drawText("混音输出断流 ${m.gaps} 次", (w - 14) * u, (bodyBottom - 8) * u, paint)
    }

    private val solfeggio = floatArrayOf(174f, 285f, 396f, 417f, 432f, 528f, 639f, 741f, 852f)
    private val waves = listOf("δ" to 2f, "θ" to 6f, "α" to 10f, "β" to 20f, "γ" to 40f)

    /** A frequency on a slider: equal steps of the slider are equal ratios of the frequency. */
    private fun toHz(x: Float, low: Float, high: Float) = (low * Math.pow((high / low).toDouble(), x.toDouble())).toFloat()
    private fun fromHz(hz: Float, low: Float, high: Float) = (Math.log((hz / low).toDouble()) / Math.log((high / low).toDouble())).toFloat().coerceIn(0f, 1f)

    private fun healing(canvas: Canvas) {
        val m = Studio.mixer
        val mid = w * 0.62f
        var y = 60f
        // a tone, at any frequency from 40 to 1000 Hz
        key(canvas, 14f, y, 96f, y + 32, "频率", lit = m.toneOn) { m.toneOn = !m.toneOn; Studio.touched() }
        var x = 104f
        for (hz in solfeggio) {
            key(canvas, x, y, x + 44, y + 32, "${hz.toInt()}", lit = m.toneOn && m.toneHz == hz, size = 11.5f) { m.toneHz = hz; m.toneOn = true; Studio.touched() }
            x += 48
        }
        slider(canvas, 104f, y + 46, mid, "赫兹", fromHz(m.toneHz, 40f, 1000f), "%.1f Hz".format(m.toneHz)) {
            // whole Hz from the slider; the stick goes by tenths
            val hz = toHz(it, 40f, 1000f)
            m.toneHz = if (Studio.nudge != 0f) (hz * 10).roundToInt() / 10f else hz.roundToInt().toFloat()
            Studio.touched()
        }
        slider(canvas, mid + 30, y + 46, w - 30, "音量", m.toneLevel / 0.6f, "${(m.toneLevel / 0.6f * 100).roundToInt()}%") { m.toneLevel = it * 0.6f }
        y += 92
        // two tones, one in each ear, a few Hz apart: the difference is what is felt
        key(canvas, 14f, y, 96f, y + 32, "双耳节拍", lit = m.binauralOn, size = 12f) { m.binauralOn = !m.binauralOn; Studio.touched() }
        x = 104f
        for ((name, hz) in waves) {
            key(canvas, x, y, x + 60, y + 32, "$name ${hz.toInt()}", lit = m.binauralOn && m.beatHz == hz, size = 12f) { m.beatHz = hz; m.binauralOn = true; Studio.touched() }
            x += 64
        }
        text(11f, shade(ink, 150))
        canvas.drawText("${Studio.bandName(m.beatHz)}：左耳 %.1f Hz，右耳 %.1f Hz".format(m.carrierHz - m.beatHz / 2, m.carrierHz + m.beatHz / 2), (x + 10) * u, (y + 21) * u, paint)
        val third = (w - 104 - 30 - 2 * 30) / 3
        slider(canvas, 104f, y + 46, 104 + third, "节拍", fromHz(m.beatHz, 0.5f, 40f), "%.1f Hz".format(m.beatHz)) { m.beatHz = (toHz(it, 0.5f, 40f) * 10).roundToInt() / 10f; Studio.touched() }
        slider(canvas, 134 + third, y + 46, 134 + 2 * third, "载波", (m.carrierHz - 100f) / 400f, "${m.carrierHz.toInt()} Hz") { m.carrierHz = 100f + (it * 400f).roundToInt(); Studio.touched() }
        slider(canvas, 164 + 2 * third, y + 46, w - 30, "音量", m.binauralLevel / 0.6f, "${(m.binauralLevel / 0.6f * 100).roundToInt()}%") { m.binauralLevel = it * 0.6f }
        y += 92
        // noise
        key(canvas, 14f, y, 96f, y + 32, "噪声", lit = m.noiseOn) { m.noiseOn = !m.noiseOn; Studio.touched() }
        x = 104f
        for ((i, name) in listOf("白", "粉", "褐").withIndex()) {
            key(canvas, x, y, x + 60, y + 32, name, lit = m.noiseOn && m.noiseColor == i) { m.noiseColor = i; m.noiseOn = true; Studio.touched() }
            x += 64
        }
        slider(canvas, x + 20, y + 4, w - 30, "音量", m.noiseLevel / 0.6f, "${(m.noiseLevel / 0.6f * 100).roundToInt()}%") { m.noiseLevel = it * 0.6f }
        y += 44
        // the reference pitch of the music itself
        val a4 = Studio.a4
        key(canvas, 14f, y, 96f, y + 32, "调音 A4", lit = a4 != 440f, size = 12f) { Studio.tune(440f) }
        key(canvas, 104f, y, 164f, y + 32, "A=432", lit = a4 == 432f, size = 12f) { Studio.tune(432f) }
        key(canvas, 168f, y, 228f, y + 32, "A=440", lit = a4 == 440f, size = 12f) { Studio.tune(440f) }
        slider(canvas, 258f, y + 4, w - 30, "整个乐队的音高（%+.0f 音分）".format(1200 * Math.log(a4 / 440.0) / Math.log(2.0)), (a4 - 426f) / 28f, "%.1f Hz".format(a4)) {
            val hz = 426f + it * 28f
            Studio.tune(if (Studio.nudge != 0f) hz else hz.roundToInt().toFloat())
        }
        text(10f, shade(ink, 110))
        canvas.drawText("右摇杆：左右微调，上下换控件。双耳节拍要戴耳机听。疗愈声音只作放松辅助，不是医疗用途。", 14 * u, (bodyBottom - 6) * u, paint)
    }

    // ------------------------------------------------------------------ what lies over the pages

    private fun veil(canvas: Canvas) {
        paint.style = Paint.Style.FILL
        paint.color = 0xf205060a.toInt()
        canvas.drawRect(0f, headerH * u, w * u, 398 * u, paint)
        // nothing under it is touched; touching beside what is on it closes it
        zone(0f, headerH, w, 398f, tap = { close() })
    }

    private val listTop = 60f
    private val listRow = 46f

    private fun list(canvas: Canvas) {
        veil(canvas)
        val cards = Studio.cards
        // the playlist's three switches, under the songs
        var kx = 14f
        for ((label, on, flip) in listOf(
            Triple("自动下一首", Studio.auto) { Studio.auto = !Studio.auto },
            Triple("小节线切歌", Studio.aligned) { Studio.aligned = !Studio.aligned },
            Triple("循环", Studio.loop) { Studio.loop = !Studio.loop },
        )) {
            val width = if (label.length > 3) 110f else 70f
            key(canvas, kx, 364f, kx + width, 392f, (if (on) "✓ " else "") + label, lit = on, size = 11.5f, tap = flip)
            kx += width + 8
        }
        val fit = ((360f - listTop) / listRow).toInt()
        val first = (cursor - fit + 1).coerceIn(0, max(0, cards.size - fit))
        for (row in 0 until min(fit, cards.size - first)) {
            val i = first + row
            val card = cards[i]
            val top = listTop + row * listRow
            rect.set(12 * u, top * u, (w - 12) * u, (top + listRow - 5) * u)
            if (i == cursor) {
                paint.color = shade(accent, 60)
                canvas.drawRoundRect(rect, 8 * u, 8 * u, paint)
            }
            paint.color = card.accent
            canvas.drawCircle(30 * u, (top + 20) * u, (if (i == Studio.current) 6f else 4f) * u, paint)
            if (i == Studio.cueIndex) {
                text(10f, accent, bold, Paint.Align.RIGHT)
                canvas.drawText("在小节线切入…", (w - 26) * u, (top + 10) * u, paint)
            }
            text(16f, ink, if (i == Studio.current) bold else plain)
            canvas.drawText(fitted(card.title, (w - 250) * u), 48 * u, (top + 18) * u, paint)
            text(10.5f, shade(ink, 140))
            canvas.drawText(fitted(card.artist, (w - 250) * u), 48 * u, (top + 33) * u, paint)
            text(11.5f, shade(ink, 170), plain, Paint.Align.RIGHT)
            canvas.drawText("${card.key}   ${card.bpm.roundToInt()} BPM   ${clock(card.seconds)}", (w - 26) * u, (top + 25) * u, paint)
            zone(12f, top, w - 12, top + listRow - 5, tap = {
                cursor = i
                pick()
            })
        }
    }

    private val suggestions = listOf(
        "雨夜窗边的 lo-fi 嘻哈，慵懒的鼓和电钢琴",
        "深夜城市的数学摇滚电子乐，7 拍琶音缠绕厚重贝斯",
        "柏林地下室的极简 techno，酸性贝斯慢慢打开",
        "海边日出的冥想音乐，缓慢呼吸的长音",
    )

    /** The start of a performance: what to play, how long, and the song that is loaded played with the AI. */
    private fun launcher(canvas: Canvas) {
        veil(canvas)
        text(15f, ink, bold)
        canvas.drawText("让 AI 演奏", 18 * u, 80 * u, paint)
        text(11f, shade(ink, 140))
        canvas.drawText("描述一种情绪、一个场景，或一种风格…", 110 * u, 80 * u, paint)
        fields["brief"] = RectF(18 * u, 90 * u, (w - 18) * u, 126 * u)
        zone(18f, 90f, w - 18, 126f) // the field takes its own touches
        var y = 134f
        for (s in suggestions) {
            key(canvas, 18f, y, w - 18, y + 24, s, size = 11.5f) { host?.setText("brief", s) }
            y += 28
        }
        y += 6
        text(12f, shade(ink, 200))
        canvas.drawText("乐句", 18 * u, (y + 20) * u, paint)
        key(canvas, 54f, y, 86f, y + 30, "−", size = 18f) { phrases = (phrases - 1).coerceAtLeast(2) }
        text(15f, ink, bold, Paint.Align.CENTER)
        canvas.drawText("$phrases", 104 * u, (y + 21) * u, paint)
        key(canvas, 122f, y, 154f, y + 30, "+", size = 18f) { phrases = (phrases + 1).coerceAtMost(32) }
        text(12f, shade(ink, 200))
        canvas.drawText("每句约", 176 * u, (y + 20) * u, paint)
        key(canvas, 222f, y, 254f, y + 30, "−", size = 18f) { bars = (bars - 1).coerceAtLeast(2) }
        text(15f, ink, bold, Paint.Align.CENTER)
        canvas.drawText("$bars", 272 * u, (y + 21) * u, paint)
        key(canvas, 290f, y, 322f, y + 30, "+", size = 18f) { bars = (bars + 1).coerceAtMost(16) }
        text(12f, shade(ink, 200))
        canvas.drawText("小节", 330 * u, (y + 20) * u, paint)
        key(canvas, 380f, y, 470f, y + 30, if (mock) "✓ 离线演示" else "离线演示", lit = mock, size = 12f) { mock = !mock }
        key(canvas, w - 150, y, w - 18, y + 30, "开始演奏", "", lit = true, size = 14f) { launch() }
        y += 46
        paint.color = shade(ink, 26)
        canvas.drawRect(18 * u, (y - 8) * u, (w - 18) * u, (y - 7.4f) * u, paint)
        val song = Studio.player.song
        text(12f, shade(ink, 170))
        canvas.drawText(fitted(if (song != null) "或者和乐手一起演「${song.title}」：曲子照谱演奏，你说话，乐手改谱。" else "选一首曲子后，也可以和乐手一起演它。", (w - 190) * u), 18 * u, (y + 20) * u, paint)
        if (song != null) key(canvas, w - 150, y, w - 18, y + 30, "合奏这首", lit = true, size = 13f) {
            close()
            Studio.jam(mock)
        }
        val keyState = if (Studio.prefs.apiKey.isEmpty()) "还没有 API Key：在“设置”里填写，否则只能离线演示" else ""
        if (keyState.isNotEmpty()) {
            text(11f, accent)
            canvas.drawText(keyState, 18 * u, (y + 50) * u, paint)
        }
    }

    private fun settings(canvas: Canvas) {
        veil(canvas)
        val p = Studio.prefs
        text(15f, ink, bold)
        canvas.drawText("设置", 18 * u, 80 * u, paint)
        var y = 96f
        fun row(label: String, field: String, note: String) {
            text(12f, shade(ink, 200))
            canvas.drawText(label, 18 * u, (y + 20) * u, paint)
            fields[field] = RectF(110 * u, y * u, (w - 18) * u, (y + 30) * u)
            zone(110f, y, w - 18, y + 30)
            text(10f, shade(ink, 120))
            canvas.drawText(fitted(note, (w - 128) * u), 110 * u, (y + 44) * u, paint)
            y += 58
        }
        val key = p.apiKey
        row("API Key", "key", if (key.isEmpty()) "还没有。也可以把密钥存成 ${Prefs.KEY_FILE} 放进应用文件夹，再点“导入密钥文件”" else "已保存（…${key.takeLast(4)}）。留空则不改")
        row("网关地址", "base", "默认 ${Prefs.DEFAULT_BASE}")
        row("模型", "models", "逗号分隔，按顺序尝试；留空用引擎的默认顺序")
        key(canvas, 110f, y, 230f, y + 34, "导入密钥文件", size = 12f) {
            Studio.say(if (p.importKeyFile()) "已导入密钥" else "没找到 Android/data/dev.agb.duo/files/${Prefs.KEY_FILE}", 6)
        }
        key(canvas, w - 150, y, w - 18, y + 34, "保存", lit = true, size = 13f) {
            host?.let { h ->
                h.text("key").trim().takeIf { it.isNotEmpty() }?.let { p.apiKey = it }
                p.baseUrl = h.text("base")
                p.models = h.text("models")
            }
            close()
            Studio.say("设置已保存")
        }
    }

    // ------------------------------------------------------------------ touch

    override fun onTouchEvent(e: MotionEvent): Boolean {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                // touches are handed over as they happen, not kept for the next frame: a pad is hit now
                requestUnbufferedDispatch(e)
                val i = e.actionIndex
                val x = e.getX(i)
                val y = e.getY(i)
                // what was drawn last lies on top
                val z = zones.lastOrNull { it.rect.contains(x, y) } ?: return true
                touches[e.getPointerId(i)] = Touch(z, x, y, false)
                if (z.tap != null) pressed = z.rect
                z.down?.invoke()
                z.drag?.invoke(x, y, 0)
            }
            MotionEvent.ACTION_MOVE -> for (i in 0 until e.pointerCount) {
                val t = touches[e.getPointerId(i)] ?: continue
                val x = e.getX(i)
                val y = e.getY(i)
                if (abs(x - t.x) > 7 * u || abs(y - t.y) > 7 * u) {
                    t.moved = true
                    if (t.zone.drag != null || !t.zone.rect.contains(x, y)) pressed = null
                }
                if (t.moved || t.zone.tap == null) t.zone.drag?.invoke(x, y, 1)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> {
                val i = e.actionIndex
                val t = touches.remove(e.getPointerId(i)) ?: return true
                t.zone.drag?.invoke(e.getX(i), e.getY(i), 2)
                if (!t.moved) t.zone.tap?.invoke()
                pressed = null
            }
            MotionEvent.ACTION_CANCEL -> {
                touches.clear()
                pressed = null
                scrub = -1f
            }
        }
        return true
    }
}
