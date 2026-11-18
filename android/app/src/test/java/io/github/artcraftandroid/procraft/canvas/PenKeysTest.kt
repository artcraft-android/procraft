package io.github.artcraftandroid.procraft.canvas

import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import io.github.artcraftandroid.procraft.core.GestureAction
import io.github.artcraftandroid.procraft.core.PointerKind
import io.github.artcraftandroid.procraft.core.TouchSettings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PenKeysTest {
    private val up = KeyEvent.KEYCODE_PAGE_UP
    private val down = KeyEvent.KEYCODE_PAGE_DOWN
    private val keyboard = InputDevice.SOURCE_KEYBOARD
    private val letters = InputDevice.KEYBOARD_TYPE_ALPHABETIC
    private val noLetters = InputDevice.KEYBOARD_TYPE_NON_ALPHABETIC

    @Test
    fun aDoubleTapGivenTwiceCountsOnce() {
        val taps = PenDoubleTaps()
        assertEquals(true, taps.isNew(1_000))
        assertEquals(false, taps.isNew(1_020))
        // the next one, soon after, measured from the first of the pair
        assertEquals(true, taps.isNew(1_320))
        assertEquals(false, taps.isNew(1_340))
    }

    @Test
    fun aPensPageKeysAreItsFirstAndSecondButton() {
        val pen = keyboard or InputDevice.SOURCE_STYLUS
        assertEquals(false, penKeyButton(up, pen, noLetters, "touch"))
        assertEquals(true, penKeyButton(down, pen, noLetters, "touch"))
    }

    @Test
    fun aDeviceThatIsAStylusToo() {
        val touchScreen = keyboard or InputDevice.SOURCE_TOUCHSCREEN or InputDevice.SOURCE_STYLUS
        assertEquals(false, penKeyButton(up, touchScreen, letters, "digitizer"))
        val bluetooth = keyboard or InputDevice.SOURCE_BLUETOOTH_STYLUS
        assertEquals(true, penKeyButton(down, bluetooth, letters, "digitizer"))
    }

    @Test
    fun aKeyOnlyDeviceWithoutLetters() {
        assertEquals(false, penKeyButton(up, keyboard, noLetters, "hid-keys"))
    }

    @Test
    fun aDeviceNamedAsAPenThoughItListsAWholeKeyboard() {
        for (name in listOf("Redmi Smart Pen", "Xiaomi Smart Pen Keyboard", "pen", "S-PEN", "Active Stylus")) {
            assertEquals(name, true, penKeyButton(down, keyboard, letters, name))
        }
    }

    @Test
    fun aTypingKeyboardKeepsItsPageKeys() {
        for (name in listOf("Logitech K380", "AT Translated Set 2 keyboard", "Virtual", "Open Keyboard", "Pendragon 60", "Spen")) {
            assertNull(name, penKeyButton(up, keyboard, letters, name))
            assertNull(name, penKeyButton(down, keyboard, letters, name))
        }
    }

    @Test
    fun noOtherKeyIsAButton() {
        val pen = keyboard or InputDevice.SOURCE_STYLUS
        for (key in listOf(KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_MOVE_HOME, KeyEvent.KEYCODE_STYLUS_BUTTON_PRIMARY)) {
            assertNull(penKeyButton(key, pen, noLetters, "Smart Pen"))
        }
    }

    @Test
    fun aPensButtonsArePressedAndLetGoByWhatItsEventsSay() {
        val first = MotionEvent.BUTTON_STYLUS_PRIMARY
        val second = MotionEvent.BUTTON_STYLUS_SECONDARY
        val buttons = PenButtons()
        assertEquals(FIRST_BUTTON to 0, buttons.next(first, penIn = true))
        // still held: nothing new; another button's state that isn't a pen's counts for nothing
        assertEquals(0 to 0, buttons.next(first or MotionEvent.BUTTON_PRIMARY, penIn = true))
        // a hand landing while it is held reads as none held, and lets nothing go
        assertEquals(0 to 0, buttons.next(0, penIn = false))
        assertEquals(SECOND_BUTTON to FIRST_BUTTON, buttons.next(second, penIn = true))
        assertEquals(0 to SECOND_BUTTON, buttons.next(0, penIn = true))
    }

    @Test
    fun a_pen_s_second_button_is_heard_whichever_way_its_driver_gives_it() {
        val buttons = PenButtons()
        // as older drivers give a pen's buttons: the mouse's right and middle buttons
        assertEquals(SECOND_BUTTON to 0, buttons.next(MotionEvent.BUTTON_TERTIARY, penIn = true))
        assertEquals(0 to SECOND_BUTTON, buttons.next(0, penIn = true))
        assertEquals(FIRST_BUTTON to 0, buttons.next(MotionEvent.BUTTON_SECONDARY, penIn = true))
        assertEquals(0 to FIRST_BUTTON, buttons.next(0, penIn = true))
        // one button given both ways at once is still one press
        val both = MotionEvent.BUTTON_STYLUS_SECONDARY or MotionEvent.BUTTON_TERTIARY
        assertEquals(SECOND_BUTTON to 0, buttons.next(both, penIn = true))
        assertEquals(0 to 0, buttons.next(MotionEvent.BUTTON_TERTIARY, penIn = true))
        assertEquals(0 to SECOND_BUTTON, buttons.next(0, penIn = true))
        // the two pressed in one event are both pressed
        val two = MotionEvent.BUTTON_STYLUS_PRIMARY or MotionEvent.BUTTON_STYLUS_SECONDARY
        assertEquals((FIRST_BUTTON or SECOND_BUTTON) to 0, buttons.next(two, penIn = true))
        assertEquals(0 to (FIRST_BUTTON or SECOND_BUTTON), buttons.next(0, penIn = true))
        // a mouse's right or middle button (no pen in the event) is no pen's
        assertEquals(0 to 0, buttons.next(MotionEvent.BUTTON_SECONDARY or MotionEvent.BUTTON_TERTIARY, penIn = false))
        assertEquals(0, penButtonsIn(MotionEvent.BUTTON_PRIMARY or MotionEvent.BUTTON_BACK or MotionEvent.BUTTON_FORWARD))
    }

    @Test
    fun a_pen_s_second_button_that_comes_as_a_key_is_its_second() {
        // as Redmi's Smart Pen gives its two buttons: Page Up, Page Down
        val pen = keyboard or InputDevice.SOURCE_STYLUS
        assertEquals(true, penKeyButton(down, pen, noLetters, "Redmi Smart Pen"))
        assertEquals(true, penKeyButton(down, keyboard, letters, "Redmi Smart Pen"))
        assertEquals(false, penKeyButton(up, keyboard, letters, "Redmi Smart Pen"))
    }

    @Test
    fun a_button_held_over_the_canvas_isn_t_let_go_by_touch_events_that_leave_it_out() {
        val first = MotionEvent.BUTTON_STYLUS_PRIMARY
        val buttons = PenButtons()
        // held while hovering
        assertEquals(FIRST_BUTTON to 0, buttons.next(first, penIn = true))
        // the pen touches and moves, its events without the button: still held
        assertEquals(0 to 0, buttons.next(0, penIn = true, touching = true))
        assertEquals(0 to 0, buttons.next(0, penIn = true, touching = true))
        // lifted (or hovering) without it: let go
        assertEquals(0 to FIRST_BUTTON, buttons.next(0, penIn = true))
        // pressed while touching still counts at once
        assertEquals(FIRST_BUTTON to 0, buttons.next(first, penIn = true, touching = true))
        assertEquals(0 to FIRST_BUTTON, buttons.next(0, penIn = true))
    }

    @Test
    fun a_button_held_through_a_stroke_whose_events_leave_it_out_is_one_press() {
        val first = MotionEvent.BUTTON_STYLUS_PRIMARY
        val buttons = PenButtons()
        // pressed hovering: once
        assertEquals(FIRST_BUTTON to 0, buttons.next(first, penIn = true))
        // the stroke, its lift too, without it: still held, not let go and pressed again as the pen
        // hovers on with it (which switched to the eraser and straight back)
        assertEquals(0 to 0, buttons.next(0, penIn = true, touching = true))
        assertEquals(0 to 0, buttons.next(0, penIn = true, lifting = true))
        assertEquals(0 to 0, buttons.next(first, penIn = true))
        // let go hovering
        assertEquals(0 to FIRST_BUTTON, buttons.next(0, penIn = true))
        // the same, let go during the stroke: the next hover says so
        assertEquals(FIRST_BUTTON to 0, buttons.next(first, penIn = true))
        assertEquals(0 to 0, buttons.next(0, penIn = true, touching = true))
        assertEquals(0 to 0, buttons.next(0, penIn = true, lifting = true))
        assertEquals(0 to FIRST_BUTTON, buttons.next(0, penIn = true))
    }

    @Test
    fun a_pen_whose_touches_show_its_buttons_lets_go_as_it_lifts_without_one() {
        val first = MotionEvent.BUTTON_STYLUS_PRIMARY
        val buttons = PenButtons()
        // pressed and let go during strokes, a pen with no hover at all: each lift says
        assertEquals(FIRST_BUTTON to 0, buttons.next(first, penIn = true, touching = true))
        assertEquals(0 to FIRST_BUTTON, buttons.next(0, penIn = true, lifting = true))
        // held into the next stroke, let go before it lifts
        assertEquals(FIRST_BUTTON to 0, buttons.next(first, penIn = true, touching = true))
        assertEquals(0 to 0, buttons.next(0, penIn = true, touching = true))
        assertEquals(0 to FIRST_BUTTON, buttons.next(0, penIn = true, lifting = true))
    }

    @Test
    fun which_events_are_the_pen_on_the_glass_and_lifting() {
        for (a in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE, MotionEvent.ACTION_POINTER_DOWN, MotionEvent.ACTION_POINTER_UP)) {
            assertEquals(true, penTouching(a))
            assertEquals(false, penLifting(a))
        }
        assertEquals(true, penLifting(MotionEvent.ACTION_UP))
        assertEquals(false, penTouching(MotionEvent.ACTION_UP))
        for (a in listOf(MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT, MotionEvent.ACTION_BUTTON_PRESS, MotionEvent.ACTION_BUTTON_RELEASE, MotionEvent.ACTION_CANCEL)) {
            assertEquals(false, penTouching(a))
            assertEquals(false, penLifting(a))
        }
    }

    @Test
    fun a_mouse_and_a_touchpads_pointer_are_the_mouse_and_a_touchpads_swipes_fingers() {
        val screen = InputDevice.SOURCE_TOUCHSCREEN
        val mouse = InputDevice.SOURCE_MOUSE
        assertEquals(PointerKind.STYLUS, pointerKind(MotionEvent.TOOL_TYPE_STYLUS, screen or InputDevice.SOURCE_STYLUS))
        assertEquals(PointerKind.STYLUS_ERASER, pointerKind(MotionEvent.TOOL_TYPE_ERASER, InputDevice.SOURCE_STYLUS))
        assertEquals(PointerKind.FINGER, pointerKind(MotionEvent.TOOL_TYPE_FINGER, screen))
        assertEquals(PointerKind.MOUSE, pointerKind(MotionEvent.TOOL_TYPE_MOUSE, mouse))
        // a touchpad moving the pointer comes as a finger from the mouse's source
        assertEquals(PointerKind.MOUSE, pointerKind(MotionEvent.TOOL_TYPE_FINGER, mouse or InputDevice.SOURCE_TOUCHPAD))
        assertEquals(PointerKind.MOUSE, pointerKind(MotionEvent.TOOL_TYPE_UNKNOWN, mouse))
        // its two-finger swipe, its three-finger swipe and its pinch move the view as fingers do
        for (gesture in listOf(3, 4, 5)) assertEquals(PointerKind.FINGER, pointerKind(MotionEvent.TOOL_TYPE_FINGER, mouse, gesture))
        // a pen's own source says nothing of a mouse
        assertEquals(PointerKind.STYLUS, pointerKind(MotionEvent.TOOL_TYPE_STYLUS, mouse))
    }

    @Test
    fun only_a_mouses_primary_button_paints() {
        assertEquals(true, mousePressPaints(MotionEvent.BUTTON_PRIMARY))
        assertEquals(true, mousePressPaints(MotionEvent.BUTTON_PRIMARY or MotionEvent.BUTTON_SECONDARY))
        // a touchpad's tap, or a press made up by the system, says of no button
        assertEquals(true, mousePressPaints(0))
        for (other in listOf(MotionEvent.BUTTON_SECONDARY, MotionEvent.BUTTON_TERTIARY, MotionEvent.BUTTON_BACK, MotionEvent.BUTTON_FORWARD)) {
            assertEquals(false, mousePressPaints(other))
        }
    }

    /** The touch settings as the shared touches ship them: a mouse paints as the pen does. */
    private fun shipped(fingerPaint: Boolean, oneFingerPan: Boolean, modelFingerPaint: Boolean) = TouchSettings(
        fingerPaint = fingerPaint, oneFingerPan = oneFingerPan, undoTaps = true, rotate = true, modelFingerPaint = modelFingerPaint,
        drawAndHold = true, touch = null, pen = null, hold = GestureAction.EYEDROPPER, modifyTouch = null, modifyPen = null,
        fourTap = null, threeSwipe = null, penButton = null, penSecondButton = null, scrub = true, hoverTouch = false,
        holdMs = 400.0, quickshapeMs = 650.0, rapidUndoMs = 650.0,
    )

    @Test
    fun on_the_tablet_with_painting_with_a_finger_off_the_mouse_paints_nothing_as_a_finger() {
        assertEquals(false, shipped(fingerPaint = false, oneFingerPan = true, modelFingerPaint = false).pointerFollowsTouch)
        for ((paint, pan) in listOf(false to true, false to false, true to false)) {
            val tablet = tabletTouchSettings(shipped(paint, pan, modelFingerPaint = false))
            // the mouse does what a finger is set to: the finger's settings kept as they were
            assertEquals(true, tablet.pointerFollowsTouch)
            assertEquals(shipped(paint, pan, modelFingerPaint = false), tablet.copy(pointerFollowsTouch = false))
        }
    }

    @Test
    fun on_the_tablet_the_mouse_turns_a_model_as_a_finger_unless_a_finger_paints_it() {
        for (modelPaint in listOf(false, true)) {
            val tablet = tabletTouchSettings(shipped(fingerPaint = true, oneFingerPan = false, modelFingerPaint = modelPaint))
            assertEquals(true, tablet.pointerFollowsTouch)
            assertEquals(modelPaint, tablet.modelFingerPaint)
        }
    }
}
