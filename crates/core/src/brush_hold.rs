//! A brush held in the library, as the original does it: held still [`BRUSH_MENU_HOLD`] (moving
//! no further than [`BRUSH_HOLD_SLOP`]), its menu shows while the finger is still down (let go
//! before, it was a tap, which chooses the brush). From then on the brush is lifted for as long as
//! the finger stays down: moved on past the slop, at any time, the menu goes and the brush follows
//! the finger. Let go without moving on, the menu stays and nothing is chosen. The apps follow the
//! touch; what a move does is [`brush_hold_move`]'s to say, and how long the hold still has to go
//! when an app first sees the touch, [`brush_menu_wait`]'s.

/// How long a brush is held still before its menu shows, seconds from the touch coming down: the
/// menu started showing 0.32 to 0.35 s into each hold measured.
pub const BRUSH_MENU_HOLD: f64 = 0.33;

/// How far (points) a finger holding a brush may move from where it came down and still be
/// holding it: further before the menu shows, it was no hold (the list or the row has it); further
/// once it shows, the brush lifts and follows the finger.
pub const BRUSH_HOLD_SLOP: f32 = 10.0;

/// How far back the touch's own clock may put its coming down when an app first sees it, seconds,
/// for the hold to count from then: further back (or ahead), that clock isn't one to go by (a
/// touch screen just made gives its first touch a time most of a second early), and the hold
/// counts from when the app saw it. A touch reaches an app some milliseconds after it came down,
/// a busy frame or two more; well inside the hold, so the press is never taken as held at once.
pub const BRUSH_TOUCH_CLOCK_TRUST: f64 = 0.25;

/// What the finger holding a brush does as it moves (see [`brush_hold_move`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrushHoldMove {
    /// Not yet past the slop: the hold goes on as it is.
    Stays,
    /// The brush lifts and follows the finger; its menu goes.
    Drags,
    /// The hold is over without lifting the brush, one that stays where it is (the Recent set's):
    /// the list under the menu has the touch (it scrolls up and down), and the menu stays. Nothing
    /// else has it: moved sideways, the brush's row doesn't slide open to its buttons (see
    /// [`brush_row_slides`]).
    Leaves,
}

/// [`BRUSH_MENU_HOLD`], for the apps.
#[uniffi::export]
pub fn brush_menu_hold_seconds() -> f64 {
    BRUSH_MENU_HOLD
}

/// [`BRUSH_HOLD_SLOP`], for the apps (points: an app working in pixels takes it times its
/// density).
#[uniffi::export]
pub fn brush_hold_slop() -> f32 {
    BRUSH_HOLD_SLOP
}

/// How much longer a brush held still waits before its menu shows, seconds, when the app first
/// sees the touch `since_down` seconds after it came down by the touch's own clock: the menu shows
/// [`BRUSH_MENU_HOLD`] after the finger landed, not after the app saw it. A time that isn't a number,
/// one ahead of the app's clock or further back than [`BRUSH_TOUCH_CLOCK_TRUST`] is a clock not to go
/// by: the hold then counts from now.
#[uniffi::export]
pub fn brush_menu_wait(since_down: f64) -> f64 {
    if !(0.0..=BRUSH_TOUCH_CLOCK_TRUST).contains(&since_down) {
        return BRUSH_MENU_HOLD;
    }
    (BRUSH_MENU_HOLD - since_down).max(0.0)
}

/// Whether a press on a brush's row may slide the row sideways to show its buttons: not once the
/// brush's menu has shown under it (`menu_shown`). From then on the press is the hold's until let go:
/// moved on it lifts the brush (one of the Recent set's leaves it to the list under the menu, which
/// only scrolls up and down, the menu staying).
#[uniffi::export]
pub fn brush_row_slides(menu_shown: bool) -> bool {
    !menu_shown
}

/// What the finger holding a brush whose menu has shown does, moved `moved` from where it came down
/// (the app's points or pixels, as `slop` is: [`BRUSH_HOLD_SLOP`] in the app's units): within the
/// slop, nothing; past it the brush lifts to be dragged, however long it was held, if it can be
/// (`movable`: the Recent set's brushes stay where they are, and the list has the touch). A
/// distance that isn't a number is taken as no move.
#[uniffi::export]
pub fn brush_hold_move(moved: f32, slop: f32, movable: bool) -> BrushHoldMove {
    if moved.is_nan() || moved <= slop.max(0.0) {
        BrushHoldMove::Stays
    } else if movable {
        BrushHoldMove::Drags
    } else {
        BrushHoldMove::Leaves
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The holds seen on the original, each then moved on: presses of 0.4 and 0.55 s, and holds of
    /// about 1.2 s with the menu full for most of a second first, all lifted the brush. A hold has
    /// no time after which a move stops lifting it: any time after the menu, a move past the slop
    /// drags (a report: a brush held a little long couldn't be moved at all).
    #[test]
    fn a_held_brush_moved_on_lifts_however_long_it_was_held() {
        let hold = brush_menu_hold_seconds();
        assert!((0.32..=0.35).contains(&hold) && hold > 0.3, "{hold}");
        // the moves are the same however long the hold was: there is no clock in the rule
        for moved in [10.5, 30.0, 36.0, 400.0] {
            assert_eq!(brush_hold_move(moved, BRUSH_HOLD_SLOP, true), BrushHoldMove::Drags, "{moved}");
        }
    }

    #[test]
    fn the_hold_lets_the_finger_wander_ten_points() {
        assert_eq!(brush_hold_slop(), 10.0);
        for moved in [0.0, 1.0, 3.5, 7.9, 9.99, 10.0] {
            assert_eq!(brush_hold_move(moved, BRUSH_HOLD_SLOP, true), BrushHoldMove::Stays, "{moved}");
            assert_eq!(brush_hold_move(moved, BRUSH_HOLD_SLOP, false), BrushHoldMove::Stays, "{moved}");
        }
        assert_eq!(brush_hold_move(10.01, BRUSH_HOLD_SLOP, true), BrushHoldMove::Drags);
        // in pixels at three times the density: 30
        assert_eq!(brush_hold_move(29.0, BRUSH_HOLD_SLOP * 3.0, true), BrushHoldMove::Stays);
        assert_eq!(brush_hold_move(31.0, BRUSH_HOLD_SLOP * 3.0, true), BrushHoldMove::Drags);
        // a move that isn't a number is no move; a slop below nothing is none
        assert_eq!(brush_hold_move(f32::NAN, BRUSH_HOLD_SLOP, true), BrushHoldMove::Stays);
        assert_eq!(brush_hold_move(0.5, -4.0, true), BrushHoldMove::Drags);
        assert_eq!(brush_hold_move(f32::INFINITY, BRUSH_HOLD_SLOP, true), BrushHoldMove::Drags);
    }

    /// The menu shows the hold after the finger landed: an app seeing the touch late waits only the
    /// rest of it, and one that saw it after the whole hold shows it at once.
    #[test]
    fn the_menu_counts_from_when_the_finger_landed() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // seen at once: the whole 0.33 s; seen 40 ms or 90 ms late (the 0.37 to 0.42 s the menu
        // showed at, timed from when the app saw the touch): 0.29 and 0.24 s more
        assert!(close(brush_menu_wait(0.0), 0.33), "{}", brush_menu_wait(0.0));
        assert!(close(brush_menu_wait(0.040), 0.29), "{}", brush_menu_wait(0.040));
        assert!(close(brush_menu_wait(0.090), 0.24), "{}", brush_menu_wait(0.090));
        // so the menu shows at 0.33 s from the touch however late (up to a quarter second) the app
        // saw it, and always some time after the app saw it (never held at once)
        for late in [0.0, 0.004, 0.016, 0.040, 0.090, 0.2, 0.25] {
            assert!(close(late + brush_menu_wait(late), BRUSH_MENU_HOLD), "seen {late} s late");
            assert!(brush_menu_wait(late) >= 0.08, "seen {late} s late");
        }
        // a clock not to go by, the hold counted from now: a virtual touch screen's first touch
        // (0.9 s early, which made a press moved at once lift and drag its brush), one ahead of
        // the app's clock, one far off, or not a number
        for odd in [0.26, 0.5, 0.9, -0.004, -3.0, 4_000_000.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(brush_menu_wait(odd), BRUSH_MENU_HOLD, "{odd}");
        }
    }

    /// Moved sideways once the menu has shown, a brush lifts (it doesn't slide its row open to its
    /// buttons), and a brush of the Recent set leaves the press to the list, which only scrolls.
    #[test]
    fn a_sideways_move_after_the_menu_opens_no_buttons() {
        assert!(!brush_row_slides(true));
        assert_eq!(brush_hold_move(30.0, BRUSH_HOLD_SLOP, true), BrushHoldMove::Drags);
        assert_eq!(brush_hold_move(30.0, BRUSH_HOLD_SLOP, false), BrushHoldMove::Leaves);
        // a press whose menu hasn't shown (moved before 0.33 s) slides its row
        assert!(brush_row_slides(false));
    }

    #[test]
    fn the_recent_sets_brushes_never_lift() {
        assert_eq!(brush_hold_move(40.0, BRUSH_HOLD_SLOP, false), BrushHoldMove::Leaves);
        assert_eq!(brush_hold_move(4.0, BRUSH_HOLD_SLOP, false), BrushHoldMove::Stays);
    }
}
