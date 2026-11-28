//! The colour panel shows its Sphere tab while this plugin is on.

#[unsafe(no_mangle)]
pub extern "C" fn procraft_colour_panel(part: i32) -> i32 {
    const SPHERE_TAB: i32 = 2;
    i32::from(part == SPHERE_TAB)
}
