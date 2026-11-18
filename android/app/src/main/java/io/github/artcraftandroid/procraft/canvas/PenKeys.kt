package io.github.artcraftandroid.procraft.canvas

import android.os.Build
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import io.github.artcraftandroid.procraft.core.PointerKind
import io.github.artcraftandroid.procraft.core.TouchSettings

/** The key Huawei's M-Pencil gives for a double-tap, twice over for each one. */
internal const val PEN_DOUBLE_TAP_KEY = 718

/** Presses of that key closer than this (ms) are one double-tap given twice. */
private const val DOUBLE_TAP_GIVEN_TWICE_MS = 300L

/** What a pen's button that comes as a key did: pressed, let go, or (a double-tap's key) tapped, never held. */
enum class PenKeyAction { PRESSED, LET_GO, DOUBLE_TAPPED }

/**
 * A pen's first side button, as a motion event's button state has it: Android's own bit, or the
 * mouse's right button, which older pens' drivers give it as (Android did before it had its own).
 */
internal const val PEN_FIRST = MotionEvent.BUTTON_STYLUS_PRIMARY or MotionEvent.BUTTON_SECONDARY

/** Its second, the same way: Android's own bit, or the mouse's middle button. */
internal const val PEN_SECOND = MotionEvent.BUTTON_STYLUS_SECONDARY or MotionEvent.BUTTON_TERTIARY

/** A pen's two side buttons, as a motion event's button state has them. */
internal const val PEN_BUTTONS = PEN_FIRST or PEN_SECOND

/** The pen's first and second side button, as [PenButtons] tells them: each whichever bit it came as. */
internal const val FIRST_BUTTON = 1
internal const val SECOND_BUTTON = 2

/** Which of the pen's side buttons ([FIRST_BUTTON], [SECOND_BUTTON]) [buttonState] has held. */
internal fun penButtonsIn(buttonState: Int): Int =
    (if (buttonState and PEN_FIRST != 0) FIRST_BUTTON else 0) or (if (buttonState and PEN_SECOND != 0) SECOND_BUTTON else 0)

/** Whether a pointer of [toolType] is a pen's (its tip or its eraser end). */
internal fun isPen(toolType: Int) = toolType == MotionEvent.TOOL_TYPE_STYLUS || toolType == MotionEvent.TOOL_TYPE_ERASER

/**
 * A touchpad's gestures, as an event's classification says (Android 14's
 * `CLASSIFICATION_TWO_FINGER_SWIPE`, `CLASSIFICATION_MULTI_FINGER_SWIPE` and `CLASSIFICATION_PINCH`):
 * fingers on the pad moving the view, not the pointer.
 */
private val TOUCHPAD_GESTURES = setOf(3, 4, 5)

/**
 * What a pointer is to the canvas, by its [toolType], the [source] its event came from and the
 * event's [classification] (0 where the system has none): a pen's tip or eraser end; a mouse, or a
 * touchpad moving the pointer (Android gives that one a finger's tool type), which does what a
 * touch is set to do, with no pressure, and is never taken for a hand resting by the pen; else a
 * finger. A touchpad's own swipes and pinches stay fingers'.
 */
internal fun pointerKind(toolType: Int, source: Int, classification: Int = 0): PointerKind = when {
    toolType == MotionEvent.TOOL_TYPE_STYLUS -> PointerKind.STYLUS
    toolType == MotionEvent.TOOL_TYPE_ERASER -> PointerKind.STYLUS_ERASER
    toolType == MotionEvent.TOOL_TYPE_MOUSE -> PointerKind.MOUSE
    source and InputDevice.SOURCE_MOUSE == InputDevice.SOURCE_MOUSE && classification !in TOUCHPAD_GESTURES -> PointerKind.MOUSE
    else -> PointerKind.FINGER
}

/** What pointer [index] of [e] is to the canvas (see [pointerKind]). */
internal fun pointerKindOf(e: MotionEvent, index: Int): PointerKind =
    pointerKind(e.getToolType(index), e.source, if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) e.classification else 0)

/**
 * [settings] as the tablet takes them: a mouse, or a touchpad moving the pointer, does what a finger
 * is set to do. With painting with a finger off it paints nothing (a drag does what a finger's
 * does), and on a model it turns the model; it still has no pressure and is never taken for a hand
 * resting by the pen.
 */
internal fun tabletTouchSettings(settings: TouchSettings): TouchSettings = settings.copy(pointerFollowsTouch = true)

/**
 * Whether a mouse's press with [buttonState] held paints: its primary button's does (and a press
 * that says of no button, as a touchpad's tap may), its others' do nothing on the canvas.
 */
internal fun mousePressPaints(buttonState: Int): Boolean = buttonState == 0 || buttonState and MotionEvent.BUTTON_PRIMARY != 0

/** The pen's side buttons as its events say they are held, from one event to the next. */
internal class PenButtons {
    private var held = 0

    /** The pen's touch events have shown a button held: its driver puts them in, so one missing as it lifts was let go. */
    private var touchesShowButtons = false

    /**
     * The buttons pressed and let go ([FIRST_BUTTON], [SECOND_BUTTON]) since the last event, by this
     * one's [buttonState]. An event without the pen in it ([penIn] false: a hand's touch, whose buttons read
     * as none held) changes nothing, or a hand landing while a button is held would let it go. While
     * the pen is [touching], a button that goes missing isn't let go yet: some pens' touch events
     * leave out a button their hover shows held, which would hand the eraser back as the stroke
     * begins. As the pen [lifting] lifts it is let go, unless its touches have never shown a button:
     * then the lift leaves it out too, and the next hover says whether it is still held (let go
     * there and pressed again as the pen hovers on, one press would switch to the eraser and
     * straight back, and the size slider with it).
     */
    fun next(buttonState: Int, penIn: Boolean, touching: Boolean = false, lifting: Boolean = false): Pair<Int, Int> {
        if (!penIn) return 0 to 0
        val shown = penButtonsIn(buttonState)
        if ((touching || lifting) && shown != 0) touchesShowButtons = true
        val kept = touching || (lifting && !touchesShowButtons)
        val now = shown or (if (kept) held else 0)
        val change = (now and held.inv()) to (held and now.inv())
        held = now
        return change
    }
}

/** Whether a motion event of [action] is the pen on the glass ([PenButtons.next]'s touching), or lifting from it (its lifting). */
internal fun penTouching(action: Int): Boolean = when (action) {
    MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE, MotionEvent.ACTION_POINTER_DOWN, MotionEvent.ACTION_POINTER_UP -> true
    else -> false
}

internal fun penLifting(action: Int): Boolean = action == MotionEvent.ACTION_UP

/** Tells a pen's next double-tap from the second press of the one just heard. */
internal class PenDoubleTaps {
    private var heardAt: Long? = null

    /** Whether a press at [at] (ms) is a double-tap of its own. */
    fun isNew(at: Long): Boolean {
        val last = heardAt
        if (last != null && at - last < DOUBLE_TAP_GIVEN_TWICE_MS) return false
        heardAt = at
        return true
    }
}

/** A device named as a pen, as a word of its own ("Smart Pen", not "Open Keyboard"). */
private val PEN_NAME = Regex("""\b(pen|stylus)\b""", RegexOption.IGNORE_CASE)

/**
 * Which of a pen's two side buttons [keyCode] is (true: the second), from a device that has
 * [sources], is of [keyboardType] and is called [name]; null if it is no pen's. Some pens, as
 * Redmi's Smart Pen, give their buttons as Page Up and Page Down. A key's own source says keyboard
 * whatever else its device is, so this goes by the device: a stylus, one without letters, or one
 * named as a pen (a pen paired over Bluetooth may list a whole keyboard's keys). Those keys stay a
 * typing keyboard's own.
 */
internal fun penKeyButton(keyCode: Int, sources: Int, keyboardType: Int, name: String): Boolean? {
    val second = when (keyCode) {
        KeyEvent.KEYCODE_PAGE_UP -> false
        KeyEvent.KEYCODE_PAGE_DOWN -> true
        else -> return null
    }
    val stylus = sources and InputDevice.SOURCE_STYLUS == InputDevice.SOURCE_STYLUS
    val pen = stylus || keyboardType != InputDevice.KEYBOARD_TYPE_ALPHABETIC || PEN_NAME.containsMatchIn(name)
    return second.takeIf { pen }
}
