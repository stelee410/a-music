package dev.agb.duo

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.opengl.GLES20
import android.opengl.GLSurfaceView
import android.view.View
import javax.microedition.khronos.egl.EGL10
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.egl.EGLDisplay
import javax.microedition.khronos.opengles.GL10
import kotlin.math.sin

/** The picture of the stage: the scene of the song that is playing, sixty times a second. */
class StageView(context: Context) : GLSurfaceView(context), GLSurfaceView.Renderer {
    private class Showing(val song: Song, val revision: Int, val kind: Int, val scene: Scene)

    private var showing: Showing? = null
    private var painter: Painter? = null
    private val frame = Frame()
    private val loud = FloatArray(2)

    init {
        setEGLContextClientVersion(2)
        setEGLConfigChooser(Smooth())
        setRenderer(this)
        renderMode = RENDERMODE_CONTINUOUSLY
    }

    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
        painter = Painter()
        GLES20.glEnable(GLES20.GL_BLEND)
        GLES20.glDepthFunc(GLES20.GL_LEQUAL)
    }

    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) {
        GLES20.glViewport(0, 0, width, height)
        frame.width = width.coerceAtLeast(1)
        frame.height = height.coerceAtLeast(1)
    }

    override fun onDrawFrame(gl: GL10?) {
        val source = Studio.playing
        val song = source.song
        val p = painter
        if (song == null || p == null) {
            GLES20.glClearColor(0.02f, 0.02f, 0.04f, 1f)
            GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT or GLES20.GL_DEPTH_BUFFER_BIT)
            return
        }
        // Another song, another scene, or (in a performance) another track: the scene is made anew.
        var s = showing
        val kind = Studio.sceneKind.mod(SCENES.size)
        if (s == null || s.song !== song || s.revision != song.revision || s.kind != kind) {
            s = Showing(song, song.revision, kind, SCENES[kind].make(song))
            showing = s
        }
        val bg = song.palette[0]
        GLES20.glClearColor((bg shr 16 and 255) / 255f, (bg shr 8 and 255) / 255f, (bg and 255) / 255f, 1f)
        GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT or GLES20.GL_DEPTH_BUFFER_BIT)
        frame.t = source.time()
        source.loudness(loud)
        frame.level = loud[0]
        frame.low = loud[1]
        // The stick is followed smoothly: a lean, not a jump.
        frame.leanX += (Studio.leanX - frame.leanX) * 0.12f
        frame.leanY += (Studio.leanY - frame.leanY) * 0.12f
        s.scene.draw(p, frame)
    }

    /** Eight bits a colour, a depth buffer, and four samples a pixel where the device has them. */
    private class Smooth : EGLConfigChooser {
        override fun chooseConfig(egl: EGL10, display: EGLDisplay): EGLConfig {
            val base = intArrayOf(EGL10.EGL_RED_SIZE, 8, EGL10.EGL_GREEN_SIZE, 8, EGL10.EGL_BLUE_SIZE, 8, EGL10.EGL_DEPTH_SIZE, 16, EGL10.EGL_RENDERABLE_TYPE, 4)
            val smooth = base + intArrayOf(EGL10.EGL_SAMPLE_BUFFERS, 1, EGL10.EGL_SAMPLES, 4, EGL10.EGL_NONE)
            val plain = base + intArrayOf(EGL10.EGL_NONE)
            val out = arrayOfNulls<EGLConfig>(1)
            val n = IntArray(1)
            for (want in arrayOf(smooth, plain)) {
                if (egl.eglChooseConfig(display, want, out, 1, n) && n[0] > 0) return out[0]!!
            }
            throw IllegalStateException("no OpenGL ES 2 configuration")
        }
    }
}

/**
 * Laid over the stage: the line that is being sung, each word lighting up as it is sung. Nothing
 * else is ever written on the stage; while the engine is not ready, three points wait.
 */
class LyricsView(context: Context) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        textAlign = Paint.Align.LEFT
    }

    override fun onDraw(canvas: Canvas) {
        val source = Studio.playing
        val song = source.song
        val waiting = source.waiting
        // Every frame while there is something to draw; a look now and then while there is not.
        if (waiting || (song != null && song.lyrics.isNotEmpty())) postInvalidateOnAnimation() else postInvalidateDelayed(120)
        if (song == null) return
        if (waiting) {
            val now = (System.nanoTime() / 1e9).toFloat()
            val r = height * 0.008f
            paint.color = song.palette[2]
            for (i in 0..2) {
                paint.alpha = (70 + 150 * (0.5f + 0.5f * sin(now * 5f - i * 0.9f))).toInt()
                canvas.drawCircle(width / 2f + (i - 1) * r * 5, height * 0.93f, r, paint)
            }
            return
        }
        if (!Studio.lyricsOn || song.lyrics.isEmpty()) return
        val t = source.time()
        val line = song.lineAt(t, 0.35f, 0.6f) ?: return
        // In before the first word, out after the last.
        val appear = ((t - (line.from - 0.35f)) / 0.3f).coerceIn(0f, 1f) * ((line.to + 0.6f - t) / 0.4f).coerceIn(0f, 1f)
        val latin = line.text.all { it.code < 0x2e80 }
        paint.textSize = height * 0.062f
        val gap = if (latin) paint.measureText(" ") else paint.textSize * 0.12f
        var total = -gap
        for (w in line.words) total += paint.measureText(w.text) + gap
        val fit = if (total > width * 0.9f) width * 0.9f / total else 1f
        paint.textSize *= fit
        var x = (width - total * fit) / 2f
        val y = height * 0.86f
        val sung = song.palette[1]
        val unsung = song.palette[2]
        for (w in line.words) {
            val part = ((t - w.from) / (w.to - w.from).coerceAtLeast(0.05f)).coerceIn(0f, 1f)
            val on = t >= w.from
            paint.setShadowLayer(height * 0.012f, 0f, 0f, 0xcc000000.toInt())
            paint.color = if (on) sung else unsung
            paint.alpha = (appear * (if (on) 255f else 150f)).toInt()
            // The word that is being sung rises a little, and settles.
            val lift = if (on) -height * 0.008f * (1f - part) else 0f
            canvas.drawText(w.text, x, y + lift, paint)
            x += (paint.measureText(w.text) + gap * fit)
        }
    }
}
