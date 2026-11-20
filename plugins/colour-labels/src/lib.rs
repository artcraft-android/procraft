//! Colour labels: Colour label, in a layer's options (a group's too), shows a panel of eight
//! colours; the one tapped labels the layer, and No label takes it off. Each layer keeps its label
//! as a mark the app saves with it (1 to 8, the colour's place), and the Layers panel shows the
//! colour on its row as the `layer_label` hook answers. Undo takes a label back as any change. It's
//! a plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.

use kit::app::{self, Layer, Panel};

/// The labels' colours (0xRRGGBB), a layer's mark the place of its own, from 1: red, orange, yellow,
/// green, teal, blue, purple and grey.
const COLOURS: [u32; 8] = [0xE5484D, 0xF2862E, 0xEFC11F, 0x46A758, 0x14A6A0, 0x3E7BFA, 0x9A5CD0, 0x8B8D98];

/// The layer the panel labels: the current one as Colour label was tapped.
static LABELLING: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(0);

/// The colour a layer marked `mark` shows: none for a mark that is no label.
fn colour_of(mark: i64) -> Option<u32> {
    usize::try_from(mark).ok()?.checked_sub(1).and_then(|at| COLOURS.get(at)).copied()
}

/// The panel, the label of the layer marked `mark` shown chosen.
fn panel(mark: i64) -> Panel {
    let chosen = colour_of(mark).map(|_| mark as usize - 1);
    Panel::new("Colour label").colours("Colour", &COLOURS, chosen).button("No label")
}

kit::actions!(|_| {
    let Some(layer) = app::current_layer() else { return };
    LABELLING.store(layer.0, core::sync::atomic::Ordering::Relaxed);
    panel(app::layer_mark(layer)).show();
});

kit::panel!(|row, value| {
    let layer = Layer(LABELLING.load(core::sync::atomic::Ordering::Relaxed));
    let mark = match row {
        0 if value >= 0.0 && (value as usize) < COLOURS.len() => value as i64 + 1,
        1 => 0,
        _ => return,
    };
    app::set_layer_mark(layer, mark);
    app::close_panel();
});

/// The colour (0xRRGGBB) the row of a layer marked `mark` shows, -1 for none.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_layer_label(mark: i64) -> i32 {
    colour_of(mark).map_or(-1, |c| c as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mark_from_1_to_8_is_its_colour_and_any_other_none() {
        assert_eq!(procraft_layer_label(1), 0xE5484D);
        assert_eq!(procraft_layer_label(8), 0x8B8D98);
        for mark in [0, -1, 9, i64::MAX, i64::MIN] {
            assert_eq!(procraft_layer_label(mark), -1, "{mark}");
        }
        // eight, each its own
        let mut seen = COLOURS.to_vec();
        seen.dedup();
        assert_eq!(seen.len(), 8);
    }

    #[test]
    fn the_panel_shows_the_layer_s_label_chosen() {
        let json = panel(3).json();
        assert!(json.contains(r#""type":"colours""#) && json.contains(r#""chosen":2"#), "{json}");
        assert!(json.contains(&COLOURS[0].to_string()) && json.contains("No label"), "{json}");
        let json = panel(0).json();
        assert!(!json.contains("chosen"), "{json}");
    }
}
