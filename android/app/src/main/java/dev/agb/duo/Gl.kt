package dev.agb.duo

import android.opengl.GLES20
import android.util.Log
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer

// The little of OpenGL ES 2 the scenes draw with: glowing points, coloured lines and triangles
// (with fog), and a soft light on a rectangle.

private fun compile(type: Int, source: String): Int {
    val s = GLES20.glCreateShader(type)
    GLES20.glShaderSource(s, source)
    GLES20.glCompileShader(s)
    val ok = IntArray(1)
    GLES20.glGetShaderiv(s, GLES20.GL_COMPILE_STATUS, ok, 0)
    if (ok[0] == 0) Log.e("agb", "shader: " + GLES20.glGetShaderInfoLog(s))
    return s
}

class Program(vertex: String, fragment: String) {
    val id: Int = GLES20.glCreateProgram()
    private val places = HashMap<String, Int>()

    init {
        GLES20.glAttachShader(id, compile(GLES20.GL_VERTEX_SHADER, vertex))
        GLES20.glAttachShader(id, compile(GLES20.GL_FRAGMENT_SHADER, fragment))
        GLES20.glLinkProgram(id)
        val ok = IntArray(1)
        GLES20.glGetProgramiv(id, GLES20.GL_LINK_STATUS, ok, 0)
        if (ok[0] == 0) Log.e("agb", "program: " + GLES20.glGetProgramInfoLog(id))
    }

    fun attribute(name: String) = places.getOrPut(name) { GLES20.glGetAttribLocation(id, name) }
    fun uniform(name: String) = places.getOrPut(name) { GLES20.glGetUniformLocation(id, name) }
}

/** Vertices gathered for one draw call and thrown away after it. */
class Batch(private val stride: Int, capacity: Int) {
    private val data = FloatArray(stride * capacity)
    private val buffer: FloatBuffer = ByteBuffer.allocateDirect(data.size * 4).order(ByteOrder.nativeOrder()).asFloatBuffer()
    private var n = 0

    val count get() = n / stride

    fun begin() {
        n = 0
    }

    // One vertex at a time, without making an array for it (these run thousands of times a frame).
    fun put(a: Float, b: Float, c: Float, d: Float, e: Float) {
        if (n + 5 > data.size) return
        data[n] = a; data[n + 1] = b; data[n + 2] = c; data[n + 3] = d; data[n + 4] = e
        n += 5
    }

    fun put(a: Float, b: Float, c: Float, d: Float, e: Float, f: Float, g: Float) {
        if (n + 7 > data.size) return
        data[n] = a; data[n + 1] = b; data[n + 2] = c; data[n + 3] = d; data[n + 4] = e; data[n + 5] = f; data[n + 6] = g
        n += 7
    }

    fun put(a: Float, b: Float, c: Float, d: Float, e: Float, f: Float, g: Float, h: Float) {
        if (n + 8 > data.size) return
        data[n] = a; data[n + 1] = b; data[n + 2] = c; data[n + 3] = d; data[n + 4] = e; data[n + 5] = f; data[n + 6] = g; data[n + 7] = h
        n += 8
    }

    /** Hand the vertices to `program`: its attributes in order, and how many floats each has. */
    fun draw(program: Program, mode: Int, names: Array<String>, sizes: IntArray) {
        if (n == 0) return
        buffer.clear()
        buffer.put(data, 0, n)
        var offset = 0
        for (i in names.indices) {
            val at = program.attribute(names[i])
            if (at >= 0) {
                buffer.position(offset)
                GLES20.glEnableVertexAttribArray(at)
                GLES20.glVertexAttribPointer(at, sizes[i], GLES20.GL_FLOAT, false, stride * 4, buffer)
            }
            offset += sizes[i]
        }
        GLES20.glDrawArrays(mode, 0, n / stride)
        for (name in names) program.attribute(name).let { if (it >= 0) GLES20.glDisableVertexAttribArray(it) }
    }
}

private val POINT_NAMES = arrayOf("aPos", "aColor", "aSize", "aAlpha")
private val POINT_SIZES = intArrayOf(3, 3, 1, 1)
private val FLAT_NAMES = arrayOf("aPos", "aColor")
private val FLAT_SIZES = intArrayOf(3, 4)
private val GLOW_NAMES = arrayOf("aPos", "aUv")
private val GLOW_SIZES = intArrayOf(3, 2)

/** What every scene draws with. Made on the GL thread, once per surface. */
class Painter {
    private val pointsProgram = Program(
        """
        attribute vec3 aPos; attribute vec3 aColor; attribute float aSize; attribute float aAlpha;
        uniform mat4 uView; uniform mat4 uProj; uniform float uScale;
        varying float vAlpha; varying vec3 vColor;
        void main() {
            vAlpha = aAlpha; vColor = aColor;
            vec4 mv = uView * vec4(aPos, 1.0);
            gl_PointSize = aSize * uScale / max(0.1, -mv.z);
            gl_Position = uProj * mv;
        }""",
        """
        precision mediump float;
        varying float vAlpha; varying vec3 vColor;
        void main() {
            float d = length(gl_PointCoord - 0.5) * 2.0;
            float a = smoothstep(1.0, 0.0, d);
            gl_FragColor = vec4(vColor * (vAlpha * a * a), 1.0);
        }""",
    )
    private val flatProgram = Program(
        """
        attribute vec3 aPos; attribute vec4 aColor;
        uniform mat4 uView; uniform mat4 uProj;
        varying vec4 vColor; varying float vDepth;
        void main() {
            vColor = aColor;
            vec4 mv = uView * vec4(aPos, 1.0);
            vDepth = -mv.z;
            gl_Position = uProj * mv;
        }""",
        """
        precision mediump float;
        uniform vec3 uFogColor; uniform vec2 uFog; uniform float uAdd;
        varying vec4 vColor; varying float vDepth;
        void main() {
            float f = uFog.y > 0.0 ? clamp((vDepth - uFog.x) / (uFog.y - uFog.x), 0.0, 1.0) : 0.0;
            vec3 c = mix(vColor.rgb, uFogColor, f * (1.0 - uAdd));
            float a = vColor.a * (1.0 - f * uAdd);
            gl_FragColor = uAdd > 0.5 ? vec4(c * a, 1.0) : vec4(c, a);
        }""",
    )
    private val glowProgram = Program(
        """
        attribute vec3 aPos; attribute vec2 aUv;
        uniform mat4 uView; uniform mat4 uProj;
        varying vec2 vUv;
        void main() { vUv = aUv; gl_Position = uProj * uView * vec4(aPos, 1.0); }""",
        """
        precision mediump float;
        uniform vec3 uColor; uniform float uPower; uniform float uBand;
        varying vec2 vUv;
        void main() {
            vec2 p = abs(vUv - 0.5) * 2.0;
            float disc = pow(max(0.0, 1.0 - length(p)), 2.2);
            float band = pow(1.0 - p.y, 3.0) * (1.0 - p.x * p.x);
            gl_FragColor = vec4(uColor * (mix(disc, band, uBand) * uPower), 1.0);
        }""",
    )

    val points = Batch(8, 6000)
    val flat = Batch(7, 24000)
    private val quad = Batch(5, 6)

    val view = FloatArray(16)
    val proj = FloatArray(16)
    /** Height of the picture in pixels over 2·tan(fov/2): what a point of size 1 at distance 1 measures. */
    var pointScale = 1f
    private val fogColor = FloatArray(3)
    private var fogNear = 0f
    private var fogFar = 0f

    fun fog(color: Int, near: Float, far: Float) {
        fogColor[0] = (color shr 16 and 255) / 255f
        fogColor[1] = (color shr 8 and 255) / 255f
        fogColor[2] = (color and 255) / 255f
        fogNear = near
        fogFar = far
    }

    fun noFog() {
        fogFar = 0f
    }

    /** Draw [points]: lights that add up. */
    fun drawPoints() {
        val p = pointsProgram
        GLES20.glUseProgram(p.id)
        GLES20.glUniformMatrix4fv(p.uniform("uView"), 1, false, view, 0)
        GLES20.glUniformMatrix4fv(p.uniform("uProj"), 1, false, proj, 0)
        GLES20.glUniform1f(p.uniform("uScale"), pointScale)
        GLES20.glBlendFunc(GLES20.GL_ONE, GLES20.GL_ONE)
        points.draw(p, GLES20.GL_POINTS, POINT_NAMES, POINT_SIZES)
    }

    /** Draw [flat] as lines or triangles; `add`: as light, not as paint. */
    fun drawFlat(mode: Int, add: Boolean) {
        val p = flatProgram
        GLES20.glUseProgram(p.id)
        GLES20.glUniformMatrix4fv(p.uniform("uView"), 1, false, view, 0)
        GLES20.glUniformMatrix4fv(p.uniform("uProj"), 1, false, proj, 0)
        GLES20.glUniform3fv(p.uniform("uFogColor"), 1, fogColor, 0)
        GLES20.glUniform2f(p.uniform("uFog"), fogNear, fogFar)
        GLES20.glUniform1f(p.uniform("uAdd"), if (add) 1f else 0f)
        if (add) GLES20.glBlendFunc(GLES20.GL_ONE, GLES20.GL_ONE) else GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE_MINUS_SRC_ALPHA)
        flat.draw(p, mode, FLAT_NAMES, FLAT_SIZES)
    }

    /**
     * A soft light on the rectangle centred at (x, y, z), `w` by `h`, facing the viewer along z:
     * a disc, or (`band`) a glow along the middle that fades to the sides.
     */
    fun drawGlow(x: Float, y: Float, z: Float, w: Float, h: Float, color: Int, power: Float, band: Boolean) {
        val p = glowProgram
        GLES20.glUseProgram(p.id)
        GLES20.glUniformMatrix4fv(p.uniform("uView"), 1, false, view, 0)
        GLES20.glUniformMatrix4fv(p.uniform("uProj"), 1, false, proj, 0)
        GLES20.glUniform3f(p.uniform("uColor"), (color shr 16 and 255) / 255f, (color shr 8 and 255) / 255f, (color and 255) / 255f)
        GLES20.glUniform1f(p.uniform("uPower"), power)
        GLES20.glUniform1f(p.uniform("uBand"), if (band) 1f else 0f)
        GLES20.glBlendFunc(GLES20.GL_ONE, GLES20.GL_ONE)
        val l = x - w / 2
        val r = x + w / 2
        val b = y - h / 2
        val t = y + h / 2
        quad.begin()
        quad.put(l, b, z, 0f, 0f)
        quad.put(r, b, z, 1f, 0f)
        quad.put(r, t, z, 1f, 1f)
        quad.put(l, b, z, 0f, 0f)
        quad.put(r, t, z, 1f, 1f)
        quad.put(l, t, z, 0f, 1f)
        quad.draw(p, GLES20.GL_TRIANGLES, GLOW_NAMES, GLOW_SIZES)
    }
}

/** A colour as three floats, and mixing two of them. */
class Rgb(var r: Float = 0f, var g: Float = 0f, var b: Float = 0f) {
    constructor(color: Int) : this((color shr 16 and 255) / 255f, (color shr 8 and 255) / 255f, (color and 255) / 255f)

    fun set(o: Rgb): Rgb {
        r = o.r
        g = o.g
        b = o.b
        return this
    }

    fun lerp(o: Rgb, x: Float): Rgb {
        r += (o.r - r) * x
        g += (o.g - g) * x
        b += (o.b - b) * x
        return this
    }
}

/** The accent colours of the palette spread over n things. */
fun spread(palette: IntArray, n: Int): List<Rgb> {
    val accents = palette.drop(1).map { Rgb(it) }
    return List(n) { i ->
        if (accents.size == 1 || n == 1) Rgb().set(accents[0])
        else {
            val x = i.toFloat() / (n - 1).coerceAtLeast(1) * (accents.size - 1)
            val k = minOf(accents.size - 2, x.toInt())
            Rgb().set(accents[k]).lerp(accents[k + 1], x - k)
        }
    }
}
