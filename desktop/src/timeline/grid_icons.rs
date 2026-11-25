//! The grid's own glyphs, as the iPad draws them (ios/ProCraft/Icons.swift's `grid…`), in the
//! shape of [`crate::icons::Icon`]: the shared icons written from Android's have none of them yet.

#![allow(dead_code)]
#![cfg_attr(rustfmt, rustfmt_skip)]

use crate::icons::{Icon, Part};

pub const GRID_LASSO: Icon = Icon { name: "grid-lasso", width: 24.0, height: 24.0, parts: &[
    Part { d: "M12 4.5C7.3 4.5 3.5 7 3.5 10.2C3.5 13.4 7.3 15.9 12 15.9C16.7 15.9 20.5 13.4 20.5 10.2C20.5 7 16.7 4.5 12 4.5z", stroke: 1.9, fill: false, even_odd: false },
    Part { d: "M7.2 15.2C5.6 16 4.9 17.4 5.4 18.6C5.9 19.8 7.6 20 8.4 19C9.2 18 8.6 16.6 7.2 16.4", stroke: 1.9, fill: false, even_odd: false },
] };

pub const GRID_INSERT: Icon = Icon { name: "grid-insert", width: 24.0, height: 24.0, parts: &[
    Part { d: "M6 5.5H18A2 2 0 0 1 20 7.5V16.5A2 2 0 0 1 18 18.5H6A2 2 0 0 1 4 16.5V7.5A2 2 0 0 1 6 5.5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M12 9V15", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M9 12H15", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_FILL: Icon = Icon { name: "grid-fill", width: 24.0, height: 24.0, parts: &[
    Part { d: "M14 5.5H6A2 2 0 0 0 4 7.5V16.5A2 2 0 0 0 6 18.5H14", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M9 12H20", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M16.5 8.5L20 12L16.5 15.5", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_OPTIONS: Icon = Icon { name: "grid-options", width: 24.0, height: 24.0, parts: &[
    Part { d: "M6 5.5H18A2 2 0 0 1 20 7.5V16.5A2 2 0 0 1 18 18.5H6A2 2 0 0 1 4 16.5V7.5A2 2 0 0 1 6 5.5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M8 10H16", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M8 14H13", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_BLEND: Icon = Icon { name: "grid-blend", width: 24.0, height: 24.0, parts: &[
    Part { d: "M6 5H18A2 2 0 0 1 20 7V17A2 2 0 0 1 18 19H6A2 2 0 0 1 4 17V7A2 2 0 0 1 6 5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M12 8.6A3.4 3.4 0 0 1 12 15.4A3.4 3.4 0 0 1 12 8.6z", stroke: 0.0, fill: true, even_odd: false },
] };

pub const GRID_MASK: Icon = Icon { name: "grid-mask", width: 24.0, height: 24.0, parts: &[
    Part { d: "M6 5H18A2 2 0 0 1 20 7V17A2 2 0 0 1 18 19H6A2 2 0 0 1 4 17V7A2 2 0 0 1 6 5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M12 8.6A3.4 3.4 0 0 1 12 15.4A3.4 3.4 0 0 1 12 8.6z", stroke: 1.5, fill: false, even_odd: false },
] };

pub const GRID_HIGHLIGHT: Icon = Icon { name: "grid-highlight", width: 24.0, height: 24.0, parts: &[
    Part { d: "M12 4.5A7.5 7.5 0 0 1 12 19.5A7.5 7.5 0 0 1 12 4.5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M6.8 17.2L17.2 6.8", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_RENAME: Icon = Icon { name: "grid-rename", width: 24.0, height: 24.0, parts: &[
    Part { d: "M5 19L6 15L16.5 4.5L19.5 7.5L9 18z", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_CLEAR: Icon = Icon { name: "grid-clear", width: 24.0, height: 24.0, parts: &[
    Part { d: "M13.5 5.5L18.5 10.5L11 18H7L4.5 15.5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M9 10L14 15", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M13 18.5H20", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_FLIP: Icon = Icon { name: "grid-flip", width: 24.0, height: 24.0, parts: &[
    Part { d: "M5 9H19", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M15.5 5.5L19 9L15.5 12.5", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M19 15H5", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M8.5 11.5L5 15L8.5 18.5", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_SCISSORS: Icon = Icon { name: "grid-scissors", width: 24.0, height: 24.0, parts: &[
    Part { d: "M7 14.5A2.5 2.5 0 0 1 7 19.5A2.5 2.5 0 0 1 7 14.5z", stroke: 1.6, fill: false, even_odd: false },
    Part { d: "M17 14.5A2.5 2.5 0 0 1 17 19.5A2.5 2.5 0 0 1 17 14.5z", stroke: 1.6, fill: false, even_odd: false },
    Part { d: "M8.5 15L16.5 4.5", stroke: 1.6, fill: false, even_odd: false },
    Part { d: "M15.5 15L7.5 4.5", stroke: 1.6, fill: false, even_odd: false },
] };

pub const GRID_COPY: Icon = Icon { name: "grid-copy", width: 24.0, height: 24.0, parts: &[
    Part { d: "M10 8.5H17A1.5 1.5 0 0 1 18.5 10V18A1.5 1.5 0 0 1 17 19.5H10A1.5 1.5 0 0 1 8.5 18V10A1.5 1.5 0 0 1 10 8.5z", stroke: 1.6, fill: false, even_odd: false },
    Part { d: "M15.5 8.5V6A1.5 1.5 0 0 0 14 4.5H7A1.5 1.5 0 0 0 5.5 6V14A1.5 1.5 0 0 0 7 15.5H8.5", stroke: 1.6, fill: false, even_odd: false },
] };

pub const GRID_PASTE: Icon = Icon { name: "grid-paste", width: 24.0, height: 24.0, parts: &[
    Part { d: "M9 6H7.5A1.5 1.5 0 0 0 6 7.5V18A1.5 1.5 0 0 0 7.5 19.5H16.5A1.5 1.5 0 0 0 18 18V7.5A1.5 1.5 0 0 0 16.5 6H15", stroke: 1.6, fill: false, even_odd: false },
    Part { d: "M10 4.5H14A1 1 0 0 1 15 5.5V6.5A1 1 0 0 1 14 7.5H10A1 1 0 0 1 9 6.5V5.5A1 1 0 0 1 10 4.5z", stroke: 1.6, fill: false, even_odd: false },
] };

pub const GRID_KEY: Icon = Icon { name: "grid-key", width: 24.0, height: 24.0, parts: &[
    Part { d: "M12 4.2L19.8 12L12 19.8L4.2 12z", stroke: 1.9, fill: false, even_odd: false },
    Part { d: "M12 9L15 12L12 15L9 12z", stroke: 0.0, fill: true, even_odd: false },
] };

pub const GRID_PHOTO: Icon = Icon { name: "grid-photo", width: 24.0, height: 24.0, parts: &[
    Part { d: "M5.5 5.5H18.5A2 2 0 0 1 20.5 7.5V16.5A2 2 0 0 1 18.5 18.5H5.5A2 2 0 0 1 3.5 16.5V7.5A2 2 0 0 1 5.5 5.5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M4 16L8.6 11.6L12 14.6L14.8 12.4L20 16.6", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M15.8 8A1.3 1.3 0 1 0 15.8 10.6A1.3 1.3 0 1 0 15.8 8z", stroke: 0.0, fill: true, even_odd: false },
] };

pub const GRID_FILE: Icon = Icon { name: "grid-file", width: 24.0, height: 24.0, parts: &[
    Part { d: "M3.5 7.5A2 2 0 0 1 5.5 5.5H9.3L11.3 7.8H18.5A2 2 0 0 1 20.5 9.8V16.5A2 2 0 0 1 18.5 18.5H5.5A2 2 0 0 1 3.5 16.5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M3.8 11H20.2", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_REMOVE: Icon = Icon { name: "grid-remove", width: 24.0, height: 24.0, parts: &[
    Part { d: "M6 5.5H18A2 2 0 0 1 20 7.5V16.5A2 2 0 0 1 18 18.5H6A2 2 0 0 1 4 16.5V7.5A2 2 0 0 1 6 5.5z", stroke: 1.7, fill: false, even_odd: false },
    Part { d: "M9 12H15", stroke: 1.7, fill: false, even_odd: false },
] };

pub const GRID_BROOM: Icon = Icon { name: "grid-broom", width: 24.0, height: 24.0, parts: &[
    Part { d: "M19.5 4.5L13 11", stroke: 1.9, fill: false, even_odd: false },
    Part { d: "M10.4 9.6L14.4 13.6L9.4 19.4L4.6 14.6z", stroke: 0.0, fill: true, even_odd: false },
] };
