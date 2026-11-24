package dev.agb.duo

import android.opengl.GLES20
import android.opengl.Matrix
import kotlin.math.PI
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin
import kotlin.math.tan

// The scenes of the stage: the same two the web player has (web/scenes/), drawn natively. A scene
// draws the song from its timeline and the time alone, so a frame can be drawn again as it was.
// No text is ever drawn here: the words that are sung are laid over the picture by LyricsView.

/** What a scene is told for each frame. */
class Frame {
    /** Song time, in seconds. */
    var t = 0f
    /** Loudness of everything, and of the lows, where the song is being heard. */
    var level = 0f
    var low = 0f
    var width = 1
    var height = 1
    /** The left stick, −1…1: the viewer leaning to look. */
    var leanX = 0f
    var leanY = 0f
}

abstract class Scene(val song: Song) {
    abstract fun draw(p: Painter, f: Frame)
}

class SceneKind(val id: String, val name: String, val make: (Song) -> Scene)

val SCENES = listOf(
    SceneKind("orbits", "轨道") { OrbitsScene(it) },
    SceneKind("spectrum", "天际线") { SkylineScene(it) },
)

fun sceneIndex(id: String) = SCENES.indexOfFirst { it.id == id }.coerceAtLeast(0)

private const val TAU = (PI * 2).toFloat()

/**
 * orbits — every track is a ring. A point runs round each ring, the rings at different speeds
 * (one, two, three bars a turn), and every note is left behind as a light where the point was when
 * it sounded. The middle breathes with the low end.
 */
class OrbitsScene(song: Song) : Scene(song) {
    private class Ring(val track: Track, val radius: Float, val bars: Int, val color: Rgb)

    private val fov = 38f
    private val tracks = song.tracks.filter { it.count > 0 }
    private val colors = spread(song.palette, max(1, tracks.size))
    private val inner = 0.75f
    private val step = min(0.5f, 3.1f / max(1, tracks.size))
    private val outer = inner + step * max(0, tracks.size - 1)
    // A turn takes one, two or three bars: rings side by side drift against each other.
    private val rings = tracks.mapIndexed { i, t -> Ring(t, inner + i * step, 1 + i % 3, colors[i]) }
    private val white = Rgb(song.palette[2])
    private val tmp = Rgb()

    override fun draw(p: Painter, f: Frame) {
        val aspect = f.width.toFloat() / f.height
        val half = tan(fov * PI / 360).toFloat()
        Matrix.perspectiveM(p.proj, 0, fov, aspect, 0.1f, 100f)
        p.pointScale = f.height / (2 * half)
        // Far enough back that the outermost ring fits a wide and a tall picture alike.
        val distance = (outer + 0.7f) / half * max(1f, 1f / min(1f, aspect))
        p.noFog()
        GLES20.glDisable(GLES20.GL_DEPTH_TEST)

        val t = f.t
        val barLength = song.barLength(t)
        val fade = barLength * 2.2f

        // The middle: it does not turn with the rings.
        Matrix.setIdentityM(p.view, 0)
        Matrix.translateM(p.view, 0, 0f, 0f, -distance)
        val breath = min(1f, f.low * 5)
        val size = inner * (0.75f + 0.5f * breath) + min(1f, f.level * 3) * 0.2f
        p.drawGlow(0f, 0f, 0f, size * 2, size * 2, song.palette[1], 0.1f + 0.6f * breath + 0.25f * min(1f, f.level * 3), false)

        Matrix.rotateM(p.view, 0, Math.toDegrees((sin(t * 0.11f) * 0.22f + f.leanY * 0.6f).toDouble()).toFloat(), 1f, 0f, 0f)
        Matrix.rotateM(p.view, 0, Math.toDegrees((cos(t * 0.07f) * 0.22f + f.leanX * 0.6f).toDouble()).toFloat(), 0f, 1f, 0f)
        Matrix.rotateM(p.view, 0, Math.toDegrees((-t * 0.02f).toDouble()).toFloat(), 0f, 0f, 1f)

        p.flat.begin()
        p.points.begin()
        for (ring in rings) {
            val track = ring.track
            val hit = track.pulse(t, 0.18f)
            val a = 0.16f + 0.55f * hit
            var px = ring.radius
            var py = 0f
            for (k in 1..SEGMENTS) {
                val ang = k * TAU / SEGMENTS
                val x = cos(ang) * ring.radius
                val y = sin(ang) * ring.radius
                p.flat.put(px, py, 0f, ring.color.r, ring.color.g, ring.color.b, a)
                p.flat.put(x, y, 0f, ring.color.r, ring.color.g, ring.color.b, a)
                px = x
                py = y
            }
            val turn = barLength * ring.bars
            val span = max(1, track.highest - track.lowest).toFloat()
            val to = track.upper(t)
            var from = to
            while (from > 0 && track.start[from - 1] > t - fade * 3) from--
            for (i in from until to) {
                val age = t - track.start[i]
                val ang = (PI / 2).toFloat() - track.start[i] / turn * TAU
                val r = ring.radius + ((track.pitch[i] - track.lowest) / span - 0.5f) * step * 0.5f
                val v = track.velocity[i]
                val fresh = exp(-age / 0.12f)
                val glow = v * exp(-age / fade)
                tmp.set(ring.color).lerp(white, fresh * 0.8f)
                p.points.put(cos(ang) * r, sin(ang) * r, 0f, tmp.r, tmp.g, tmp.b, 0.11f + 0.3f * fresh * v + 0.08f * v, min(1f, 0.12f + glow + fresh * 0.6f))
            }
            // The point that runs round the ring.
            val ang = (PI / 2).toFloat() - t / turn * TAU
            p.points.put(cos(ang) * ring.radius, sin(ang) * ring.radius, 0f, white.r, white.g, white.b, 0.09f + 0.14f * hit, 0.6f + 0.4f * hit)
        }
        GLES20.glLineWidth(2f)
        p.drawFlat(GLES20.GL_LINES, true)
        p.drawPoints()
    }

    private companion object {
        const val SEGMENTS = 160
    }
}

/**
 * spectrum — a skyline of the notes: one column per pitch, as high as that pitch was just played,
 * in the colour of the track that played it, mirrored in the floor. The floor's lines come towards
 * the viewer in time with the beat, and the horizon glows with the drums.
 */
class SkylineScene(song: Song) : Scene(song) {
    private val width = 15f
    private val tonal = song.tracks.filter { it.count > 0 && !it.drums }
    private val drums = song.tracks.filter { it.count > 0 && it.drums }
    private val colors = spread(song.palette, max(1, tonal.size))
    private val low: Int
    private val columns: Int
    private val height: FloatArray
    private val tint: Array<Rgb>
    private val dark = Rgb(song.palette[0])
    private val idle = Rgb(song.palette[3])
    private val line = Rgb(song.palette[3])
    private val c = Rgb()
    private val eye = FloatArray(3)

    init {
        // The pitches the song uses, and never fewer than three octaves: the skyline fills the width.
        var lo = min(tonal.minOfOrNull { it.lowest } ?: 48, 48)
        var hi = max(tonal.maxOfOrNull { it.highest } ?: 72, 72)
        val short = 36 - (hi - lo)
        if (short > 0) {
            lo -= short / 2
            hi += ceil(short / 2f).toInt()
        }
        low = lo
        columns = hi - lo + 1
        height = FloatArray(columns)
        tint = Array(columns) { Rgb() }
    }

    private fun box(p: Painter, x: Float, w: Float, h: Float, c: Rgb, alpha: Float, top: Boolean) {
        val l = x - w / 2
        val r = x + w / 2
        val zf = -4f + w / 2
        val zb = -4f - w / 2
        val b = p.flat
        // front
        b.put(l, 0f, zf, c.r, c.g, c.b, alpha); b.put(r, 0f, zf, c.r, c.g, c.b, alpha); b.put(r, h, zf, c.r, c.g, c.b, alpha)
        b.put(l, 0f, zf, c.r, c.g, c.b, alpha); b.put(r, h, zf, c.r, c.g, c.b, alpha); b.put(l, h, zf, c.r, c.g, c.b, alpha)
        // the sides, a little darker
        val s = 0.62f
        b.put(l, 0f, zb, c.r * s, c.g * s, c.b * s, alpha); b.put(l, 0f, zf, c.r * s, c.g * s, c.b * s, alpha); b.put(l, h, zf, c.r * s, c.g * s, c.b * s, alpha)
        b.put(l, 0f, zb, c.r * s, c.g * s, c.b * s, alpha); b.put(l, h, zf, c.r * s, c.g * s, c.b * s, alpha); b.put(l, h, zb, c.r * s, c.g * s, c.b * s, alpha)
        b.put(r, 0f, zf, c.r * s, c.g * s, c.b * s, alpha); b.put(r, 0f, zb, c.r * s, c.g * s, c.b * s, alpha); b.put(r, h, zb, c.r * s, c.g * s, c.b * s, alpha)
        b.put(r, 0f, zf, c.r * s, c.g * s, c.b * s, alpha); b.put(r, h, zb, c.r * s, c.g * s, c.b * s, alpha); b.put(r, h, zf, c.r * s, c.g * s, c.b * s, alpha)
        if (top) {
            val u = 1.18f
            val tr = min(1f, c.r * u)
            val tg = min(1f, c.g * u)
            val tb = min(1f, c.b * u)
            b.put(l, h, zf, tr, tg, tb, alpha); b.put(r, h, zf, tr, tg, tb, alpha); b.put(r, h, zb, tr, tg, tb, alpha)
            b.put(l, h, zf, tr, tg, tb, alpha); b.put(r, h, zb, tr, tg, tb, alpha); b.put(l, h, zb, tr, tg, tb, alpha)
        }
    }

    override fun draw(p: Painter, f: Frame) {
        val t = f.t
        Matrix.perspectiveM(p.proj, 0, 46f, f.width.toFloat() / f.height, 0.1f, 80f)
        var kick = 0f
        for (d in drums) kick = max(kick, d.pulse(t, 0.14f))
        eye[0] = sin(t * 0.13f) * 1.4f + f.leanX * 3.2f
        eye[1] = (2.0f + sin(t * 0.09f) * 0.3f + kick * 0.05f - f.leanY * 1.2f).coerceAtLeast(0.5f)
        eye[2] = 5.6f
        Matrix.setLookAtM(p.view, 0, eye[0], eye[1], eye[2], 0f, 1.7f, -4f, 0f, 1f, 0f)
        p.fog(song.palette[0], 9f, 30f)

        height.fill(0f)
        for (k in tonal.indices) {
            val track = tonal[k]
            val to = track.upper(t)
            var from = to
            while (from > 0 && track.start[from - 1] > t - 4f) from--
            for (i in from until to) {
                val col = track.pitch[i] - low
                if (col < 0 || col >= columns) continue
                val age = t - track.start[i]
                // Struck, then held at a lower level for as long as the note lasts, then gone quickly.
                val struck = exp(-age / 0.3f)
                val held = if (age < track.length[i]) 0.55f else 0.55f * exp(-(age - track.length[i]) / 0.18f)
                val e = track.velocity[i] * max(struck, held)
                if (e > height[col]) {
                    height[col] = e
                    tint[col].set(colors[k])
                }
            }
        }

        // The horizon.
        GLES20.glDisable(GLES20.GL_DEPTH_TEST)
        p.drawGlow(0f, 0f, -22f, 60f, 14f, song.palette[1], 0.12f + min(0.75f, f.low * 4 + kick * 0.25f) + min(0.2f, f.level), true)

        // The floor: lines across, one passing for every beat, and lines away to the horizon.
        val beat = song.beatPhase(t)
        val a = 0.14f + 0.3f * kick
        p.flat.begin()
        for (i in 0 until 24) {
            val z = 6f - (i - beat) * 1.5f
            p.flat.put(-width, 0f, z, line.r, line.g, line.b, a)
            p.flat.put(width, 0f, z, line.r, line.g, line.b, a)
        }
        for (i in -14..14) {
            p.flat.put(i.toFloat(), 0f, 6f, line.r, line.g, line.b, a)
            p.flat.put(i.toFloat(), 0f, -30f, line.r, line.g, line.b, a)
        }
        GLES20.glLineWidth(2f)
        p.drawFlat(GLES20.GL_LINES, false)

        // The columns in the floor, then the columns.
        val dx = width / columns
        p.flat.begin()
        for (i in 0 until columns) {
            colour(i)
            box(p, (i - (columns - 1) / 2f) * dx, dx * 0.78f, -(0.04f + height[i] * 4.6f) * 0.8f, c, 0.22f, false)
        }
        p.drawFlat(GLES20.GL_TRIANGLES, false)
        GLES20.glEnable(GLES20.GL_DEPTH_TEST)
        p.flat.begin()
        for (i in 0 until columns) {
            colour(i)
            box(p, (i - (columns - 1) / 2f) * dx, dx * 0.78f, 0.04f + height[i] * 4.6f, c, 1f, true)
        }
        p.drawFlat(GLES20.GL_TRIANGLES, false)
        GLES20.glDisable(GLES20.GL_DEPTH_TEST)
    }

    private fun colour(i: Int) {
        if (height[i] > 0.001f) c.set(dark).lerp(tint[i], 0.35f + 0.65f * min(1f, height[i] * 1.3f))
        else c.set(dark).lerp(idle, 0.2f)
    }
}
