package dev.agb.duo

import android.app.Activity
import android.app.ActivityOptions
import android.content.Context
import android.content.Intent
import android.graphics.RectF
import android.graphics.drawable.GradientDrawable
import android.hardware.display.DisplayManager
import android.media.AudioManager
import android.os.Bundle
import android.text.InputType
import android.util.Log
import android.util.TypedValue
import android.view.Display
import android.view.Gravity
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout

/**
 * The app on a handheld with two screens (it was made for the Retroid Pocket Duo Lite: a
 * 1920×1080 screen over a 1280×960 one that is touched, with a pad at their sides).
 *
 * This activity is the stage, on the main screen: the scene of what is being heard and the words
 * that are sung, nothing else. The console is an activity of its own on the other screen
 * ([ConsoleActivity]); with one screen only it is laid over a corner of the stage. What the two
 * share is in [Studio].
 */
class MainActivity : Activity() {
    private var root: FrameLayout? = null
    private var stage: StageView? = null
    private var inline: ConsolePanel? = null
    private var moved = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Started from the lower screen: the stage belongs on the main one, so go there.
        val here = display?.displayId ?: Display.DEFAULT_DISPLAY
        if (here != Display.DEFAULT_DISPLAY && !intent.getBooleanExtra(MOVED, false)) {
            moved = true
            val again = Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK).putExtra(MOVED, true)
            startActivity(again, ActivityOptions.makeBasic().setLaunchDisplayId(Display.DEFAULT_DISPLAY).toBundle())
            finish()
            return
        }
        instance = this
        Studio.start(this)
        volumeControlStream = AudioManager.STREAM_MUSIC
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val views = FrameLayout(this)
        stage = StageView(this).also { views.addView(it, FrameLayout.LayoutParams(-1, -1)) }
        views.addView(LyricsView(this), FrameLayout.LayoutParams(-1, -1))
        views.setOnClickListener { Studio.togglePlay() }
        // With the pad in use the system would light up the view that has the keys: the stage shows the scene only.
        views.defaultFocusHighlightEnabled = false
        root = views
        setContentView(views)
        hideBars(window)
        getSystemService(DisplayManager::class.java).registerDisplayListener(displays, null)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && !moved) hideBars(window)
    }

    private val displays = object : DisplayManager.DisplayListener {
        override fun onDisplayAdded(displayId: Int) = arrange()
        override fun onDisplayRemoved(displayId: Int) = arrange()
        override fun onDisplayChanged(displayId: Int) {}
    }

    /** Put the console on the other screen when there is one, else over a corner of the stage. */
    private fun arrange() {
        val views = root ?: return
        if (isFinishing) return
        val mine = display?.displayId ?: Display.DEFAULT_DISPLAY
        val other = getSystemService(DisplayManager::class.java).displays.firstOrNull { it.displayId != mine && it.state != Display.STATE_OFF && it.flags and Display.FLAG_PRIVATE == 0 }
        if (other != null) {
            inline?.let { views.removeView(it) }
            inline = null
            if (ConsoleActivity.instance?.display?.displayId != other.displayId) {
                try {
                    startActivity(Intent(this, ConsoleActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK), ActivityOptions.makeBasic().setLaunchDisplayId(other.displayId).toBundle())
                } catch (e: Exception) {
                    Log.w("agb", "the console could not go to the other screen: ${e.message}")
                }
            }
        } else if (inline == null) {
            ConsoleActivity.instance?.finish()
            val h = resources.displayMetrics.heightPixels
            val lp = FrameLayout.LayoutParams(h * 4 / 3 * 2 / 3, h * 2 / 3)
            lp.gravity = Gravity.BOTTOM or Gravity.END
            inline = ConsolePanel(this).also { views.addView(it, lp) }
        }
    }

    override fun onStart() {
        super.onStart()
        if (moved) return
        arrange()
        stage?.onResume()
        Studio.wake()
    }

    override fun onStop() {
        super.onStop()
        if (moved) return
        stage?.onPause()
        // Left: the music does not go on by itself behind another app.
        if (!isChangingConfigurations && ConsoleActivity.instance?.hasWindowFocus() != true) Studio.rest()
    }

    override fun onDestroy() {
        super.onDestroy()
        if (moved) return
        getSystemService(DisplayManager::class.java).unregisterDisplayListener(displays)
        if (instance === this) instance = null
        if (isFinishing) {
            Studio.rest()
            ConsoleActivity.instance?.finish()
        }
    }

    override fun dispatchKeyEvent(event: KeyEvent) = (!moved && Studio.key(event)) || super.dispatchKeyEvent(event)
    override fun dispatchGenericMotionEvent(event: MotionEvent) = (!moved && Studio.stick(event)) || super.dispatchGenericMotionEvent(event)

    companion object {
        const val MOVED = "dev.agb.duo.moved"
        @Volatile var instance: MainActivity? = null

        fun hideBars(window: android.view.Window) {
            window.insetsController?.let {
                it.hide(WindowInsets.Type.systemBars())
                it.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        }
    }
}

/** The console on the second screen: an activity, so that it can be typed in and has the pad's keys when it is touched. */
class ConsoleActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        instance = this
        Studio.start(this)
        volumeControlStream = AudioManager.STREAM_MUSIC
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContentView(ConsolePanel(this))
        MainActivity.hideBars(window)
    }

    override fun onDestroy() {
        super.onDestroy()
        if (instance === this) instance = null
        // closed by the listener: the stage goes with it
        if (isFinishing) MainActivity.instance?.finish()
    }

    override fun dispatchKeyEvent(event: KeyEvent) = Studio.key(event) || super.dispatchKeyEvent(event)
    override fun dispatchGenericMotionEvent(event: MotionEvent) = Studio.stick(event) || super.dispatchGenericMotionEvent(event)

    /** "Back" (a swipe from the edge of the screen, on this device) closes what lies over the console, never the app. */
    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        Studio.overlay?.close()
    }

    companion object {
        @Volatile var instance: ConsoleActivity? = null
    }
}

/** The console with its text fields: real ones, laid over the drawn view where it says they belong. */
class ConsolePanel(context: Context) : FrameLayout(context), ConsoleView.Host {
    private val view = ConsoleView(context)
    private val edits = LinkedHashMap<String, EditText>()
    private val shown = HashMap<String, RectF>()

    init {
        addView(view, LayoutParams(-1, -1))
        view.host = this
        defaultFocusHighlightEnabled = false
        view.defaultFocusHighlightEnabled = false
        for ((name, hint) in listOf("brief" to "写下你想听的…", "chat" to "对乐手说…", "key" to "粘贴或输入 API Key", "base" to Prefs.DEFAULT_BASE, "models" to "例如 gemini-3-flash-preview,gpt-5-mini")) {
            val e = EditText(context)
            e.hint = hint
            e.setSingleLine(true)
            e.setTextColor(0xfff1ede4.toInt())
            e.setHintTextColor(0x66f1ede4)
            e.background = GradientDrawable().apply {
                cornerRadius = 12f
                setColor(0x30ffffff)
            }
            e.inputType = if (name == "brief" || name == "chat") InputType.TYPE_CLASS_TEXT else InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
            e.imeOptions = (if (name == "chat") EditorInfo.IME_ACTION_SEND else EditorInfo.IME_ACTION_DONE) or EditorInfo.IME_FLAG_NO_EXTRACT_UI
            e.setOnEditorActionListener { _, action, event ->
                // "send" on the keyboard of the screen, or Enter on one that is plugged in
                val enter = event != null && event.keyCode == KeyEvent.KEYCODE_ENTER && event.action == KeyEvent.ACTION_DOWN
                if (name == "chat" && (action == EditorInfo.IME_ACTION_SEND || enter)) {
                    Studio.chat(e.text.toString())
                    e.setText("")
                }
                if (action == EditorInfo.IME_ACTION_DONE) done()
                true
            }
            e.visibility = View.GONE
            edits[name] = e
            addView(e, LayoutParams(10, 10))
        }
    }

    // Each frame: the fields go where the view has drawn their places.
    private val place = object : Runnable {
        override fun run() {
            for ((name, e) in edits) {
                val r = view.fields[name]
                if (r == null) {
                    if (e.visibility != View.GONE) {
                        if (e.hasFocus()) done()
                        e.visibility = View.GONE
                        shown.remove(name)
                    }
                } else if (shown[name] != r) {
                    shown[name] = RectF(r)
                    val lp = LayoutParams(r.width().toInt(), r.height().toInt())
                    lp.leftMargin = r.left.toInt()
                    lp.topMargin = r.top.toInt()
                    e.layoutParams = lp
                    e.setTextSize(TypedValue.COMPLEX_UNIT_PX, r.height().coerceAtMost(height / 480f * 30f) * 0.5f)
                    val pad = (r.height() * 0.3f).toInt()
                    e.setPadding(pad, 0, pad, 0)
                    e.visibility = View.VISIBLE
                }
            }
            if (isAttachedToWindow) postOnAnimation(this)
        }
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        postOnAnimation(place)
    }

    override fun text(field: String) = edits[field]?.text?.toString() ?: ""

    override fun setText(field: String, value: String) {
        edits[field]?.let {
            it.setText(value)
            it.setSelection(value.length)
        }
    }

    override fun done() {
        context.getSystemService(InputMethodManager::class.java)?.hideSoftInputFromWindow(windowToken, 0)
        edits.values.forEach { it.clearFocus() }
    }
}
